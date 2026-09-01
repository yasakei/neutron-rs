//! Core state types of the ntroutine substrate: the global scheduling and
//! parking registry, channels, goroutines, and reactor interests.
//!
//! All blocking coordination (which goroutine is parked on which channel, the
//! channel buffers, timers, and file-descriptor interests) lives under a single
//! [`Mutex`] guarded [`Global`] state, so a block/unblock decision and the
//! registration of a waiter are one atomic critical section — there is no lost
//! wakeup. Runnable goroutines are handed to per-worker local queues (see
//! [`crate::ntask::scheduler`]), so CPU-bound work spreads across the OS-thread
//! pool while I/O-bound goroutines park without tying up a thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::idmap::IdMap;
use crate::registry::{self, NULL};

/// A poll function for one async future. Returns `1` when the future
/// completed, `0` when it is still pending. See [`crate::AsyncPollFn`].
pub(crate) type PollFn = crate::AsyncPollFn;

/// Reclaims one abandoned future. See [`crate::AsyncCleanupFn`].
pub(crate) type CleanupFn = crate::AsyncCleanupFn;

/// A per-goroutine wait target set by generated code before a poll returns
/// `0`. The worker's driver reads it after the poll to decide whether to
/// requeue, park on a channel, park on a timer, or park on a descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Park {
    /// No explicit wait: a cooperative yield. The goroutine is requeued.
    None,
    /// Wait on a channel core, performing the given operation.
    Chan { core: i64, op: ChanOp },
    /// Wait until the given wall-clock deadline.
    Timer { at: i64 },
    /// Wait for readiness on a descriptor, for the given operation.
    Fd { io: i64, read: bool },
    /// Wait until a sibling goroutine completes.
    Join { target: i64 },
    /// Wait until an external worker thread completes an offloaded blocking
    /// job (see [`AsyncOp`]). The result handle is stored on the goroutine.
    Job { core: i64 },
}

/// Direction of a channel operation a parked goroutine wants to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChanOp {
    Send,
    Recv,
}

/// A goroutine: one stackless future scheduled onto the worker pool. It does
/// not rely on any worker thread-local async stack, so a torn-out goroutine is
/// migratable between workers.
#[derive(Debug)]
pub(crate) struct Goroutine {
    /// Nested async task stack. It moves with the goroutine between workers.
    pub(crate) tasks: Vec<(PollFn, i64)>,
    /// Registry id of this goroutine's `Handle::Goroutine` wrapper, so it is
    /// reclaimed by one removal when the goroutine finishes.
    pub(crate) handle: i64,
    /// Reclaims the root future if this goroutine never completes, so an
    /// abandoned future's owned handles are released at shutdown. `None` for a
    /// goroutine whose future is not heap-allocated by generated code.
    pub(crate) cleanup: Option<(CleanupFn, i64)>,
    /// The pending wait target, set by the last park call.
    pub(crate) park: Park,
    /// The value a blocked sender is handing off, or `NULL`.
    pub(crate) pending_send: i64,
    /// The value a receiver picked up (or the zero value on close), or `NULL`.
    pub(crate) recv_result: i64,
    /// Whether `recv_result` carries a real value (`false` after a close-drain
    /// or a failed receive). `for v in chan` needs the distinction because a
    /// raw scalar `0` is a legal received value.
    pub(crate) recv_ok: bool,
    /// `1` once the future has completed (its result is in `result`).
    pub(crate) done: bool,
    /// The result handle of a completed goroutine.
    pub(crate) result: i64,
    /// Message of an uncaught exception that ended this goroutine's future,
    /// or `NULL`. Ownership transfers to the thread that joins it, which
    /// re-raises it so the caller observes the pending exception.
    pub(crate) pending_exception: i64,
    /// Sibling goroutines parked waiting for this one to complete.
    pub(crate) joiners: Vec<i64>,
}

/// A channel: an optional bounded ring buffer of `i64` slots plus the parked
/// senders and receivers. Slots store raw scalar values or owned handles
/// (for heap element types); ownership of a handle slot transfers to the
/// receiver when it is received out, and back to the runtime when the channel
/// is dropped.
#[derive(Debug)]
pub(crate) struct Chan {
    pub(crate) buf: VecDeque<i64>,
    pub(crate) cap: usize,
    pub(crate) owns_elements: bool,
    pub(crate) closed: bool,
    pub(crate) senders: VecDeque<i64>,
    pub(crate) receivers: VecDeque<i64>,
}

impl Chan {
    pub(crate) fn new(cap: usize, owns_elements: bool) -> Self {
        Chan {
            buf: VecDeque::with_capacity(cap),
            cap,
            owns_elements,
            closed: false,
            senders: VecDeque::new(),
            receivers: VecDeque::new(),
        }
    }
}

/// A reactor registration: a timer or a file-descriptor readiness interest
/// that can be polled from a goroutine.
#[derive(Debug)]
pub(crate) struct AsyncIo {
    /// `0` = timer, non-zero = the raw file descriptor interest.
    pub(crate) fd: i64,
    /// Poll the readiness without blocking.
    pub(crate) ready: bool,
    /// Whether a goroutine is currently parked on this interest.
    pub(crate) parked: bool,
    /// Goroutines parked waiting for this descriptor to become ready.
    pub(crate) waiters: Vec<i64>,
}

