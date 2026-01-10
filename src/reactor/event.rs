use crate::api::Post;
use crate::types::{BreathingPhase, MediaHandle, NavDirection};
use eframe::egui;
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
			Event::Source(SourceEvent::KeyPress { .. }) => Priority::High,
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
	Search {
		query: String,
		page: u32,
	},
	Navigate(NavDirection),
	KeyPress {
		key: egui::Key,
		modifiers: egui::Modifiers,
	},
}

#[derive(Clone, Debug)]
pub enum GatewayEvent {
	/// Request a search
	SearchRequest {
		query: String,
		page: u32,
		limit: u32,
	},
	/// Search completed successfully
	SearchComplete { posts: Vec<Post>, page: u32 },
	/// Search failed
	SearchError { message: String },
	/// Request next page
	FetchNextPage,
}

#[derive(Clone, Debug)]
pub enum BrowserEvent {
	/// Posts received, update collection
	PostsReceived {
		posts: Vec<Post>,
		page: u32,
		is_new: bool,
	},
	/// Navigation triggered
	Navigate { direction: NavDirection },
	/// Current post changed (emitted after navigation)
	CurrentPostChanged {
		post: Box<Post>,
		index: usize,
		total: usize,
	},
	/// Near end of results, should prefetch
	NearEndOfResults { remaining: usize },
}

#[derive(Clone, Debug)]
pub enum MediaEvent {
	/// Load media for a post
	LoadRequest { url: String, is_video: bool },
	/// Media loaded successfully
	Ready { url: String, handle: MediaHandle },
	/// Media load failed
	LoadError { url: String, error: String },
	/// Prefetch hint
	Prefetch { urls: Vec<(String, bool)> },
}

#[derive(Clone, Debug)]
pub enum BreathingEvent {
	/// Toggle overlay visibility
	Toggle,
	/// Phase transition completed (self-scheduled)
	PhaseComplete,
	/// Phase changed (for ViewManager to render)
	PhaseChanged {
		phase: BreathingPhase,
		remaining: Duration,
	},
	/// Adjust idle multiplier
	SetIdleMultiplier { value: f32 },
}

#[derive(Clone, Debug)]
pub enum ViewEvent {
	/// Media is ready to display
	MediaReady { handle: MediaHandle },
	/// User manually panned
	UserPanned,
	/// Set pan speed
	SetPanSpeed { seconds: f32 },
}

#[derive(Clone, Debug)]
pub enum SettingsEvent {
	/// Toggle auto-play
	ToggleAutoPlay,
	/// Set auto-play delay
	SetDelay { duration: Duration },
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

	pub fn with_scheduled(mut self, event: Event, delay: Duration) -> Self {
		self.scheduled.push((event, delay));
		self
	}
}
