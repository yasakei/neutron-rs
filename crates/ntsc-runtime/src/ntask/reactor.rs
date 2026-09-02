//! Reactor: a single background OS thread that (a) fires parked timer
//! goroutines and (b) parks goroutines waiting for file-descriptor readiness,
//! so workers never block on I/O.
//!
//! The reactor owns the readiness-wait fd set. Worker threads never touch it
//! directly; they only reach a small, mutex-guarded, cross-thread interface:
//!
//! * [`wake_reactor`] — pokes the reactor to re-scan timers and the interest
//!   table (via a self-pipe written to from any thread).
//! * [`register_fd`] — records a goroutine's fd interest for the reactor.
//!
//! The readiness backend is selected at compile time: `poll` on Unix (woken
//! through a non-blocking self-pipe), and `WSAPoll` on Windows (woken through
//! a loopback socket pair, since `WSAPoll` cannot wait on a kernel event). All
//! system calls are isolated in this module and wrapped with documented safety
//! invariants; the rest of the crate stays entirely safe Rust.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

#[cfg(windows)]
use std::io::{Read as _, Write as _};
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;

use super::core::{self, GLOBAL};
use super::scheduler;

/// Set when the reactor thread should exit.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// A lazily-created self-pipe used to wake the reactor from any thread. The
/// read end is part of the reactor's wait set; writing a byte to the write end
/// unblocks a poll wait and forces a re-scan.
#[cfg(unix)]
struct WakePipe {
    read_fd: i32,
    write_fd: i32,
}

#[cfg(unix)]
static WAKE: LazyLock<WakePipe> = LazyLock::new(|| unsafe {
    // `pipe` creates the two ends with `O_CLOEXEC`; both are owned by this
    // static for the program lifetime.
    let mut fds = [0i32; 2];
    let rc = libc::pipe(fds.as_mut_ptr());
    assert_eq!(rc, 0, "reactor: failed to create wake pipe");
    // Mark the read end non-blocking so a pending wake never wedges the loop.
    let flags = libc::fcntl(fds[0], libc::F_GETFL);
    let _ = libc::fcntl(fds[0], libc::F_SETFL, flags | libc::O_NONBLOCK);
    WakePipe {
        read_fd: fds[0],
        write_fd: fds[1],
    }
});

/// Windows has no `poll(2)`: the reactor waits with `WSAPoll`, the Winsock
/// equivalent, which accepts one `SOCKET` per entry instead of a file
/// descriptor. The socket pair in [`super::WAKE`] is included in every wait
/// set, so a byte written from any thread unblocks a wait, and `SetEvent` is
/// not needed.
#[cfg(windows)]
mod win {
    #[repr(C)]
    pub(crate) struct PollFd {
        /// `SOCKET` is a `UINT_PTR`, so `usize` has the right size and
        /// alignment on both 32- and 64-bit targets.
        pub(crate) fd: usize,
        pub(crate) events: i16,
        pub(crate) revents: i16,
    }

    pub(crate) const POLLRDNORM: i16 = 0x0100;
    pub(crate) const POLLWRNORM: i16 = 0x0010;
    pub(crate) const POLLERR: i16 = 0x0001;
    pub(crate) const POLLHUP: i16 = 0x0002;

    #[link(name = "ws2_32")]
    unsafe extern "system" {
        pub(crate) fn WSAPoll(fd_array: *mut PollFd, fds: u32, timeout: i32) -> i32;
    }
}

/// The wake channel on Windows: a loopback TCP pair whose read end is always
/// in the reactor's `WSAPoll` set. Writing one byte from any thread unblocks a
/// poll wait — the direct analogue of the Unix self-pipe, which `WSAPoll` (a
/// sockets-only wait) cannot replace with a plain event handle.
#[cfg(windows)]
struct WakeSocket {
    /// The raw `SOCKET` of the read end, cached so building the wait set never
    /// needs a lock.
    read_socket: usize,
    /// The watched end; drained by the reactor thread.
    read: Mutex<TcpStream>,
    /// The woken end; written by any thread.
    write: Mutex<TcpStream>,
}

