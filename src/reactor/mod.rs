pub mod event;
pub mod queue;
pub mod scheduler;

pub use event::{
	BreathingEvent, BrowserEvent, ComponentResponse, Event, GatewayEvent, MediaEvent,
	SettingsEvent, SourceEvent, ViewEvent,
};
pub use queue::EventQueue;
pub use scheduler::Scheduler;

use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::gateway::BooruGateway;
use crate::media::MediaCache;
use crate::settings::SettingsManager;
use crate::types::NavDirection;
use crate::view::ViewManager;
use eframe::egui;

/// Main reactor that orchestrates all components
pub struct Reactor {
	queue: EventQueue,
	scheduler: Scheduler,

	// components
	pub gateway: BooruGateway,
	pub browser: ContentBrowser,
	pub media: MediaCache,
	pub breathing: BreathingOverlay,
	pub view: ViewManager,
	pub settings: SettingsManager,
}

impl Reactor {
	pub fn new(ctx: &egui::Context) -> Self {
		Self {
			queue: EventQueue::new(),
			scheduler: Scheduler::new(),
			gateway: BooruGateway::new(),
			browser: ContentBrowser::new(),
			media: MediaCache::new(ctx),
			breathing: BreathingOverlay::new(),
			view: ViewManager::new(),
			settings: SettingsManager::new(),
		}
	}

	/// Called once per frame
	pub fn tick(&mut self, ctx: &egui::Context) {
		// Drain scheduled events
		self.scheduler.tick(&mut self.queue);

		// Process queue
		while let Some(event) = self.queue.pop() {
			let response = self.route(&event);
			for e in response.events {
				self.queue.push(e);
			}
			for (e, d) in response.scheduled {
				self.scheduler.schedule(e, d);
			}
		}

		// Render
		// We need to split the borrows to allow view to mutably access media
		let events = {
			let gateway = &self.gateway;
			let browser = &self.browser;
			let breathing = &self.breathing;
			let settings = &self.settings;

			self.view
				.render(ctx, gateway, browser, &mut self.media, breathing, settings)
		};

		// Queue any events from rendering
		for event in events {
			self.queue.push(event);
		}
	}

	fn route(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Source(e) => self.handle_source(e),
			Event::Gateway(_) => self.gateway.handle(event),
			Event::Browser(_) => self.browser.handle(event),
			Event::Media(_) => self.media.handle(event),
			Event::Breathing(_) => self.breathing.handle(event),
			Event::View(_) => self.view.handle(event),
			Event::Settings(_) => self.settings.handle(event),
		}
	}

	fn handle_source(&mut self, event: &SourceEvent) -> ComponentResponse {
		match event {
			SourceEvent::Search { query, page } => {
				ComponentResponse::emit(Event::Gateway(GatewayEvent::SearchRequest {
					query: query.clone(),
					page: *page,
					limit: 50,
				}))
			}
			SourceEvent::Navigate(direction) => {
				ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
					direction: *direction,
				}))
			}
			SourceEvent::KeyPress { key, modifiers } => {
				// Handle global key bindings
				self.handle_keypress(*key, *modifiers)
			}
		}
	}

	fn handle_keypress(&mut self, key: egui::Key, modifiers: egui::Modifiers) -> ComponentResponse {
		match key {
			egui::Key::Space => {
				if modifiers.ctrl {
					ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
						direction: NavDirection::Skip(10),
					}))
				} else if modifiers.shift {
					ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
						direction: NavDirection::Prev,
					}))
				} else {
					ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
						direction: NavDirection::Next,
					}))
				}
			}
			egui::Key::C => ComponentResponse::emit(Event::Settings(SettingsEvent::ToggleAutoPlay)),
			_ => ComponentResponse::none(),
		}
	}
}

impl eframe::App for Reactor {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		self.tick(ctx);
	}
}
