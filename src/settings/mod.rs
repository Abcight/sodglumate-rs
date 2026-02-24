use crate::breathing::BreathingOverlay;
use crate::reactor::{BreathingEvent, BrowserEvent, ComponentResponse, Event, SettingsEvent};
use crate::types::{BreathingPhase, NavDirection};
use std::time::{Duration, Instant};

pub struct SettingsManager {
	auto_play: bool,
	auto_play_delay: Duration,
	slideshow_scheduled: bool,
	cap_by_breathing: bool,
	last_advance_time: Instant,
}

impl SettingsManager {
	pub fn new(auto_play: bool, auto_play_delay: Duration, cap_by_breathing: bool) -> Self {
		Self {
			auto_play,
			auto_play_delay,
			slideshow_scheduled: false,
			cap_by_breathing,
			last_advance_time: Instant::now(),
		}
	}

	pub fn handle(&mut self, event: &Event, breathing: &BreathingOverlay) -> ComponentResponse {
		match event {
			Event::Settings(SettingsEvent::ToggleAutoPlay) => {
				self.auto_play = !self.auto_play;
				if self.auto_play {
					self.last_advance_time = Instant::now();
					if !self.slideshow_scheduled {
						self.slideshow_scheduled = true;
						return ComponentResponse::schedule(
							Event::Settings(SettingsEvent::SlideshowAdvance),
							self.auto_play_delay,
						);
					}
				}
				ComponentResponse::none()
			}
			Event::Settings(SettingsEvent::SetDelay { duration }) => {
				self.auto_play_delay = *duration;
				ComponentResponse::none()
			}
			Event::Settings(SettingsEvent::AdjustDelay { delta_secs }) => {
				let current_secs = self.auto_play_delay.as_secs() as i64;
				let new_secs = (current_secs + delta_secs).clamp(1, 60);
				self.auto_play_delay = Duration::from_secs(new_secs as u64);
				ComponentResponse::none()
			}
			Event::Settings(SettingsEvent::ToggleCapByBreathing) => {
				self.cap_by_breathing = !self.cap_by_breathing;
				ComponentResponse::none()
			}
			Event::Breathing(BreathingEvent::PhaseStarted(phase)) => {
				if self.auto_play && self.cap_by_breathing && breathing.is_visible() {
					if matches!(phase, BreathingPhase::Prepare | BreathingPhase::Release) {
						// Immediately trigger advance in these phases
						return ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
							direction: NavDirection::Next,
						}));
					}
				}
				ComponentResponse::none()
			}
			Event::Browser(BrowserEvent::Navigate { .. }) => {
				if self.auto_play {
					self.last_advance_time = Instant::now();
					if !self.slideshow_scheduled {
						self.slideshow_scheduled = true;
						return ComponentResponse::schedule(
							Event::Settings(SettingsEvent::SlideshowAdvance),
							self.auto_play_delay,
						);
					}
				}
				ComponentResponse::none()
			}
			Event::Settings(SettingsEvent::SlideshowAdvance) => {
				self.slideshow_scheduled = false;
				if self.auto_play {
					let elapsed = self.last_advance_time.elapsed();
					if elapsed < self.auto_play_delay {
						// We haven't waited long enough since the last manual navigation or advance
						self.slideshow_scheduled = true;
						return ComponentResponse::schedule(
							Event::Settings(SettingsEvent::SlideshowAdvance),
							self.auto_play_delay - elapsed,
						);
					}

					// Check breathing cap
					if self.cap_by_breathing && breathing.is_visible() {
						let phase = breathing.state().phase;
						if matches!(phase, BreathingPhase::Inhale | BreathingPhase::Hold) {
							// Blocked by breathing, reschedule to check again shortly
							self.slideshow_scheduled = true;
							return ComponentResponse::schedule(
								Event::Settings(SettingsEvent::SlideshowAdvance),
								Duration::from_secs(1),
							);
						}
					}

					// Navigate to next and schedule another advance
					self.slideshow_scheduled = true;
					self.last_advance_time = Instant::now();
					let mut response =
						ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
							direction: NavDirection::Next,
						}));
					response.scheduled.push((
						Event::Settings(SettingsEvent::SlideshowAdvance),
						self.auto_play_delay,
					));
					return response;
				}
				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	// Accessors for ViewManager/UI
	pub fn auto_play(&self) -> bool {
		self.auto_play
	}

	pub fn cap_by_breathing(&self) -> bool {
		self.cap_by_breathing
	}

	pub fn auto_play_delay(&self) -> Duration {
		self.auto_play_delay
	}
}

impl Default for SettingsManager {
	fn default() -> Self {
		Self::new(false, Duration::from_secs(16), false)
	}
}
