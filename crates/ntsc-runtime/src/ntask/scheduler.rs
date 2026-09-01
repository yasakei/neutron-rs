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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, RwLock, mpsc};
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
    static CURRENT_GID: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

/// Workers launched by [`start`]; kept so [`shutdown`] can join them.
static WORKERS: LazyLock<Mutex<Vec<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static REACTOR: LazyLock<Mutex<Option<JoinHandle<()>>>> = LazyLock::new(|| Mutex::new(None));
type WorkerQueue = Arc<Mutex<VecDeque<i64>>>;
/// Per-worker ready queues. Rebuilt only by [`start`] and [`shutdown`], so
/// spawns and pops take the read side and never clone the vector.
static QUEUES: LazyLock<RwLock<Vec<WorkerQueue>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// Whether the worker pool is running, so a spawn need not lock [`WORKERS`].
static STARTED: AtomicBool = AtomicBool::new(false);

/// Workers currently blocked in [`wait_for_work`]. With none parked, a wake is
/// pure overhead: a busy worker re-checks the queues when it finishes anyway.
static IDLE: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static WORKER_INDEX: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Start the worker pool. Idempotent. Call once (or let the first spawn
/// trigger it).
pub(crate) fn start() {
    if SHUTDOWN.load(Ordering::Relaxed) || STARTED.load(Ordering::Acquire) {
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
    *QUEUES.write().unwrap_or_else(|p| p.into_inner()) = queues;
    for index in 0..count {
        if let Ok(worker) = std::thread::Builder::new()
            .name(format!("ntask-worker-{index}"))
            .spawn(move || worker_loop(index))
        {
            workers.push(worker);
        }
    }
    STARTED.store(true, Ordering::Release);
}

/// Start the bounded offload pool. Idempotent. A small, fixed number of
/// standalone threads run blocking jobs (`process.exec*`, socket transfers)
/// so a scheduler worker is never blocked on I/O or a child process. Bound
/// the count so runaway children cannot exhaust threads.
fn start_offload_pool() {
    let mut guard = OFFLOAD_SEND.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_some() {
        // The sender is the single initialization marker: another thread has
        // already run the one-time setup (or is about to, under this lock).
        return;
    }
    OFFLOAD_STOP.store(false, Ordering::Relaxed);
    let count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(2, 8);
    let (tx, rx) = mpsc::channel::<OffloadJob>();
    // Publish the sender under the lock *before* spawning workers, so any
    // concurrent `run_offload` that locks `OFFLOAD_SEND` after this returns
    // is guaranteed to see it (no job can be dropped racing the startup).
    *guard = Some(tx);
    OFFLOADING.store(true, Ordering::SeqCst);
    drop(guard);
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
    // `start_offload_pool` publishes the sender under this same lock before
    // returning, so it is always `Some` here.
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

/// Register a goroutine running `poll` over `future`; returns its id without
/// scheduling it. `handle` is its registry wrapper id, reserved by the caller.
/// `cleanup` reclaims the future if the goroutine never completes.
pub(crate) fn register(
    poll: core::PollFn,
    future: i64,
    cleanup: Option<core::CleanupFn>,
    handle: i64,
) -> i64 {
    start();
    core::register_goroutine(new_goroutine(poll, future, cleanup, handle))
}

/// Register a goroutine and queue it as runnable. A spawn from outside the
/// worker pool takes the global lock once instead of twice.
pub(crate) fn spawn_runnable(
    poll: core::PollFn,
    future: i64,
    cleanup: Option<core::CleanupFn>,
    handle: i64,
) -> i64 {
    start();
    let goroutine = new_goroutine(poll, future, cleanup, handle);
    if WORKER_INDEX.with(|index| index.get()).is_some() {
        let gid = core::register_goroutine(goroutine);
        make_runnable(gid);
        return gid;
    }
    let gid = core::register_goroutine_runnable(goroutine);
    notify_one();
    gid
}

fn new_goroutine(
    poll: core::PollFn,
    future: i64,
    cleanup: Option<core::CleanupFn>,
    handle: i64,
) -> Goroutine {
    Goroutine {
        tasks: vec![(poll, future)],
        handle,
        cleanup: cleanup.map(|f| (f, future)),
        park: Park::None,
        pending_send: NULL,
        recv_result: NULL,
        recv_ok: false,
        done: false,
        result: NULL,
        pending_exception: NULL,
        joiners: Vec::new(),
    }
}

/// Push a goroutine onto the ready queue and wake a worker if one is asleep.
pub(crate) fn make_runnable(gid: i64) {
    let local = WORKER_INDEX.with(|index| index.get());
    if let Some(index) = local {
        let queues = QUEUES.read().unwrap_or_else(|p| p.into_inner());
        if let Some(queue) = queues.get(index) {
            queue
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push_back(gid);
            drop(queues);
            notify_one();
            return;
        }
    }
    core::GLOBAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ready
        .push_back(gid);
    notify_one();
}

/// Notify a single idle worker; a no-op when every worker is busy.
fn notify_one() {
    if IDLE.load(Ordering::Acquire) == 0 {
        return;
    }
    let (lock, cvar) = &*SIGNAL;
    let _g = lock.lock().unwrap_or_else(|p| p.into_inner());
    cvar.notify_one();
}

/// Notify all workers (used when a burst of goroutines became runnable).
pub(crate) fn notify_all() {
    if IDLE.load(Ordering::Acquire) == 0 {
        return;
    }
    notify_all_unconditional();
}

/// Notify all workers even when none is counted idle: a worker between its
/// queue check and its wait is not yet in `IDLE` but must see the stop flag.
fn notify_all_unconditional() {
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
            Some(gid) => {
                drive(gid);
            }
            None => wait_for_work(),
        }
    }
}

/// Goroutines a worker moves from the shared queue into its own per pop. A
/// spawn loop in `main` fills the shared queue far faster than workers drain
/// it, and taking one id per lock acquisition made the global mutex the
/// bottleneck: four workers plus the spawner contended on it 100k times.
const STEAL_BATCH: usize = 64;

/// Pop a runnable goroutine, or `None` when every queue is empty.
fn pop_ready(index: usize) -> Option<i64> {
    let queues = QUEUES.read().unwrap_or_else(|p| p.into_inner());
    if let Some(queue) = queues.get(index) {
        if let Some(gid) = queue.lock().unwrap_or_else(|p| p.into_inner()).pop_front() {
            return Some(gid);
        }
        // Local queue empty: refill it from the shared queue in one pass.
        let mut batch: Vec<i64> = {
            let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
            let take = g.ready.len().min(STEAL_BATCH);
            g.ready.drain(..take).collect()
        };
        if let Some(gid) = batch.pop() {
            if !batch.is_empty() {
                let mut local = queue.lock().unwrap_or_else(|p| p.into_inner());
                local.extend(batch);
            }
            return Some(gid);
        }
    } else {
        drop(queues);
        return core::GLOBAL
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .ready
            .pop_front();
    }
    queues.iter().enumerate().find_map(|(other, queue)| {
        (other != index)
            .then(|| queue.lock().unwrap_or_else(|p| p.into_inner()).pop_back())
            .flatten()
    })
}

/// Queue checks a worker makes before it parks. A spawn loop hands out work
/// faster than a syscall round trip, so a worker that sleeps the moment its
/// queue runs dry makes every later spawn pay a futex wake.
const SPIN_BEFORE_PARK: u32 = 64;

/// Sleep until a goroutine becomes runnable or we are asked to stop.
fn wait_for_work() {
    for _ in 0..SPIN_BEFORE_PARK {
        if SHUTDOWN.load(Ordering::Relaxed) || has_work() {
            return;
        }
        std::hint::spin_loop();
    }
    let (lock, cvar) = &*SIGNAL;
    IDLE.fetch_add(1, Ordering::Release);
    loop {
        if SHUTDOWN.load(Ordering::Relaxed) {
            break;
        }
        if has_work() {
            break;
        }
        let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        let _ = cvar
            .wait_timeout(guard, std::time::Duration::from_millis(10))
            .unwrap_or_else(|p| p.into_inner());
    }
    IDLE.fetch_sub(1, Ordering::Release);
}

fn has_work() -> bool {
    if QUEUES
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .any(|queue| !queue.lock().unwrap_or_else(|p| p.into_inner()).is_empty())
    {
        return true;
    }
    !core::GLOBAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ready
        .is_empty()
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
    CURRENT_GID.with(|cur| cur.set(Some(gid)));
    crate::install_async_tasks(tasks);
    crate::poll_async_tasks_once();
    let tasks = crate::take_async_tasks();
    let done = tasks.is_empty();
    CURRENT_GID.with(|cur| cur.set(None));

    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    if done {
        let (joiners, reclaim, handle) = {
            let Some(gr) = g.goroutines.get_mut(&gid) else {
                return;
            };
            gr.tasks = tasks;
            gr.done = true;
            // The trampoline already reclaimed the future on this path.
            gr.cleanup = None;
            // The poll ran on this worker, so an exception it raised (a throw
            // with no handler in an async body) is parked on this thread's
            // TLS. Capture it so the thread that joins the goroutine can
            // re-raise it; otherwise the uncaught exception would vanish with
            // the worker thread.
            let thrown = crate::take_pending_exception_message();
            if thrown != NULL {
                gr.pending_exception = thrown;
            }
            let joiners = std::mem::take(&mut gr.joiners);
            // A goroutine nobody waits on is reclaimed here: the registry
            // entry (and its `Handle::Goroutine` wrapper) exists only while a
            // joiner may still need its result or pending exception. An entry
            // with joiners or a captured throw stays until an explicit
            // join/drop consumes it.
            let reclaim = gr.pending_exception == NULL && joiners.is_empty();
            (joiners, reclaim, gr.handle)
        };
        for j in &joiners {
            g.ready.push_back(*j);
        }
        if reclaim {
            g.goroutines.remove(&gid);
        }
        drop(g);
        if reclaim {
            crate::registry::remove_goroutine_handle(handle);
        }
        // Reaches a woken joiner and any `join_blocking` caller; the main
        // thread waits on this for every program's root future.
        notify_all();
        return;
    }
    let park = {
        let Some(gr) = g.goroutines.get_mut(&gid) else {
            return;
        };
        gr.tasks = tasks;
        let park = gr.park;
        gr.park = Park::None;
        park
    };
    let mut woke = false;
    let mut requeue_local = false;
    match park {
        Park::None => {
            requeue_local = true;
        }
        Park::Chan { core, op } => woke = chan_op(&mut g, gid, core, op),
        Park::Timer { at } => {
            g.timers.entry(at).or_default().push(gid);
            core::timers_pending_offset(1);
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
                // `ready` stays set; consumed via `io_ready` when it re-polls.
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
    // A bare park made nothing runnable: whoever later completes it (offload
    // job, timer, channel partner) notifies then. Only a real handoff or
    // requeue needs a worker.
    if woke {
        notify_all();
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
            gr.recv_ok = true;
        }
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.pending_send = NULL;
        }
        g.ready.push_back(receiver);
        g.ready.push_back(gid);
        return true;
    }
    if cap > 0 && chan.buf.len() < cap {
        chan.buf.push_back(value);
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.pending_send = NULL;
        }
        g.ready.push_back(gid);
        return false;
    }
    chan.senders.push_back(gid);
    false
}

