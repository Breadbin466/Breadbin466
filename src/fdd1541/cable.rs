// =======================================================
// src/fdd1541/cable.rs — Virtual IEC cable and lock-free event ring
// =======================================================

use super::constants::{DRIVE_RING_CAPACITY, DRIVE_RING_MASK};
use std::hint::spin_loop;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/* The producer and consumer counters occupy separate cache lines so the host and drive threads do not invalidate each other while exchanging IEC events. */
#[repr(align(128))]
pub(super) struct PaddedU64(pub(super) AtomicU64);

/* EventRing is a single-producer, single-consumer queue. The producer publishes a fully written slot by advancing head with Release ordering; the consumer observes that publication through an Acquire load before reading the slot. */
pub(super) struct EventRing {
	cycles: Box<[AtomicU64]>,
	states: Box<[AtomicU32]>,
	head: PaddedU64,
	tail: PaddedU64,
}

impl EventRing {
	pub(super) fn new() -> Self {
		let cycles = (0..DRIVE_RING_CAPACITY)
			.map(|_| AtomicU64::new(0))
			.collect::<Vec<_>>()
			.into_boxed_slice();
		let states = (0..DRIVE_RING_CAPACITY)
			.map(|_| AtomicU32::new(0))
			.collect::<Vec<_>>()
			.into_boxed_slice();
		Self {
			cycles,
			states,
			head: PaddedU64(AtomicU64::new(0)),
			tail: PaddedU64(AtomicU64::new(0)),
		}
	}

	/* A full ring applies back-pressure instead of dropping an electrical transition. Shutdown alone may interrupt that wait because no later emulated cycle will consume the pending transition. */
	#[inline(always)]
	pub(super) fn push(&self, cycle: u64, state: u32, stopping: &AtomicBool) -> bool {
		let head = self.head.0.load(Ordering::Relaxed);
		while head.wrapping_sub(self.tail.0.load(Ordering::Acquire)) >= DRIVE_RING_CAPACITY as u64 {
			if stopping.load(Ordering::Acquire) {
				return false;
			}
			spin_loop();
		}
		let slot = (head & DRIVE_RING_MASK) as usize;
		self.cycles[slot].store(cycle, Ordering::Relaxed);
		self.states[slot].store(state, Ordering::Relaxed);
		self.head.0.store(head.wrapping_add(1), Ordering::Release);
		true
	}

	#[inline(always)]
	pub(super) fn is_empty(&self) -> bool {
		self.head.0.load(Ordering::Acquire) == self.tail.0.load(Ordering::Relaxed)
	}

	/* Events are consumed in emulated-cycle order. A future event remains queued so the host never observes a device transition before its scheduled boundary. */
	#[inline(always)]
	pub(super) fn drain_up_to(&self, target: u64, state: &mut u32) -> bool {
		let head = self.head.0.load(Ordering::Acquire);
		let mut tail = self.tail.0.load(Ordering::Relaxed);
		let mut advanced = false;
		while tail != head {
			let slot = (tail & DRIVE_RING_MASK) as usize;
			let cycle = self.cycles[slot].load(Ordering::Relaxed);
			if cycle > target {
				break;
			}
			*state = self.states[slot].load(Ordering::Relaxed);
			tail = tail.wrapping_add(1);
			advanced = true;
		}
		if advanced {
			self.tail.0.store(tail, Ordering::Release);
		}
		advanced
	}

	pub(super) fn clear(&self) {
		let head = self.head.0.load(Ordering::Acquire);
		self.tail.0.store(head, Ordering::Release);
	}
}

/* IecCable contains only shared clocks and packed line-state transitions. No CPU, VIA or disk state crosses this boundary. */
pub(super) struct IecCable {
	pub(super) host_cycle: PaddedU64,
	pub(super) drive_cycle: PaddedU64,
	pub(super) device_events: EventRing,
	pub(super) stopping: AtomicBool,
	pub(super) waiting: AtomicBool,
}

impl IecCable {
	pub(super) fn new() -> Self {
		Self {
			host_cycle: PaddedU64(AtomicU64::new(0)),
			drive_cycle: PaddedU64(AtomicU64::new(0)),
			device_events: EventRing::new(),
			stopping: AtomicBool::new(false),
			waiting: AtomicBool::new(false),
		}
	}
}