#[cfg(windows)]
static WAKE: LazyLock<WakeSocket> = LazyLock::new(|| {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("reactor: failed to create wake socket listener");
    let addr = listener.local_addr().expect("reactor: wake socket address");
    let write = TcpStream::connect(addr).expect("reactor: wake socket connect");
    let (read, _) = listener.accept().expect("reactor: wake socket accept");
    // Both ends non-blocking: a wake write never stalls its caller, and the
    // reactor drains a pending byte without blocking.
    let _ = read.set_nonblocking(true);
    let _ = write.set_nonblocking(true);
    let read_socket = AsRawSocket::as_raw_socket(&read) as usize;
    WakeSocket {
        read_socket,
        read: Mutex::new(read),
        write: Mutex::new(write),
    }
});

/// The interests the reactor should watch: io-core id -> (fd, read interest).
/// The fd is the native handle keyed by the platform backend: an `int` fd on
/// Unix, a `SOCKET` (64-bit) on Windows, so it is kept at full width and cast
/// when the wait set is built.
static FD_INTERESTS: LazyLock<Mutex<std::collections::HashMap<i64, (i64, bool)>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Set when the interest table changed since the reactor last rebuilt its
/// pollfd array.
static INTERESTS_DIRTY: AtomicBool = AtomicBool::new(true);

/// Ensure the reactor thread is running and poke it to re-scan.
pub(crate) fn wake_reactor() {
    ensure_reactor_thread();
    wake_write();
}

/// Ensure the reactor thread is running.
fn ensure_reactor_thread() {
    scheduler::start();
    if REACTOR_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = std::thread::Builder::new()
        .name("ntask-reactor".into())
        .spawn(reactor_loop)
        .expect("spawn reactor");
    scheduler::register_reactor_handle(handle);
}

static REACTOR_STARTED: AtomicBool = AtomicBool::new(false);

/// Write a byte to the wake pipe so the reactor re-scans.
#[cfg(unix)]
fn wake_write() {
    let pipe = &*WAKE;
    // Pipe writes of one byte are atomic; a full buffer just means the reactor
    // is already awake, so the error is ignorable.
    let byte = 1u8;
    let _ = unsafe { libc::write(pipe.write_fd, std::ptr::addr_of!(byte).cast(), 1) };
}

/// Signal the wake socket so the reactor re-scans.
#[cfg(windows)]
fn wake_write() {
    // One byte wakes the poll; a full socket buffer just means the reactor is
    // already awake, so a WouldBlock error is ignorable (same contract as the
    // Unix self-pipe).
    let _ = WAKE
        .write
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .write(&[1]);
}

/// Record (or update) a goroutine's interest in an fd. Called by a worker when
/// a goroutine parks on a descriptor.
pub(crate) fn register_fd(io: i64, fd: i64, read: bool) {
    if fd <= 0 {
        // No real descriptor yet: nothing to watch until a socket is attached.
        // Wake so the goroutine re-polls and reports ready=false immediately
        // rather than hanging.
        wake_reactor();
        return;
    }
    let mut table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
    let changed = match table.get(&io) {
        Some(&(f, r)) => f != fd || r != read,
        None => true,
    };
    if changed {
        table.insert(io, (fd, read));
        drop(table);
        INTERESTS_DIRTY.store(true, Ordering::Release);
        wake_reactor();
    }
}

/// Attach a raw fd to an io core so the reactor can watch it. `fd <= 0`
/// detaches. Also records the descriptor on the core so a later park
/// re-registers it on its own (see [`register_fd`]).
pub(crate) fn attach_fd(io: i64, fd: i64, read: bool) {
    {
        let mut g = super::core::GLOBAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Some(slot) = g.ios.get_mut(&io) {
            slot.fd = fd;
        }
    }
    let mut table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
    if fd <= 0 {
        table.remove(&io);
    } else {
        table.insert(io, (fd, read));
    }
    drop(table);
    INTERESTS_DIRTY.store(true, Ordering::Release);
    wake_reactor();
}

/// Remove an io core's fd watch and wake the reactor. Safe to call on a core
/// with no recorded interest (or one already dropped). Only wakes when an
/// interest was actually present, so repeated drops do not churn.
pub(crate) fn detach_fd(io: i64) {
    let mut table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
    if table.remove(&io).is_some() {
        drop(table);
        INTERESTS_DIRTY.store(true, Ordering::Release);
        wake_reactor();
    }
}

