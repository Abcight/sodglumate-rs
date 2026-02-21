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
		let mut reactor = Self {
			queue: EventQueue::new(),
			scheduler: Scheduler::new(),
			gateway: BooruGateway::new(),
			browser: ContentBrowser::new(),
			media: MediaCache::new(ctx),
			breathing: BreathingOverlay::new(),
			view: ViewManager::new(),
			settings: SettingsManager::new(),
			beat: SystemBeat::new(),
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
		match event {
			Event::Source(e) => self.handle_source(e),
			Event::Gateway(_) => self.gateway.handle(event),
			Event::Browser(_) => self.browser.handle(event),
			Event::Media(_) => self.media.handle(event),
			Event::Breathing(_) => self.breathing.handle(event),
			Event::View(_) => self.view.handle(event),
			Event::Settings(_) => self.settings.handle(event),
			Event::Beat(_) => self.beat.handle(event),
		}
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
}
