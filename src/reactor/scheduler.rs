use super::event::Event;
use super::queue::EventQueue;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

struct ScheduledEvent {
	emit_at: Instant,
	event: Event,
}

impl PartialEq for ScheduledEvent {
	fn eq(&self, other: &Self) -> bool {
		self.emit_at == other.emit_at
	}
}

impl Eq for ScheduledEvent {}

impl PartialOrd for ScheduledEvent {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for ScheduledEvent {
	fn cmp(&self, other: &Self) -> Ordering {
		other.emit_at.cmp(&self.emit_at)
	}
}

pub struct Scheduler {
	pending: BinaryHeap<ScheduledEvent>,
}

impl Scheduler {
	pub fn new() -> Self {
		Self {
			pending: BinaryHeap::new(),
		}
	}

	/// Schedule an event to fire after `delay`
	pub fn schedule(&mut self, event: Event, delay: Duration) {
		self.pending.push(ScheduledEvent {
			emit_at: Instant::now() + delay,
			event,
		});
	}

	/// Poll and drain ready events into the queue
	pub fn tick(&mut self, queue: &mut EventQueue) {
		let now = Instant::now();
		while let Some(scheduled) = self.pending.peek() {
			if scheduled.emit_at <= now {
				let scheduled = self.pending.pop().unwrap();
				queue.push(scheduled.event);
			} else {
				break;
			}
		}
	}
}

impl Default for Scheduler {
	fn default() -> Self {
		Self::new()
	}
}
