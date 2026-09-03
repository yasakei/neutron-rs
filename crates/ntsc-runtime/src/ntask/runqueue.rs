//! Lock-free per-worker run queue: a fixed-size ring of goroutine ids with an
//! atomic head and tail, plus a single-slot LIFO cell.
//!
//! Follows the Go runtime's `runq` (`runtime/proc.go`) and Tokio's
//! `multi_thread::queue`: the owner is the only producer, so it publishes with a
//! release store and never needs a read-modify-write to push. Thieves and the
//! owner both consume from the head via compare-exchange. Because the payload is
//! a plain `i64`, no ownership dance around the slots is needed — a slot is only
//! read after the CAS that claims its index succeeds.
//!
//! `next` is Go's `runnext` / Tokio's LIFO slot: the most recently made-runnable
//! goroutine is handed straight back to the same worker, which keeps a channel
//! ping-pong on one core and its state in cache.

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

use crate::registry::NULL;

/// Ring capacity. A power of two so the index wrap is a mask, matching Go's
/// 256-entry `runq`.
const CAPACITY: usize = 256;
const MASK: u32 = CAPACITY as u32 - 1;

pub(crate) struct RunQueue {
    /// Consumed up to here. Advanced by the owner and by thieves, so every
    /// advance is a compare-exchange.
    head: AtomicU32,
    /// Produced up to here. Written only by the owner, with a release store.
    tail: AtomicU32,
    ring: Box<[AtomicI64]>,
    /// Single-slot LIFO cell, `NULL` when empty.
    next: AtomicI64,
}

impl RunQueue {
    pub(crate) fn new() -> Self {
        RunQueue {
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            ring: (0..CAPACITY).map(|_| AtomicI64::new(NULL)).collect(),
            next: AtomicI64::new(NULL),
        }
    }

