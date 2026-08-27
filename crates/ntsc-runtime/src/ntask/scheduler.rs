//! M:N task scheduler: a fixed OS-thread pool multiplexing cheap stackless
//! goroutines onto `num_cpus` workers.
//!
//! Runnable goroutines are queued on one shared ready queue under the global
//! lock ([`crate::ntask::core::GLOBAL`]). Workers pop a goroutine, run its poll
//! function without the lock held, then re-lock to read its wait target and
//! requeue it, park it on a channel/timer/descriptor, or finish it. Because a
//! goroutine is a single stackless future (no worker thread-local async stack),
//! a torn-out goroutine can be picked up by any worker — that is what lets CPU
//! work spread across the pool.
//!
//! Blocking coordination (channel buffers, parked waiters, timers, fd
//! interests) is all inside the same [`GLOBAL`] mutex as the ready queue, so a
//! block/unblock decision is one atomic critical section: there is no lost
//! wakeup. Idle workers sleep on a condition variable; any spawn, channel
//! handoff, timer expiry, or fd-ready event re-notifies them.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, mpsc};
use std::thread::JoinHandle;

use super::core::{self, ChanOp, Goroutine, Park};
use crate::registry::NULL;

/// Idle-worker sleep signal. Workers wait on this when the ready queue is
/// empty; every path that makes a goroutine runnable notifies it.
pub(crate) static SIGNAL: LazyLock<(Mutex<()>, Condvar)> =
    LazyLock::new(|| (Mutex::new(()), Condvar::new()));

/// A blocking job handed to the bounded offload pool: run to completion on a
/// standalone thread so a scheduler worker is never blocked on I/O or a
/// child process.
type OffloadJob = Box<dyn FnOnce() + Send>;

static OFFLOADING: AtomicBool = AtomicBool::new(false);
static OFFLOAD_STOP: AtomicBool = AtomicBool::new(false);
static OFFLOAD_SEND: LazyLock<Mutex<Option<mpsc::Sender<OffloadJob>>>> =
    LazyLock::new(|| Mutex::new(None));
static OFFLOAD_THREADS: LazyLock<Mutex<Vec<JoinHandle<()>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

thread_local! {
    static CURRENT_GID: std::cell::RefCell<Option<i64>> =
        const { std::cell::RefCell::new(None) };
}

/// Workers launched by [`start`]; kept so [`shutdown`] can join them.
static WORKERS: LazyLock<Mutex<Vec<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static REACTOR: LazyLock<Mutex<Option<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(None));
type WorkerQueue = Arc<Mutex<VecDeque<i64>>>;
static QUEUES: LazyLock<Mutex<Vec<WorkerQueue>>> = LazyLock::new(|| Mutex::new(Vec::new()));