fn chan_recv(g: &mut core::Global, gid: i64, core_id: i64) -> bool {
    let Some(chan) = g.chans.get_mut(&core_id) else {
        // Channel gone: receive returns the zero value.
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = NULL;
            gr.recv_ok = false;
        }
        g.ready.push_back(gid);
        return false;
    };
    let mut woke = false;
    if let Some(v) = chan.buf.pop_front() {
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = v;
            gr.recv_ok = true;
        }
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
            gr.recv_ok = true;
        }
        g.ready.push_back(s);
        g.ready.push_back(gid);
        return true;
    }
    if chan.closed {
        if let Some(gr) = g.goroutines.get_mut(&gid) {
            gr.recv_result = NULL;
            gr.recv_ok = false;
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
    let (drained, senders, owns_elements, count) = {
        let Some(chan) = g.chans.get_mut(&core_id) else {
            eprintln!("[sched] chan_close: not found");
            return;
        };
        if chan.closed {
            return;
        }
        chan.closed = true;
        let receivers: Vec<i64> = chan.receivers.drain(..).collect();
        let senders: Vec<i64> = chan.senders.drain(..).collect();
        let count = receivers.len() + senders.len();
        // Parked receivers drain whatever is buffered; the rest stays in the
        // buffer for future receives (close forbids sends, it does not
        // discard queued values — they are released when the channel itself
        // drops).
        let drained: Vec<(i64, Option<i64>)> = receivers
            .into_iter()
            .map(|r| (r, chan.buf.pop_front()))
            .collect();
        (drained, senders, chan.owns_elements, count)
    };
    let mut abandoned = Vec::new();
    for (r, got) in drained {
        let has_gr = g.goroutines.contains_key(&r);
        match (got, has_gr, owns_elements) {
            (Some(value), true, _) => {
                if let Some(gr) = g.goroutines.get_mut(&r) {
                    gr.recv_result = value;
                    gr.recv_ok = true;
                }
            }
            (Some(value), false, true) => abandoned.push(value),
            (None, true, _) => {
                if let Some(gr) = g.goroutines.get_mut(&r) {
                    gr.recv_result = NULL;
                    gr.recv_ok = false;
                }
            }
            (None, false, _) => {}
            (Some(_), false, false) => {}
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
    CURRENT_GID.with(|cur| cur.get())
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
            gr.recv_ok = false;
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

/// Whether the last receive on this goroutine delivered a real value.
pub(crate) fn recv_ok() -> bool {
    current_gid()
        .and_then(|gid| {
            core::GLOBAL
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .goroutines
                .get(&gid)
                .map(|gr| gr.recv_ok)
        })
        .unwrap_or(false)
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
                // Counted as idle so the completing worker still wakes us,
                // instead of this thread sitting out the full quantum.
                IDLE.fetch_add(1, Ordering::Release);
                let (lock, cvar) = &*SIGNAL;
                let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                let _ = cvar
                    .wait_timeout(guard, std::time::Duration::from_millis(10))
                    .unwrap_or_else(|p| p.into_inner());
                IDLE.fetch_sub(1, Ordering::Release);
            }
            None => return NULL,
        }
    }
}

/// Stop the worker pool and reactor, joining their threads. Idempotent, and
/// resets global state so it can be reused across tests in one binary.
pub(crate) fn shutdown() {
    if !SHUTDOWN.swap(true, Ordering::Relaxed) {
        // Unconditional: a worker that is about to park has not yet been
        // counted in `IDLE`, and it must still observe the stop flag.
        notify_all_unconditional();
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
    // Reclaim goroutines that were never joined or driven, releasing their
    // handles now that the workers have stopped.
    struct Abandoned {
        handle: i64,
        exception: i64,
        result: i64,
        cleanup: Option<(core::CleanupFn, i64)>,
    }
    let mut g = core::GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let pending: Vec<Abandoned> = g
        .goroutines
        .values()
        .map(|gr| Abandoned {
            handle: gr.handle,
            exception: gr.pending_exception,
            result: if gr.done { gr.result } else { NULL },
            cleanup: gr.cleanup,
        })
        .collect();
    g.goroutines.clear();
    g.ready.clear();
    g.timers.clear();
    g.ops.clear();
    core::timers_reset();
    drop(g);
    for abandoned in pending {
        crate::registry::remove_goroutine_handle(abandoned.handle);
        if abandoned.exception != NULL {
            let _ = crate::registry::remove(abandoned.exception);
        }
        if abandoned.result != NULL {
            let _ = crate::registry::remove(abandoned.result);
        }
        // A goroutine that never completed still owns its future; its drop
        // reaches back into the scheduler, so the global lock is released.
        if let Some((cleanup, future)) = abandoned.cleanup {
            cleanup(future);
        }
    }
    QUEUES.write().unwrap_or_else(|p| p.into_inner()).clear();
    IDLE.store(0, Ordering::Release);
    STARTED.store(false, Ordering::Release);
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

/// Wake up to `count` idle workers.
pub(crate) fn wake_workers(count: usize) {
    if count == 0 || IDLE.load(Ordering::Acquire) == 0 {
        return;
    }
    let (lock, cvar) = &*SIGNAL;
    let _g = lock.lock().unwrap_or_else(|p| p.into_inner());
    for _ in 0..count {
        cvar.notify_one();
    }
}