/// Mark shutdown and wake the reactor so it exits promptly.
pub(crate) fn shutdown() {
    SHUTDOWN.store(true, Ordering::Relaxed);
    wake_write();
}

pub(crate) fn reset() {
    REACTOR_STARTED.store(false, Ordering::SeqCst);
    FD_INTERESTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    INTERESTS_DIRTY.store(true, Ordering::Release);
}

/// The readiness wait set, rebuilt by [`readiness_wait`]. On Unix it is a
/// `pollfd` array; on Windows no `pollfd` type exists (descriptor readiness is
/// not wired to the reactor yet), so it is an empty marker vector.
#[cfg(unix)]
type PollBuf = Vec<libc::pollfd>;

/// Windows `WSAPOLLFD` entries. Sockets are `SOCKET` handles (64-bit), so the
/// buffer is the `win::PollFd` array passed to `WSAPoll`; it mirrors the Unix
/// `pollfd` array so [`reactor_loop`] has one caller-owned reuse buffer across
/// platforms.
#[cfg(windows)]
type PollBuf = Vec<win::PollFd>;

/// The reactor thread: wait for timers and fd readiness, waking goroutines.
fn reactor_loop() {
    // The readiness buffer is reused; rebuilt only when the interest table changes.
    let mut fds: PollBuf = Vec::new();
    let mut index_of_core: Vec<i64> = Vec::new();
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }
        // Fire due timers and compute the wait timeout to the next one.
        let timeout_ms = fire_timers_and_next();
        // Wait for fd readiness or a wake, with the timeout as a cap so timers
        // are never late.
        let ready = readiness_wait(timeout_ms, &mut fds, &mut index_of_core);
        if !ready.is_empty() {
            wake_ready_ios(ready);
        }
    }
}

/// Fire all timers whose deadline has passed, waking their goroutines. Returns
/// the ms until the next pending timer, or `None` (=-1, block without timeout)
/// if none.
fn fire_timers_and_next() -> i64 {
    // Fast path: with no pending timers there is nothing to scan, so avoid
    // taking the global lock on every reactor iteration.
    if !core::has_pending_timers() {
        return -1;
    }
    let now = core::now_ms();
    let mut g = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let due: Vec<i64> = g
        .timers
        .range(..=now)
        .flat_map(|(_, gids)| gids.iter().copied())
        .collect();
    g.timers.retain(|k, _| *k > now);
    let next = g.timers.keys().next().copied();
    core::timers_pending_offset(-(due.len() as i64));
    drop(g);

    for gid in due {
        scheduler::make_runnable(gid);
    }

    match next {
        Some(deadline) => (deadline - now).max(1),
        None => -1,
    }
}

/// Wait for fd readiness (or a wake), blocking up to `timeout_ms` (-1 = wait
/// indefinitely). Returns the io-core ids whose goroutines should be re-polled.
/// `fds`/`index_of_core` are caller-owned buffers reused across waits; they are
/// only rebuilt when the interest table changed.
#[cfg(unix)]
fn readiness_wait(
    timeout_ms: i64,
    fds: &mut Vec<libc::pollfd>,
    index_of_core: &mut Vec<i64>,
) -> Vec<i64> {
    if INTERESTS_DIRTY.swap(false, Ordering::AcqRel) {
        let interests: Vec<(i64, i32, bool)> = {
            let table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
            table
                .iter()
                .map(|(&io, &(fd, read))| (io, fd as i32, read))
                .collect()
        };
        // The wake pipe read end is always watched for read readiness.
        fds.clear();
        index_of_core.clear();
        fds.push(libc::pollfd {
            fd: WAKE.read_fd,
            events: libc::POLLIN,
            revents: 0,
        });
        for (io, fd, read) in &interests {
            fds.push(libc::pollfd {
                fd: *fd,
                events: if *read { libc::POLLIN } else { libc::POLLOUT },
                revents: 0,
            });
            index_of_core.push(*io);
        }
    }

    let timeout_c = if timeout_ms < 0 {
        -1
    } else {
        timeout_ms.min(i64::from(i32::MAX)) as i32
    };
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_c) };
    if rc <= 0 {
        // Consume any wake byte and return nothing ready.
        drain_wake();
        return Vec::new();
    }

    // Drain the wake pipe (a wake byte may accompany genuine readiness).
    if fds[0].revents & libc::POLLIN != 0 {
        drain_wake();
    }

    let mut ready = Vec::new();
    for (i, slot) in fds.iter().enumerate().skip(1) {
        if slot.revents & (libc::POLLIN | libc::POLLOUT | libc::POLLERR | libc::POLLHUP) != 0 {
            ready.push(index_of_core[i - 1]);
        }
    }
    ready
}

