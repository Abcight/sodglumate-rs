use super::event::Event;
use std::collections::VecDeque;

/// Priority event queue with 4 priority levels
pub struct EventQueue {
	queues: [VecDeque<Event>; 4],
}

impl EventQueue {
	pub fn new() -> Self {
		Self {
			queues: [
				VecDeque::new(), // Critical
				VecDeque::new(), // High
				VecDeque::new(), // Normal
				VecDeque::new(), // Low
			],
		}
	}

	/// Push an event to the appropriate priority queue
	pub fn push(&mut self, event: Event) {
		let priority = event.priority();
		self.queues[priority.as_index()].push_back(event);
	}

	/// Pop the highest priority event available
	pub fn pop(&mut self) -> Option<Event> {
		for queue in &mut self.queues {
			if let Some(event) = queue.pop_front() {
				return Some(event);
			}
		}
		None
	}
}

impl Default for EventQueue {
	fn default() -> Self {
		Self::new()
	}
}
