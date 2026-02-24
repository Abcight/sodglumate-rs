use crate::reactor::{BreathingEvent, ComponentResponse, Event};
use crate::types::{BreathingPhase, BreathingStyle};
use rand::Rng;
use std::time::{Duration, Instant};

pub struct BreathingState {
	pub phase: BreathingPhase,
	pub start_time: Instant,
	pub duration: Duration,
}

pub struct BreathingOverlay {
	state: BreathingState,
	show_overlay: bool,
	idle_multiplier: f32,
	style: BreathingStyle,
}

impl BreathingOverlay {
	pub fn new(show_overlay: bool, idle_multiplier: f32, style: BreathingStyle) -> Self {
		Self {
			state: BreathingState {
				phase: BreathingPhase::Prepare,
				start_time: Instant::now(),
				duration: Duration::from_secs(5),
			},
			show_overlay,
			idle_multiplier,
			style,
		}
	}

	pub fn init(&self) -> ComponentResponse {
		let mut response = ComponentResponse::emit(Event::Breathing(BreathingEvent::PhaseStarted(
			self.state.phase,
		)));
		response.scheduled.push((
			Event::Breathing(BreathingEvent::PhaseComplete),
			self.state.duration,
		));
		response
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Breathing(BreathingEvent::Toggle) => {
				self.show_overlay = !self.show_overlay;
				ComponentResponse::none()
			}
			Event::Breathing(BreathingEvent::PhaseComplete) => {
				// Transition to next phase
				let (next_phase, duration) = self.transition_phase();
				self.state = BreathingState {
					phase: next_phase,
					start_time: Instant::now(),
					duration,
				};

				let mut response = ComponentResponse::emit(Event::Breathing(
					BreathingEvent::PhaseStarted(next_phase),
				));
				response
					.scheduled
					.push((Event::Breathing(BreathingEvent::PhaseComplete), duration));
				response
			}
			Event::Breathing(BreathingEvent::SetIdleMultiplier { value }) => {
				self.idle_multiplier = *value;
				ComponentResponse::none()
			}
			Event::Breathing(BreathingEvent::SetStyle { style }) => {
				self.style = *style;
				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	fn transition_phase(&self) -> (BreathingPhase, Duration) {
		let mut rng = rand::rng();

		match self.state.phase {
			BreathingPhase::Prepare => {
				// -> Inhale (5-10s)
				let duration_secs = rng.random_range(5..=10);
				(BreathingPhase::Inhale, Duration::from_secs(duration_secs))
			}
			BreathingPhase::Inhale => {
				// -> Hold (same as Inhale)
				(BreathingPhase::Hold, self.state.duration)
			}
			BreathingPhase::Hold => {
				// -> Release (4s)
				(BreathingPhase::Release, Duration::from_secs(4))
			}
			BreathingPhase::Release => {
				// 20% -> Inhale, 80% -> Prepare
				if rng.random_bool(0.2) {
					(BreathingPhase::Prepare, Duration::from_secs(3))
				} else {
					let duration_secs: u64 = rng.random_range(17..=28);
					let duration_secs = (duration_secs as f32 * self.idle_multiplier) as u64;
					(BreathingPhase::Idle, Duration::from_secs(duration_secs))
				}
			}
			BreathingPhase::Idle => {
				// -> Prepare (5s)
				(BreathingPhase::Prepare, Duration::from_secs(5))
			}
		}
	}

	// Accessors for ViewManager
	pub fn is_visible(&self) -> bool {
		self.show_overlay
	}

	pub fn state(&self) -> &BreathingState {
		&self.state
	}

	pub fn idle_multiplier(&self) -> f32 {
		self.idle_multiplier
	}

	pub fn style(&self) -> BreathingStyle {
		self.style
	}
}

impl Default for BreathingOverlay {
	fn default() -> Self {
		Self::new(false, 1.0, BreathingStyle::default())
	}
}