/// Windows backend: `WSAPoll` over the registered sockets plus the wake socket,
/// with the same rebuild-on-dirty behavior as the Unix `poll` path. Sockets are
/// `SOCKET` handles rather than file descriptors, so only the interest table is
/// shared between the backends.
#[cfg(windows)]
fn readiness_wait(
    timeout_ms: i64,
    fds: &mut Vec<win::PollFd>,
    index_of_core: &mut Vec<i64>,
) -> Vec<i64> {
    if INTERESTS_DIRTY.swap(false, Ordering::AcqRel) {
        let interests: Vec<(i64, usize, bool)> = {
            let table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
            table
                .iter()
                .map(|(&io, &(fd, read))| (io, fd as usize, read))
                .collect()
        };
        // The wake socket is always watched for readability.
        fds.clear();
        index_of_core.clear();
        fds.push(win::PollFd {
            fd: WAKE.read_socket,
            events: win::POLLRDNORM,
            revents: 0,
        });
        for (io, fd, read) in &interests {
            fds.push(win::PollFd {
                fd: *fd,
                events: if *read {
                    win::POLLRDNORM
                } else {
                    win::POLLWRNORM
                },
                revents: 0,
            });
            index_of_core.push(*io);
        }
    }

    let timeout_c = if timeout_ms < 0 {
        -1
    } else {
        timeout_ms.min(i64::from(i32::MAX)) as i32
    };
    let rc = unsafe { win::WSAPoll(fds.as_mut_ptr(), fds.len() as u32, timeout_c) };
    if rc <= 0 {
        // Timeout (0) or failure (-1). A socket closed mid-wait can make the
        // whole call fail with `WSAENOTSOCK`; nothing is reported ready and the
        // dirty flag is re-armed so the next round rebuilds from the interest
        // table. The shutdown flag still ends the loop promptly.
        INTERESTS_DIRTY.store(true, Ordering::Release);
        drain_wake();
        return Vec::new();
    }

    // Drain the wake socket (a wake byte may accompany genuine readiness).
    if fds[0].revents & (win::POLLRDNORM | win::POLLERR | win::POLLHUP) != 0 {
        drain_wake();
    }

    let mut ready = Vec::new();
    for (i, slot) in fds.iter().enumerate().skip(1) {
        if slot.revents & (win::POLLRDNORM | win::POLLWRNORM | win::POLLERR | win::POLLHUP) != 0 {
            ready.push(index_of_core[i - 1]);
        }
    }
    ready
}

/// Drain all pending wake bytes from the self-pipe.
#[cfg(unix)]
fn drain_wake() {
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(WAKE.read_fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            break;
        }
    }
}

/// Drain all pending wake bytes from the wake socket. The read end is
/// non-blocking, so an empty socket reports `WouldBlock` and the drain ends.
#[cfg(windows)]
fn drain_wake() {
    let mut buf = [0u8; 64];
    loop {
        match WAKE
            .read
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .read(&mut buf)
        {
            Ok(0) | Err(_) => break,
            Ok(_) => continue,
        }
    }
}

/// Wake the goroutines parked on the given io cores (they re-poll and see
/// `ready`).
fn wake_ready_ios(ios: Vec<i64>) {
    let mut g = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut woken = 0usize;
    for io in &ios {
        if let Some(slot) = g.ios.get_mut(io) {
            slot.ready = true;
            slot.parked = false;
            for waiter in std::mem::take(&mut slot.waiters) {
                g.ready.push_back(waiter);
                core::sync_ready_len(&g);
                woken += 1;
            }
        }
    }
    drop(g);
    if woken > 0 {
        scheduler::wake_workers(woken);
    }
}