thread_local! {
    static WORKER_INDEX: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Start the worker pool. Idempotent. Call once (or let the first spawn
/// trigger it).
pub(crate) fn start() {
    if SHUTDOWN.load(Ordering::Relaxed) {
        return;
    }
    let mut workers = WORKERS.lock().unwrap_or_else(|p| p.into_inner());
    if !workers.is_empty() {
        return;
    }
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1);
    let queues: Vec<_> = (0..count)
        .map(|_| Arc::new(Mutex::new(VecDeque::new())))
        .collect();
    *QUEUES.lock().unwrap_or_else(|p| p.into_inner()) = queues;
    for index in 0..count {
        if let Ok(worker) = std::thread::Builder::new()
            .name(format!("ntask-worker-{index}"))
            .spawn(move || worker_loop(index))
        {
            workers.push(worker);
        }
    }
}

/// Start the bounded offload pool. Idempotent. A small, fixed number of
/// standalone threads run blocking jobs (`process.exec*`, socket transfers)
/// so a scheduler worker is never blocked on I/O or a child process. Bound
/// the count so runaway children cannot exhaust threads.
fn start_offload_pool() {
    if OFFLOADING.swap(true, Ordering::SeqCst) {
        return;
    }
    OFFLOAD_STOP.store(false, Ordering::Relaxed);
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(2, 8);
    let (tx, rx) = mpsc::channel::<OffloadJob>();
    *OFFLOAD_SEND.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    let rx = Arc::new(Mutex::new(rx));
    let mut threads = OFFLOAD_THREADS.lock().unwrap_or_else(|p| p.into_inner());
    for index in 0..count {
        let rx = Arc::clone(&rx);
        if let Ok(handle) = std::thread::Builder::new()
            .name(format!("ntask-offload-{index}"))
            .spawn(move || offload_worker(rx))
        {
            threads.push(handle);
        }
    }
}

/// A single offload-pool worker: pull jobs off the queue and run them to
/// completion.
fn offload_worker(rx: Arc<Mutex<mpsc::Receiver<OffloadJob>>>) {
    loop {
        let job = {
            let Ok(rx) = rx.lock() else {
                return;
            };
            match rx.recv() {
                Ok(job) => job,
                Err(_) => return,
            }
        };
        job();
        if OFFLOAD_STOP.load(Ordering::Relaxed) {
            return;
        }
    }
}

/// Enqueue a blocking job onto the offload pool. It runs on a standalone
/// thread; the caller registers an op, submits a job that completes it, and
/// parks the current goroutine on it.
pub(crate) fn run_offload(job: impl FnOnce() + Send + 'static) {
    start_offload_pool();
    let send = OFFLOAD_SEND.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(tx) = send.as_ref() {
        let _ = tx.send(Box::new(job));
    }
}

/// Park the current goroutine on an already-registered offloaded job
/// (used by the reactive future poll path, which registers the op itself so a
/// worker can complete it with the future's result).
pub(crate) fn park_op(core_id: i64) {
    park_self(Park::Job { core: core_id });
}

/// Spawn a goroutine running `poll` over the future `future`; returns its id.
/// The goroutine is scheduled immediately.
pub(crate) fn spawn(poll: core::PollFn, future: i64) -> i64 {
    start();
    let gid = core::register_goroutine(Goroutine {
        tasks: vec![(poll, future)],
        park: Park::None,
        pending_send: NULL,
        recv_result: NULL,
        done: false,
        result: NULL,
        pending_exception: NULL,
        joiners: Vec::new(),
    });
    make_runnable(gid);
    gid
}

/// Push a goroutine onto the ready queue and wake a worker.
pub(crate) fn make_runnable(gid: i64) {
    let local = WORKER_INDEX.with(|index| index.get());
    if let Some(index) = local
        && let Some(queue) = QUEUES
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(index)
            .cloned()
    {
        queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(gid);
        notify_one();
        return;
    }
    core::GLOBAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ready
        .push_back(gid);
    notify_one();
}

/// Notify a single idle worker.
fn notify_one() {
    let (lock, cvar) = &*SIGNAL;
    let _g = lock.lock().unwrap_or_else(|p| p.into_inner());
    cvar.notify_one();
}

/// Notify all workers (used at shutdown and when a burst of goroutines became
/// runnable).
pub(crate) fn notify_all() {
    let (lock, cvar) = &*SIGNAL;
    let _g = lock.lock().unwrap_or_else(|p| p.into_inner());
    cvar.notify_all();
}

/// The worker loop: pull a runnable goroutine and drive it, sleeping when
/// there is nothing to do.
fn worker_loop(index: usize) {
    WORKER_INDEX.with(|worker| worker.set(Some(index)));
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return;
        }
        let gid = pop_ready(index);
        match gid {
            Some(gid) => drive(gid),
            None => wait_for_work(),
        }
    }
}

/// Pop a runnable goroutine, or `None` when the ready queue is empty.
fn pop_ready(index: usize) -> Option<i64> {
    let queues = QUEUES.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if let Some(gid) = queues
        .get(index)
        .and_then(|queue| queue.lock().unwrap_or_else(|p| p.into_inner()).pop_front())
    {
        return Some(gid);
    }
    if let Some(gid) = core::GLOBAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ready
        .pop_front()
    {
        return Some(gid);
    }
    queues.iter().enumerate().find_map(|(other, queue)| {
        (other != index)
            .then(|| queue.lock().unwrap_or_else(|p| p.into_inner()).pop_back())
            .flatten()
    })
}

/// Sleep until a goroutine becomes runnable or we are asked to stop.
fn wait_for_work() {
    let (lock, cvar) = &*SIGNAL;
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            return;
        }
        if has_work() {
            return;
        }
        let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        let _ = cvar
            .wait_timeout(guard, std::time::Duration::from_millis(10))
            .unwrap_or_else(|p| p.into_inner());
    }
}

fn has_work() -> bool {
    if !core::GLOBAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ready
        .is_empty()
    {
        return true;
    }
    QUEUES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|queue| !queue.lock().unwrap_or_else(|p| p.into_inner()).is_empty())
}