/// An offloaded blocking job. A goroutine parks on it while a worker-thread
/// (the bounded offload pool) runs the blocking work; that thread completes
/// the op, waking the parked goroutine. This is how a scheduler thread stays
/// free while a child process runs or a blocking socket transfer happens.
#[derive(Debug)]
pub(crate) struct AsyncOp {
    /// The goroutine parked waiting for completion, if any.
    pub(crate) waiter: Option<i64>,
    /// The result handle once the job finished, or `NULL`.
    pub(crate) result: i64,
    /// Whether the worker finished the job.
    pub(crate) done: bool,
}

/// The global scheduling + parking state, guarded by one mutex.
pub(crate) struct Global {
    pub(crate) next_core: i64,
    /// Ready goroutine ids, shared queue consumed by workers. A global queue
    /// keeps the block/unblock path deadlock-free and still lets many workers
    /// run CPU-bound goroutines in parallel.
    pub(crate) ready: VecDeque<i64>,
    pub(crate) goroutines: IdMap<Goroutine>,
    pub(crate) chans: IdMap<Chan>,
    pub(crate) ios: IdMap<AsyncIo>,
    /// Offloaded blocking jobs not yet completed (or awaiting a reader).
    pub(crate) ops: IdMap<AsyncOp>,
    /// wall-clock ms deadline -> woken goroutine ids.
    pub(crate) timers: std::collections::BTreeMap<i64, Vec<i64>>,
}

pub(crate) static GLOBAL: LazyLock<Mutex<Global>> = LazyLock::new(|| {
    Mutex::new(Global {
        next_core: 1,
        ready: VecDeque::new(),
        goroutines: IdMap::default(),
        chans: IdMap::default(),
        ios: IdMap::default(),
        ops: IdMap::default(),
        timers: std::collections::BTreeMap::new(),
    })
});

/// Number of goroutines currently parked on timers.
static TIMER_PENDING: AtomicI64 = AtomicI64::new(0);

/// Adjust the pending-timer count by `delta` (negative to decrement). The
/// global lock must be held whenever a timer is parked or fired.
pub(crate) fn timers_pending_offset(delta: i64) {
    let _ = TIMER_PENDING.fetch_add(delta, Ordering::Relaxed);
}

/// Whether any goroutine is parked on a timer. The reactor fast path uses this
/// to skip the global lock when there is nothing to scan.
pub(crate) fn has_pending_timers() -> bool {
    TIMER_PENDING.load(Ordering::Relaxed) > 0
}

