use crate::api::Post;
use crate::types::NavDirection;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Event {
	Source(SourceEvent),
	Gateway(GatewayEvent),
	Browser(BrowserEvent),
	Media(MediaEvent),
	Breathing(BreathingEvent),
	View(ViewEvent),
	Settings(SettingsEvent),
}

impl Event {
	pub fn priority(&self) -> Priority {
		match self {
			Event::Source(_) => Priority::High,
			Event::Gateway(GatewayEvent::SearchError { .. }) => Priority::Critical,
			Event::Gateway(_) => Priority::Normal,
			Event::Browser(_) => Priority::Normal,
			Event::Media(MediaEvent::Prefetch { .. }) => Priority::Low,
			Event::Media(_) => Priority::Normal,
			Event::Breathing(_) => Priority::Low,
			Event::View(_) => Priority::Normal,
			Event::Settings(SettingsEvent::SlideshowAdvance) => Priority::Normal,
			Event::Settings(_) => Priority::Normal,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
	Critical = 0,
	High = 1,
	Normal = 2,
	Low = 3,
}

impl Priority {
	pub fn as_index(&self) -> usize {
		*self as usize
	}
}

#[derive(Clone, Debug)]
pub enum SourceEvent {
	Search { query: String, page: u32 },
	Navigate(NavDirection),
}

#[derive(Clone, Debug)]
pub enum GatewayEvent {
	SearchRequest {
		query: String,
		page: u32,
		limit: u32,
	},
	SearchError {
		message: String,
	},
	FetchNextPage,
}

#[derive(Clone, Debug)]
pub enum BrowserEvent {
	PostsReceived {
		posts: Vec<Post>,
		page: u32,
		is_new: bool,
	},
	Navigate {
		direction: NavDirection,
	},
}

#[derive(Clone, Debug)]
pub enum MediaEvent {
	LoadRequest {
		sample_url: Option<String>,
		full_url: Option<String>,
		is_video: bool,
	},
	LoadError {
		error: String,
	},
	Prefetch {
		urls: Vec<(Option<String>, Option<String>, bool)>, // (sample_url, full_url, is_video)
	},
}

#[derive(Clone, Debug)]
pub enum BreathingEvent {
	Toggle,
	PhaseComplete,
	SetIdleMultiplier { value: f32 },
}

#[derive(Clone, Debug)]
pub enum ViewEvent {
	MediaReady,
}

#[derive(Clone, Debug)]
pub enum SettingsEvent {
	/// Toggle auto-play
	ToggleAutoPlay,
	/// Set auto-play delay
	SetDelay { duration: Duration },
	/// Adjust auto-play delay by delta
	AdjustDelay { delta_secs: i64 },
	/// Timer fired, advance slideshow
	SlideshowAdvance,
}

/// Response from component.handle()
#[derive(Default)]
pub struct ComponentResponse {
	/// Events to dispatch immediately
	pub events: Vec<Event>,
	/// Events to schedule (event, delay)
	pub scheduled: Vec<(Event, Duration)>,
}

impl ComponentResponse {
	pub fn none() -> Self {
		Self::default()
	}

	pub fn emit(event: Event) -> Self {
		Self {
			events: vec![event],
			scheduled: vec![],
		}
	}

	pub fn emit_many(events: Vec<Event>) -> Self {
		Self {
			events,
			scheduled: vec![],
		}
	}

	pub fn schedule(event: Event, delay: Duration) -> Self {
		Self {
			events: vec![],
			scheduled: vec![(event, delay)],
		}
	}
}
