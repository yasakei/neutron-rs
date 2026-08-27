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
//! The readiness backend is selected at compile time: `epoll` on Linux,
//! `kqueue` on macOS/BSD, and a portable `poll` fallback elsewhere. All system
//! calls are isolated in this module and wrapped with documented safety
//! invariants; the rest of the crate stays entirely safe Rust.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use super::core::{self, GLOBAL};
use super::scheduler;

/// Set when the reactor thread should exit.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// A lazily-created self-pipe used to wake the reactor from any thread. The
/// read end is part of the reactor's wait set; writing a byte to the write end
/// unblocks a poll/epoll/kqueue wait and forces a re-scan.
struct WakePipe {
    read_fd: i32,
    write_fd: i32,
}

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

/// The interests the reactor should watch: io-core id -> (fd, read interest).
static FD_INTERESTS: LazyLock<Mutex<std::collections::HashMap<i64, (i32, bool)>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

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
fn wake_write() {
    let pipe = &*WAKE;
    // Pipe writes of one byte are atomic; a full buffer just means the reactor
    // is already awake, so the error is ignorable.
    let byte = 1u8;
    let _ = unsafe { libc::write(pipe.write_fd, std::ptr::addr_of!(byte).cast(), 1) };
}

/// Record (or update) a goroutine's interest in an fd. Called by a worker when
/// a goroutine parks on a descriptor.
pub(crate) fn register_fd(io: i64, fd: i64, read: bool) {
    if fd <= 0 {
        // No real descriptor yet: nothing to watch until a socket is attached
        // (see `ntask_io_attach_fd`). Wake so the goroutine re-polls and
        // reports ready=false immediately rather than hanging.
        wake_reactor();
        return;
    }
    let mut table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
    table.insert(io, (fd as i32, read));
    drop(table);
    wake_reactor();
}

/// Attach a raw fd to an io core so the reactor can watch it. `fd <= 0`
/// detaches.
pub(crate) fn attach_fd(io: i64, fd: i64, read: bool) {
    let mut table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
    if fd <= 0 {
        table.remove(&io);
    } else {
        table.insert(io, (fd as i32, read));
    }
    drop(table);
    wake_reactor();
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
}

/// The reactor thread: wait for timers and fd readiness, waking goroutines.
fn reactor_loop() {
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }
        // Fire due timers and compute the wait timeout to the next one.
        let timeout_ms = fire_timers_and_next();
        // Wait for fd readiness or a wake, with the timeout as a cap so timers
        // are never late.
        let ready = readiness_wait(timeout_ms);
        if !ready.is_empty() {
            wake_ready_ios(ready);
        }
    }
}

/// Fire all timers whose deadline has passed, waking their goroutines. Returns
/// the ms until the next pending timer, or `None` (=-1, block without timeout)
/// if none.
fn fire_timers_and_next() -> i64 {
    let now = core::now_ms();
    let mut g = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let due: Vec<i64> = g
        .timers
        .range(..=now)
        .flat_map(|(_, gids)| gids.iter().copied())
        .collect();
    g.timers.retain(|k, _| *k > now);
    let next = g.timers.keys().next().copied();
    drop(g);

    let mut woke = false;
    for gid in due {
        scheduler::make_runnable(gid);
        woke = true;
    }
    if woke {
        // make_runnable already notifies one worker each call.
    }

    match next {
        Some(deadline) => (deadline - now).max(1),
        None => -1,
    }
}

/// Wait for fd readiness (or a wake), blocking up to `timeout_ms` (-1 = wait
/// indefinitely). Returns the io-core ids whose goroutines should be re-polled.
fn readiness_wait(timeout_ms: i64) -> Vec<i64> {
    let interests: Vec<(i64, i32, bool)> = {
        let table = FD_INTERESTS.lock().unwrap_or_else(|p| p.into_inner());
        table
            .iter()
            .map(|(&io, &(fd, read))| (io, fd, read))
            .collect()
    };

    // The wake pipe read end is always watched for read readiness.
    let mut fds: Vec<libc::pollfd> = Vec::with_capacity(interests.len() + 1);
    let mut index_of_core: Vec<i64> = Vec::with_capacity(interests.len());
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

/// Drain all pending wake bytes from the self-pipe.
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
    let mut woke = false;
    for io in &ios {
        if let Some(slot) = g.ios.get_mut(io) {
            slot.ready = true;
            slot.parked = false;
            for waiter in std::mem::take(&mut slot.waiters) {
                g.ready.push_back(waiter);
                woke = true;
            }
        }
    }
    drop(g);
    if woke {
        scheduler::wake_workers_all();
    }
}
