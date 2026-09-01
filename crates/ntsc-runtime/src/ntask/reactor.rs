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
//! through a non-blocking self-pipe), and a `WaitForSingleObject` wake event
//! on Windows. All system calls are isolated in this module and wrapped with
//! documented safety invariants; the rest of the crate stays entirely safe
//! Rust.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

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

/// Windows has no `poll`: the reactor waits on a kernel32 event instead, and
/// `SetEvent` from any thread wakes the wait.
#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub(crate) const INFINITE: u32 = 0xFFFF_FFFF;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub(crate) fn CreateEventW(
            attributes: *mut c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> *mut c_void;
        pub(crate) fn SetEvent(event: *mut c_void) -> i32;
        pub(crate) fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    }
}

/// The wake handle on Windows: an auto-reset event. The wait clears it
/// atomically, so a `SetEvent` that races the wait re-signals the event and
/// the wake is never lost.
#[cfg(windows)]
struct WakeEvent {
    // Stored as a plain integer so the static stays `Sync`; only ever handed
    // back to kernel32 as a HANDLE.
    event: usize,
}

#[cfg(windows)]
static WAKE: LazyLock<WakeEvent> = LazyLock::new(|| {
    let event = unsafe { win::CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
    assert!(
        !event.is_null(),
        "reactor: failed to create wake event: {}",
        std::io::Error::last_os_error()
    );
    WakeEvent {
        event: event as usize,
    }
});

/// The interests the reactor should watch: io-core id -> (fd, read interest).
static FD_INTERESTS: LazyLock<Mutex<std::collections::HashMap<i64, (i32, bool)>>> =
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

/// Signal the wake event so the reactor re-scans.
#[cfg(windows)]
fn wake_write() {
    let _ = unsafe { win::SetEvent(WAKE.event as *mut std::ffi::c_void) };
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
        Some(&(f, r)) => f != fd as i32 || r != read,
        None => true,
    };
    if changed {
        table.insert(io, (fd as i32, read));
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
        table.insert(io, (fd as i32, read));
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

/// Windows has no `pollfd`: the backend waits on the wake event alone, so the
/// buffer is never populated. It still exists so [`reactor_loop`] has one
/// caller-owned reuse buffer across platforms.
#[cfg(windows)]
type PollBuf = Vec<u8>;

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
                .map(|(&io, &(fd, read))| (io, fd, read))
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

/// Windows backend: wait on the wake event with the timer timeout. Descriptor
/// readiness is not wired to a Windows backend yet (no language construct
/// registers fd interests there), so no io core is ever reported ready.
#[cfg(windows)]
fn readiness_wait(timeout_ms: i64, _fds: &mut PollBuf, _index_of_core: &mut Vec<i64>) -> Vec<i64> {
    let timeout_c = if timeout_ms < 0 {
        win::INFINITE
    } else {
        timeout_ms.min(i64::from(u32::MAX - 1)) as u32
    };
    // The auto-reset event clears itself when the wait returns; a `SetEvent`
    // that raced the wait re-signals it for the next wait, so there is nothing
    // to drain.
    let _ = unsafe { win::WaitForSingleObject(WAKE.event as *mut std::ffi::c_void, timeout_c) };
    Vec::new()
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