/// Drive one goroutine to a suspension/completion point. The poll function
/// runs without the global lock held, so it may call back into the runtime.
fn drive(gid: i64) {
    let tasks = {
        let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
        match g.goroutines.get_mut(&gid) {
            Some(g) => std::mem::take(&mut g.tasks),
            None => return,
        }
    };
    CURRENT_GID.with(|cur| *cur.borrow_mut() = Some(gid));
    crate::install_async_tasks(tasks);
    crate::poll_async_tasks_once();
    let tasks = crate::take_async_tasks();
    let done = tasks.is_empty();
    CURRENT_GID.with(|cur| *cur.borrow_mut() = None);

    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let Some(gr) = g.goroutines.get_mut(&gid) else {
        return;
    };
    gr.tasks = tasks;
    if done {
        gr.done = true;
        // The poll ran on this worker, so an exception it raised (a throw with
        // no handler in an async body) is parked on this thread's TLS. Capture
        // it so the thread that joins the goroutine can re-raise it; otherwise
        // the uncaught exception would vanish with the worker thread.
        let thrown = crate::take_pending_exception_message();
        if thrown != NULL {
            gr.pending_exception = thrown;
        }
        let joiners = std::mem::take(&mut gr.joiners);
        for j in &joiners {
            g.ready.push_back(*j);
        }
        drop(g);
        notify_all();
        return;
    }
    let park = gr.park;
    gr.park = Park::None;
    let mut woke = false;
    let mut requeue_local = false;
    match park {
        Park::None => {
            requeue_local = true;
        }
        Park::Chan { core, op } => woke = chan_op(&mut g, gid, core, op),
        Park::Timer { at } => {
            g.timers.entry(at).or_default().push(gid);
            let pending = !g.ready.is_empty();
            drop(g);
            super::reactor::wake_reactor();
            if pending {
                notify_all();
            }
            return;
        }
        Park::Fd { io, read } => {
            let fd = g.ios.get_mut(&io).map(|slot| {
                slot.parked = true;
                slot.ready = false;
                slot.waiters.push(gid);
                slot.fd
            });
            let fd = fd.unwrap_or(0);
            drop(g);
            super::reactor::register_fd(io, fd, read);
            return;
        }
        Park::Join { target } => woke = join_park(&mut g, gid, target),
        Park::Job { core } => {
            let done = g.ops.get_mut(&core).map(|op| op.done).unwrap_or(false);
            if done {
                // The offload pool already finished it; the goroutine resumes
                // to reap the result on its next poll.
                requeue_local = true;
            } else if let Some(op) = g.ops.get_mut(&core) {
                op.waiter = Some(gid);
            } else {
                requeue_local = true;
            }
        }
    }
    drop(g);
    if requeue_local {
        make_runnable(gid);
        return;
    }
    // Always re-notify a sleeper: at least the requeued current goroutine (or a
    // woken opposite waiter / joiner) is runnable.
    if woke {
        notify_all();
    } else {
        notify_one();
    }
}

/// Park on a sibling goroutine's completion. Returns `true` if the target
/// already finished and this goroutine was requeued (so a wake must be sent).
fn join_park(g: &mut core::Global, gid: i64, target: i64) -> bool {
    let Some(done_result) = g
        .goroutines
        .get(&target)
        .map(|target_g| (target_g.done, target_g.result))
    else {
        g.ready.push_back(gid);
        return true;
    };
    if done_result.0 {
        if let Some(joiner) = g.goroutines.get_mut(&gid) {
            joiner.result = done_result.1;
        }
        g.ready.push_back(gid);
        true
    } else {
        if let Some(target_g) = g.goroutines.get_mut(&target) {
            target_g.joiners.push(gid);
        }
        false
    }
}

/// Perform a channel send/recv for a goroutine that just parked on it. Returns
/// `true` if an opposite waiter was woken (so a wake must be sent).
fn chan_op(g: &mut core::Global, gid: i64, core_id: i64, op: ChanOp) -> bool {
    match op {
        ChanOp::Send => chan_send(g, gid, core_id),
        ChanOp::Recv => chan_recv(g, gid, core_id),
    }
}