    /// Ids waiting in the ring. Wrapping subtraction, so a torn read of the two
    /// indices can only under- or over-count, never panic.
    fn ring_len(&self) -> u32 {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head).min(CAPACITY as u32)
    }

    /// Whether this queue holds nothing. Two relaxed-ish atomic loads, so the
    /// pre-park spin can call it without touching a lock.
    pub(crate) fn is_empty(&self) -> bool {
        self.next.load(Ordering::Acquire) == NULL && self.ring_len() == 0
    }

    /// Runnable ids held here, including the LIFO slot.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.ring_len() as usize + usize::from(self.next.load(Ordering::Acquire) != NULL)
    }

    /// Push into the LIFO slot, returning the id it displaced (which the caller
    /// pushes onto the ring). Owner only.
    pub(crate) fn push_next(&self, gid: i64) -> Option<i64> {
        let previous = self.next.swap(gid, Ordering::AcqRel);
        (previous != NULL).then_some(previous)
    }

    /// Push onto the ring's tail. Returns `false` when it is full, so the caller
    /// can overflow to the shared queue. Owner only: `tail` has no other writer,
    /// so the release store is enough to publish the slot.
    pub(crate) fn push(&self, gid: i64) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= CAPACITY as u32 {
            return false;
        }
        self.ring[(tail & MASK) as usize].store(gid, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Take the next id for this worker: the LIFO slot first, then the ring's
    /// head. Owner only, though the head CAS still races thieves.
    pub(crate) fn pop(&self) -> Option<i64> {
        let next = self.next.load(Ordering::Acquire);
        if next != NULL
            && self
                .next
                .compare_exchange(next, NULL, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            return Some(next);
        }
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            if head == tail {
                return None;
            }
            let gid = self.ring[(head & MASK) as usize].load(Ordering::Relaxed);
            if self
                .head
                .compare_exchange(
                    head,
                    head.wrapping_add(1),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return Some(gid);
            }
        }
    }

    /// Move half of this queue's ids into `dst`, returning one of them to run
    /// immediately. The payload is copied before the head CAS commits the claim;
    /// a lost CAS discards the copy and retries, as in Go's `runqgrab`.
    pub(crate) fn steal_into(&self, dst: &RunQueue) -> Option<i64> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            let available = tail.wrapping_sub(head);
            if available == 0 || available > CAPACITY as u32 {
                // Empty, or a torn snapshot of the two indices; retry the read.
                if available == 0 {
                    return None;
                }
                continue;
            }
            let take = available - available / 2;
            let mut batch = [NULL; CAPACITY / 2 + 1];
            for (offset, slot) in batch.iter_mut().enumerate().take(take as usize) {
                *slot = self.ring[(head.wrapping_add(offset as u32) & MASK) as usize]
                    .load(Ordering::Relaxed);
            }
            if self
                .head
                .compare_exchange(
                    head,
                    head.wrapping_add(take),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_err()
            {
                continue;
            }
            // The last one is returned to the thief; the rest go on its ring.
            for &gid in batch.iter().take(take as usize - 1) {
                if !dst.push(gid) {
                    // Cannot happen (the thief's ring was empty), but never drop
                    // a goroutine: fall back to the shared queue.
                    super::core::push_ready(gid);
                }
            }
            return Some(batch[take as usize - 1]);
        }
    }

    /// Claim the older half of the ring for the shared queue, so a full ring
    /// costs one shared-lock acquisition per half-ring rather than per push
    /// (Go's `runqputslow`). Owner only.
    pub(crate) fn take_half(&self) -> Vec<i64> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            let available = tail.wrapping_sub(head);
            if available == 0 || available > CAPACITY as u32 {
                return Vec::new();
            }
            let take = available - available / 2;
            let batch: Vec<i64> = (0..take)
                .map(|offset| {
                    self.ring[(head.wrapping_add(offset) & MASK) as usize].load(Ordering::Relaxed)
                })
                .collect();
            if self
                .head
                .compare_exchange(
                    head,
                    head.wrapping_add(take),
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return batch;
            }
        }
    }

    /// Drain everything for shutdown.
    pub(crate) fn drain(&self) -> Vec<i64> {
        let mut ids = Vec::new();
        let next = self.next.swap(NULL, Ordering::AcqRel);
        if next != NULL {
            ids.push(next);
        }
        while let Some(gid) = self.pop() {
            ids.push(gid);
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_pop_preserve_fifo_order_after_the_lifo_slot() {
        let queue = RunQueue::new();
        assert!(queue.is_empty());
        for gid in 1..=4 {
            assert!(queue.push(gid));
        }
        assert_eq!(queue.len(), 4);
        assert!(queue.push_next(99).is_none());
        // The LIFO slot runs first, then the ring in order.
        assert_eq!(queue.pop(), Some(99));
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), Some(4));
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_full_ring_refuses_a_push_so_the_caller_can_overflow() {
        let queue = RunQueue::new();
        for gid in 0..CAPACITY as i64 {
            assert!(queue.push(gid + 1));
        }
        assert!(!queue.push(9999));
        assert_eq!(queue.len(), CAPACITY);
        assert_eq!(queue.pop(), Some(1));
        assert!(queue.push(9999));
    }

    #[test]
    fn push_next_returns_the_displaced_id() {
        let queue = RunQueue::new();
        assert!(queue.push_next(7).is_none());
        assert_eq!(queue.push_next(8), Some(7));
        assert_eq!(queue.pop(), Some(8));
    }

    #[test]
    fn stealing_takes_half_and_leaves_the_rest() {
        let victim = RunQueue::new();
        let thief = RunQueue::new();
        for gid in 1..=8 {
            assert!(victim.push(gid));
        }

        let ran = victim.steal_into(&thief).expect("steal");

        // Half of 8 is 4: one is returned to run, three land on the thief.
        assert_eq!(victim.len(), 4);
        assert_eq!(thief.len(), 3);
        assert_eq!(ran, 4);
        assert_eq!(thief.pop(), Some(1));
        assert_eq!(victim.pop(), Some(5));
    }

    #[test]
    fn stealing_a_single_id_leaves_the_victim_empty() {
        let victim = RunQueue::new();
        let thief = RunQueue::new();
        assert!(victim.push(42));

        assert_eq!(victim.steal_into(&thief), Some(42));

        assert!(victim.is_empty());
        assert!(thief.is_empty());
        assert_eq!(victim.steal_into(&thief), None);
    }

    #[test]
    fn indices_survive_wrapping_past_the_ring_capacity() {
        let queue = RunQueue::new();
        for round in 0..(CAPACITY * 4) as i64 {
            assert!(queue.push(round + 1));
            assert_eq!(queue.pop(), Some(round + 1));
        }
        assert!(queue.is_empty());
    }
}
