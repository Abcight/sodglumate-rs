pub mod event;
pub mod queue;
pub mod scheduler;

pub use event::{
	BeatEvent, BreathingEvent, BrowserEvent, ComponentResponse, Event, GatewayEvent, MediaEvent,
	SettingsEvent, SourceEvent, ViewEvent,
};
pub use queue::EventQueue;
pub use scheduler::Scheduler;

use crate::beat::SystemBeat;
use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::gateway::BooruGateway;
use crate::media::MediaCache;
use crate::settings::SettingsManager;
use crate::view::ViewManager;
use eframe::egui;

pub struct Reactor {
	queue: EventQueue,
	scheduler: Scheduler,

	pub gateway: BooruGateway,
	pub browser: ContentBrowser,
	pub media: MediaCache,
	pub breathing: BreathingOverlay,
	pub view: ViewManager,
	pub settings: SettingsManager,
	pub beat: SystemBeat,
}

impl Reactor {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!("Initializing all components");
		let settings = crate::config::load_settings();

		let mut reactor = Self {
			queue: EventQueue::new(),
			scheduler: Scheduler::new(),
			gateway: BooruGateway::new(),
			browser: ContentBrowser::new(),
			media: MediaCache::new(ctx),
			breathing: BreathingOverlay::new(
				false, // Breathing always starts off
				settings.breathing_idle_multiplier,
				settings.breathing_style,
			),
			view: ViewManager::new(
				settings.search_query,
				settings.search_page_input,
				settings.auto_pan_cycle_duration,
				settings.beat_pulse_enabled,
				settings.beat_pulse_scale,
			),
			settings: SettingsManager::new(
				settings.auto_play,
				std::time::Duration::from_secs_f32(settings.auto_play_delay_secs),
				settings.cap_by_breathing,
			),
			beat: SystemBeat::new(settings.selected_audio_device),
		};

		// Initialize all components
		reactor.process_response(reactor.breathing.init());
		log::info!("Initialization complete");

		reactor
	}

	fn process_response(&mut self, response: ComponentResponse) {
		for e in response.events {
			self.queue.push(e);
		}
		for (e, d) in response.scheduled {
			self.scheduler.schedule(e, d);
		}
	}

	pub fn tick(&mut self, ctx: &egui::Context) {
		// Drain scheduled events
		self.scheduler.tick(&mut self.queue);

		// Poll async components
		let gateway_response = self.gateway.poll();
		let media_response = self.media.poll();
		let beat_response = self.beat.poll();
		self.process_response(gateway_response);
		self.process_response(media_response);
		self.process_response(beat_response);

		// Process event queue until empty
		let mut iterations = 0;
		while let Some(event) = self.queue.pop() {
			log::trace!("Processing event: {:?}", event);
			let response = self.route(&event);
			self.process_response(response);

			iterations += 1;
			if iterations > 1000 {
				log::warn!("Event loop exceeded 1000 iterations, breaking");
				break;
			}
		}

		// Render
		let events = {
			let gateway = &self.gateway;
			let browser = &self.browser;
			let breathing = &self.breathing;
			let settings = &self.settings;
			let beat = &self.beat;

			self.view.render(
				ctx,
				gateway,
				browser,
				&mut self.media,
				breathing,
				settings,
				beat,
			)
		};

		// Process any events from rendering immediately
		for event in events {
			log::trace!("Processing render event: {:?}", event);
			let response = self.route(&event);
			self.process_response(response);
		}
	}

	fn route(&mut self, event: &Event) -> ComponentResponse {
		let mut response;

		match event {
			Event::Source(e) => response = self.handle_source(e),
			Event::Gateway(_) => response = self.gateway.handle(event),
			Event::Browser(b) => {
				response = self.browser.handle(event);
				if let BrowserEvent::Navigate { .. } = b {
					let settings_res = self.settings.handle(event, &self.breathing);
					response.events.extend(settings_res.events);
					response.scheduled.extend(settings_res.scheduled);
				}
			}
			Event::Media(_) => response = self.media.handle(event),
			Event::View(_) => response = self.view.handle(event),
			Event::Beat(_) => response = self.beat.handle(event),
			Event::Breathing(b) => {
				response = self.breathing.handle(event);
				if let BreathingEvent::PhaseStarted(_) = b {
					// Route PhaseStarted to settings as well
					let settings_res = self.settings.handle(event, &self.breathing);
					response.events.extend(settings_res.events);
					response.scheduled.extend(settings_res.scheduled);
				}
			}
			Event::Settings(_) => response = self.settings.handle(event, &self.breathing),
		}

		response
	}

	fn handle_source(&mut self, event: &SourceEvent) -> ComponentResponse {
		match event {
			SourceEvent::Search { query, page } => {
				log::info!("Source search: query='{}', page={}", query, page);
				ComponentResponse::emit(Event::Gateway(GatewayEvent::SearchRequest {
					query: query.clone(),
					page: *page,
					limit: 50,
				}))
			}
			SourceEvent::Navigate(direction) => {
				log::debug!("Source navigate: {:?}", direction);
				ComponentResponse::emit(Event::Browser(BrowserEvent::Navigate {
					direction: *direction,
				}))
			}
		}
	}
}

impl eframe::App for Reactor {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		self.tick(ctx);
	}

	fn save(&mut self, _storage: &mut dyn eframe::Storage) {
		let saved = crate::config::SavedSettings {
			search_query: self.view.search_query.clone(),
			search_page_input: self.view.search_page_input.clone(),
			auto_play: self.settings.auto_play(),
			auto_play_delay_secs: self.settings.auto_play_delay().as_secs_f32(),
			cap_by_breathing: self.settings.cap_by_breathing(),
			breathing_idle_multiplier: self.breathing.idle_multiplier(),
			breathing_style: self.breathing.style(),
			auto_pan_cycle_duration: self.view.auto_pan_cycle_duration,
			selected_audio_device: self.beat.selected_device().clone(),
			beat_pulse_enabled: self.view.beat_pulse_enabled,
			beat_pulse_scale: self.view.beat_pulse_scale,
		};
		crate::config::save_settings(&saved);
	}
}