/// Reset the pending-timer count.
pub(crate) fn timers_reset() {
    TIMER_PENDING.store(0, Ordering::Relaxed);
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Register a new goroutine and return its core id. The goroutine is not
/// scheduled until [`crate::ntask::scheduler::make_runnable`] is called for it.
pub(crate) fn register_goroutine(g: Goroutine) -> i64 {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let id = guard.next_core;
    guard.next_core = guard.next_core.saturating_add(1);
    guard.goroutines.insert(id, g);
    id
}

/// Register a new channel and return its core id.
pub(crate) fn register_chan(cap: usize, owns_elements: bool) -> i64 {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let id = guard.next_core;
    guard.next_core = guard.next_core.saturating_add(1);
    guard.chans.insert(id, Chan::new(cap, owns_elements));
    id
}

/// Register a new reactor interest and return its core id.
pub(crate) fn register_io() -> i64 {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let id = guard.next_core;
    guard.next_core = guard.next_core.saturating_add(1);
    guard.ios.insert(
        id,
        AsyncIo {
            fd: 0,
            ready: false,
            parked: false,
            waiters: Vec::new(),
        },
    );
    id
}

/// Drop a goroutine core and clean up any global wait queues it was
/// parked on (timers, channels, io, offloaded jobs). Otherwise a
/// `go` that was parked on `async.sleep` and then dropped (e.g. its
/// `Handle::Goroutine` was reaped after `main` returned) would keep its
/// deadline in `timers` and `TIMER_PENDING` would stay >0, keeping the
/// reactor spinning. The same applies to `chan`/`io`/`job` waiters.
pub(crate) fn drop_goroutine(core: i64) {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    guard.goroutines.remove(&core);
    // Timers: remove this gid from any deadline bucket.
    let mut removed_timers = 0i64;
    guard.timers.retain(|_, gids| {
        let before = gids.len();
        gids.retain(|&gid| gid != core);
        removed_timers += (before - gids.len()) as i64;
        !gids.is_empty()
    });
    if removed_timers > 0 {
        timers_pending_offset(-removed_timers);
    }
    // Channels: remove from any senders/receivers queue.
    for chan in guard.chans.values_mut() {
        chan.senders.retain(|&gid| gid != core);
        chan.receivers.retain(|&gid| gid != core);
    }
    // Reactor io: remove from any waiters list.
    for io in guard.ios.values_mut() {
        io.waiters.retain(|&gid| gid != core);
        if io.waiters.is_empty() {
            io.parked = false;
        }
    }
    // Offloaded jobs: clear waiter if it was this gid.
    for op in guard.ops.values_mut() {
        if op.waiter == Some(core) {
            op.waiter = None;
        }
    }
}

/// Drop a channel core, reclaiming any buffered element handles.
pub(crate) fn drop_chan(core: i64) {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(chan) = guard.chans.remove(&core) {
        for slot in chan.buf {
            if chan.owns_elements {
                let _ = registry::remove(slot);
            }
        }
    }
}

/// Drop a reactor-interest core.
pub(crate) fn drop_io(core: i64) {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    guard.ios.remove(&core);
}

pub(crate) fn io_ready(core: i64) -> bool {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let Some(io) = guard.ios.get_mut(&core) else {
        return false;
    };
    std::mem::take(&mut io.ready)
}

/// Reserve a fresh offloaded-job id. The caller enqueues the blocking work
/// on the offload pool and parks the calling goroutine on it. See
/// [`crate::ntask::scheduler::offload_start`] and
/// [`crate::ntask::scheduler::complete_job`].
pub(crate) fn register_op() -> i64 {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let id = guard.next_core;
    guard.next_core = guard.next_core.saturating_add(1);
    guard.ops.insert(
        id,
        AsyncOp {
            waiter: None,
            result: NULL,
            done: false,
        },
    );
    id
}

/// Mark an offloaded job done with `result`. Callable from any thread
/// (typically a worker on the offload pool); wakes the parked goroutine, if
/// any.
pub(crate) fn complete_op(core: i64, result: i64) {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let Some(op) = guard.ops.get_mut(&core) else {
        return;
    };
    op.result = result;
    op.done = true;
    let waiter = op.waiter.take();
    if let Some(gid) = waiter {
        guard.ready.push_back(gid);
    }
    drop(guard);
    if waiter.is_some() {
        crate::ntask::scheduler::notify_all();
    }
}

/// Take the result handle of a completed offloaded job, removing it from the
/// table. Ownership of the handle transfers to the caller. Returns `NULL`
/// for an unknown or not-yet-completed job.
pub(crate) fn op_result(core: i64) -> i64 {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    let Some(op) = guard.ops.remove(&core) else {
        return NULL;
    };
    op.result
}

/// `1` when the offloaded job is already done, `0` otherwise (or unknown).
pub(crate) fn op_done(core: i64) -> bool {
    GLOBAL
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .ops
        .get(&core)
        .map(|op| op.done)
        .unwrap_or(false)
}

/// Drop an offloaded job that will never be reaped (e.g. its future was
/// dropped mid-flight), releasing the result handle if one arrived.
pub(crate) fn drop_op(core: i64) {
    let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(op) = guard.ops.remove(&core)
        && op.done
        && op.result != NULL
    {
        let _ = registry::remove(op.result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A goroutine dropped while parked on a timer must leave no deadline
    /// behind: a stale entry would keep `TIMER_PENDING` above zero and the
    /// reactor scanning forever.
    #[test]
    fn dropping_a_timer_parked_goroutine_clears_its_deadline() {
        extern "C" fn never_done(_: i64) -> i8 {
            0
        }
        let core = register_goroutine(Goroutine {
            tasks: vec![(never_done as PollFn, NULL)],
            handle: NULL,
            cleanup: None,
            park: Park::None,
            pending_send: NULL,
            recv_result: NULL,
            recv_ok: false,
            done: false,
            result: NULL,
            pending_exception: NULL,
            joiners: Vec::new(),
        });
        let deadline = now_ms() + 10_000;
        {
            let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
            guard.timers.entry(deadline).or_default().push(core);
        }
        timers_pending_offset(1);
        assert!(has_pending_timers());

        drop_goroutine(core);

        let guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
        assert!(!guard.timers.contains_key(&deadline));
        assert!(!guard.goroutines.contains_key(&core));
        drop(guard);
        assert!(!has_pending_timers());
    }

    /// A goroutine dropped while parked on a channel must be removed from the
    /// channel's wait queues, or a later handoff would target a dead id.
    #[test]
    fn dropping_a_chan_parked_goroutine_clears_its_wait_queues() {
        extern "C" fn never_done(_: i64) -> i8 {
            0
        }
        let chan_core = register_chan(0, false);
        let core = register_goroutine(Goroutine {
            tasks: vec![(never_done as PollFn, NULL)],
            handle: NULL,
            cleanup: None,
            park: Park::Chan {
                core: chan_core,
                op: ChanOp::Send,
            },
            pending_send: NULL,
            recv_result: NULL,
            recv_ok: false,
            done: false,
            result: NULL,
            pending_exception: NULL,
            joiners: Vec::new(),
        });
        {
            let mut guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
            let chan = guard.chans.get_mut(&chan_core).expect("channel");
            chan.senders.push_back(core);
        }

        drop_goroutine(core);

        let guard = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
        let chan = guard.chans.get(&chan_core).expect("channel");
        assert!(chan.senders.is_empty());
        assert!(chan.receivers.is_empty());
        drop(guard);
        drop_chan(chan_core);
    }
}