fn chan_send(g: &mut core::Global, gid: i64, core_id: i64) -> bool {
    let value = g
        .goroutines
        .get(&gid)
        .map(|gr| gr.pending_send)
        .unwrap_or(NULL);
    let Some(chan) = g.chans.get_mut(&core_id) else {
        // Channel gone: treat as a no-op send (value released by caller).
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.pending_send = NULL;
        }
        g.ready.push_back(gid);
        return false;
    };
    if chan.closed {
        if chan.owns_elements {
            let _ = crate::registry::remove(value);
        }
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.pending_send = NULL;
        }
        g.ready.push_back(gid);
        return false;
    }
    let cap = chan.cap;
    if let Some(receiver) = chan.receivers.pop_front() {
        if let Some(gr) = g.goroutines.get_mut(&receiver) {
            gr.recv_result = value;
        }
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.pending_send = NULL;
        }
        g.ready.push_back(receiver);
        g.ready.push_back(gid);
        return true;
    }
    if cap > 0 && chan.buf.len() < cap {
        // Buffered: room available. Push the value.
        chan.buf.push_back(value);
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.pending_send = NULL;
        }
        g.ready.push_back(gid);
        return false;
    }
    // Buffer full (or unbuffered with no receiver) — park as a sender, keeping
    // the value ready for handoff.
    chan.senders.push_back(gid);
    false
}

fn chan_recv(g: &mut core::Global, gid: i64, core_id: i64) -> bool {
    let Some(chan) = g.chans.get_mut(&core_id) else {
        // Channel gone: receive returns the zero value.
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = NULL;
        }
        g.ready.push_back(gid);
        return false;
    };
    let mut woke = false;
    if let Some(v) = chan.buf.pop_front() {
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = v;
        }
        // Refill the freed slot from a parked sender.
        if let Some(s) = chan.senders.pop_front() {
            let sv = g
                .goroutines
                .get_mut(&s)
                .map(|sg| sg.pending_send)
                .unwrap_or(NULL);
            if let Some(sg) = g.goroutines.get_mut(&s) {
                sg.pending_send = NULL;
            }
            chan.buf.push_back(sv);
            g.ready.push_back(s);
            woke = true;
        }
        g.ready.push_back(gid);
        return woke;
    }
    if let Some(s) = chan.senders.pop_front() {
        // Unbuffered: take the parked sender's value directly.
        let sv = g
            .goroutines
            .get_mut(&s)
            .map(|sg| sg.pending_send)
            .unwrap_or(NULL);
        if let Some(sg) = g.goroutines.get_mut(&s) {
            sg.pending_send = NULL;
        }
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = sv;
        }
        g.ready.push_back(s);
        g.ready.push_back(gid);
        return true;
    }
    if chan.closed {
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = NULL;
        }
        g.ready.push_back(gid);
        return false;
    }
    // Empty and open — park as a receiver.
    chan.receivers.push_back(gid);
    false
}

/// Close a channel: no more sends; parked receivers are woken to drain or see
/// the zero value; parked senders are released.
pub(crate) fn chan_close(core_id: i64) {
    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let Some(chan) = g.chans.get_mut(&core_id) else {
        return;
    };
    if chan.closed {
        return;
    }
    chan.closed = true;
    let receivers: Vec<i64> = chan.receivers.drain(..).collect();
    let senders: Vec<i64> = chan.senders.drain(..).collect();
    let count = receivers.len() + senders.len();
    let owns_elements = chan.owns_elements;
    let mut buffered = std::mem::take(&mut chan.buf);
    let mut abandoned = Vec::new();
    for r in receivers {
        if let Some(gr) = g.goroutines.get_mut(&r) {
            gr.recv_result = buffered.pop_front().unwrap_or(NULL);
        }
        g.ready.push_back(r);
    }
    for s in senders {
        if let Some(gr) = g.goroutines.get_mut(&s) {
            if gr.pending_send != NULL {
                abandoned.push(gr.pending_send);
            }
            gr.pending_send = NULL;
        }
        g.ready.push_back(s);
    }
    if owns_elements {
        abandoned.extend(buffered);
    }
    drop(g);
    if owns_elements {
        for value in abandoned {
            let _ = crate::registry::remove(value);
        }
    }
    if count > 0 {
        notify_all();
    }
}

/// The current goroutine id on this worker, if any.
pub(crate) fn current_gid() -> Option<i64> {
    CURRENT_GID.with(|cur| *cur.borrow())
}

/// The id of the goroutine this code is running in; panics if none.
/// Park the current goroutine with the given wait target (does not requeue).
pub(crate) fn park_self(park: Park) {
    let Some(gid) = current_gid() else {
        return;
    };
    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(gr) = g.goroutines.get_mut(&gid) {
        gr.park = park;
    }
}

