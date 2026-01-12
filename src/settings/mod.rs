use crate::reactor::{BrowserEvent, ComponentResponse, Event, SettingsEvent};
use crate::types::NavDirection;
use std::time::Duration;

pub struct SettingsManager {
	auto_play: bool,
	auto_play_delay: Duration,
	slideshow_scheduled: bool,
}

impl SettingsManager {
	pub fn new() -> Self {
		Self {
			auto_play: false,
			auto_play_delay: Duration::from_secs(16),
			slideshow_scheduled: false,
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Settings(SettingsEvent::ToggleAutoPlay) => {
				self.auto_play = !self.auto_play;
				if self.auto_play && !self.slideshow_scheduled {
					self.slideshow_scheduled = true;
					return ComponentResponse::schedule(
						Event::Settings(SettingsEvent::SlideshowAdvance),
						self.auto_play_delay,
					);
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
			Event::Settings(SettingsEvent::SlideshowAdvance) => {
				self.slideshow_scheduled = false;
				if self.auto_play {
					// Navigate to next and schedule another advance
					self.slideshow_scheduled = true;
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

	pub fn auto_play_delay(&self) -> Duration {
		self.auto_play_delay
	}
}

impl Default for SettingsManager {
	fn default() -> Self {
		Self::new()
	}
}