/// Park the current goroutine on a channel send with the given value.
pub(crate) fn park_chan_send(core_id: i64, value: i64) {
    let Some(gid) = current_gid() else {
        return;
    };
    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(gr) = g.goroutines.get_mut(&gid) {
        gr.pending_send = value;
        gr.park = Park::Chan {
            core: core_id,
            op: ChanOp::Send,
        };
    }
}

/// Park the current goroutine on a channel receive.
pub(crate) fn park_chan_recv(core_id: i64) {
    if let Some(gid) = current_gid() {
        let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = NULL;
        }
    }
    park_self(Park::Chan {
        core: core_id,
        op: ChanOp::Recv,
    });
}

/// Park the current goroutine until `deadline_ms` (wall clock).
pub(crate) fn park_timer(deadline_ms: i64) {
    park_self(Park::Timer { at: deadline_ms });
}

/// Park the current goroutine on fd readiness.
pub(crate) fn park_fd(io: i64, read: bool) {
    park_self(Park::Fd { io, read });
}

/// Park the current goroutine until `target` completes.
pub(crate) fn park_join(target: i64) {
    park_self(Park::Join { target });
}

/// The result handle recorded on the current goroutine by its last receive.
pub(crate) fn recv_result() -> i64 {
    current_gid()
        .and_then(|gid| {
            core::GLOBAL
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .goroutines
                .get(&gid)
                .map(|gr| gr.recv_result)
        })
        .unwrap_or(NULL)
}

/// Block the calling *OS thread* until the goroutine completes, then return its
/// result handle. Used by the synchronous runtime bridge when a caller wants to
/// wait on a spawned goroutine.
pub(crate) fn join_blocking(gid: i64) -> i64 {
    loop {
        let state = core::GLOBAL
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .goroutines
            .get(&gid)
            .map(|gr| (gr.done, gr.result, gr.pending_exception));
        match state {
            Some((true, r, exception)) => {
                if exception != NULL {
                    crate::rearm_pending_exception(exception);
                }
                return r;
            }
            Some((false, _, _)) => {
                let (lock, cvar) = &*SIGNAL;
                let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                guard = cvar.wait(guard).unwrap_or_else(|p| p.into_inner());
            }
            None => return NULL,
        }
    }
}

/// Stop the worker pool and reactor, joining their threads. Idempotent, and
/// resets global state so it can be reused across tests in one binary.
pub(crate) fn shutdown() {
    if !SHUTDOWN.swap(true, Ordering::Relaxed) {
        notify_all();
    }
    let workers: Vec<_> = std::mem::take(&mut *WORKERS.lock().unwrap_or_else(|p| p.into_inner()));
    for w in workers {
        let _ = w.join();
    }
    let reactor = REACTOR.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(r) = reactor {
        let _ = r.join();
    }
    super::reactor::reset();
    // Stop and join the offload pool: flag the workers and wake them with a
    // sentinel job each so they exit their blocking `recv`.
    if OFFLOADING.load(Ordering::SeqCst) {
        OFFLOAD_STOP.store(true, Ordering::Relaxed);
        let count = OFFLOAD_THREADS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len();
        let send = OFFLOAD_SEND.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(tx) = send.as_ref() {
            for _ in 0..count {
                let _ = tx.send(Box::new(|| {}));
            }
        }
        drop(send);
        let offload: Vec<_> =
            std::mem::take(&mut *OFFLOAD_THREADS.lock().unwrap_or_else(|p| p.into_inner()));
        for handle in offload {
            let _ = handle.join();
        }
        OFFLOAD_SEND
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        OFFLOADING.store(false, Ordering::SeqCst);
        OFFLOAD_STOP.store(false, Ordering::Relaxed);
    }
    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    g.ready.clear();
    g.timers.clear();
    g.ops.clear();
    drop(g);
    QUEUES.lock().unwrap_or_else(|p| p.into_inner()).clear();
    SHUTDOWN.store(false, Ordering::Relaxed);
}

/// Records the reactor thread's join handle for [`shutdown`].
pub(crate) fn register_reactor_handle(handle: JoinHandle<()>) {
    *REACTOR.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle);
}

// Re-exported for the ABI layer.
pub(crate) use core::complete_op;
pub(crate) use core::drop_chan;
pub(crate) use core::drop_goroutine;
pub(crate) use core::drop_io;
pub(crate) use core::drop_op;
pub(crate) use core::io_ready;
pub(crate) use core::op_done;
pub(crate) use core::op_result;
pub(crate) use core::register_chan;
pub(crate) use core::register_io;
pub(crate) use core::register_op;

pub(crate) fn wake_workers_all() {
    notify_all();
}
