use crate::api::E621Client;
use crate::reactor::{BrowserEvent, ComponentResponse, Event, GatewayEvent};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// Message from async tasks back to the component
pub enum GatewayMessage {
	SearchComplete {
		posts: Vec<crate::api::Post>,
		page: u32,
		is_new: bool,
	},
	SearchError {
		message: String,
	},
}

pub struct BooruGateway {
	client: Arc<E621Client>,
	sender: mpsc::Sender<GatewayMessage>,
	receiver: mpsc::Receiver<GatewayMessage>,
	current_query: String,
	current_page: u32,
	fetch_pending: bool,
	last_request_times: VecDeque<Instant>,
}

impl BooruGateway {
	pub fn new() -> Self {
		log::info!("Initializing Gateway with rate limiting (2 req/sec)");
		let (sender, receiver) = mpsc::channel(100);
		Self {
			client: Arc::new(E621Client::new()),
			sender,
			receiver,
			current_query: String::new(),
			current_page: 1,
			fetch_pending: false,
			last_request_times: VecDeque::new(),
		}
	}

	/// Check if we can make an API request (hard limit: 2 req/sec)
	fn can_request(&self) -> bool {
		if self.last_request_times.len() < 2 {
			return true;
		}
		if let Some(oldest) = self.last_request_times.front() {
			oldest.elapsed().as_secs_f32() >= 1.0
		} else {
			true
		}
	}

	fn record_request(&mut self) {
		self.last_request_times.push_back(Instant::now());
		if self.last_request_times.len() > 2 {
			self.last_request_times.pop_front();
		}
	}

	pub fn poll(&mut self) -> ComponentResponse {
		let mut responses = Vec::new();
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				GatewayMessage::SearchComplete {
					posts,
					page,
					is_new,
				} => {
					log::info!(
						"Search complete: page={}, posts={}, is_new={}",
						page,
						posts.len(),
						is_new
					);
					self.fetch_pending = false;
					self.current_page = page;
					responses.push(Event::Browser(BrowserEvent::PostsReceived {
						posts,
						page,
						is_new,
					}));
				}
				GatewayMessage::SearchError { message } => {
					log::error!("Search error: {}", message);
					self.fetch_pending = false;
					responses.push(Event::Gateway(GatewayEvent::SearchError { message }));
				}
			}
		}

		if responses.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::emit_many(responses)
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Gateway(GatewayEvent::SearchRequest { query, page, limit }) => {
				if !self.can_request() {
					log::warn!("API rate limit exceeded, dropping search request");
					return ComponentResponse::none();
				}
				log::info!(
					"SearchRequest: query='{}', page={}, limit={}",
					query,
					page,
					limit
				);
				self.record_request();
				self.current_query = query.clone();
				self.current_page = *page;
				self.fetch_pending = true;
				self.spawn_search(query.clone(), *page, *limit, true);
			}
			Event::Gateway(GatewayEvent::FetchNextPage) => {
				if !self.can_request() {
					log::debug!("API rate limit: delaying FetchNextPage");
					return ComponentResponse::none();
				}
				if !self.fetch_pending && !self.current_query.is_empty() {
					let next_page = self.current_page + 1;
					log::info!(
						"FetchNextPage: query='{}', page={}",
						self.current_query,
						next_page
					);
					self.record_request();
					self.fetch_pending = true;
					self.spawn_search(self.current_query.clone(), next_page, 50, false);
				} else if self.fetch_pending {
					log::debug!("FetchNextPage ignored: fetch already pending");
				}
			}
			_ => {}
		}
		ComponentResponse::none()
	}

	fn spawn_search(&self, query: String, page: u32, limit: u32, is_new: bool) {
		log::info!(
			"Spawning API request: query='{}', page={}, limit={}",
			query,
			page,
			limit
		);
		let client = self.client.clone();
		let sender = self.sender.clone();

		tokio::spawn(async move {
			log::debug!("API request started: page={}", page);
			match client.search_posts(&query, limit, page).await {
				Ok(posts) => {
					log::info!(
						"API response: page={}, received {} posts",
						page,
						posts.len()
					);
					let _ = sender
						.send(GatewayMessage::SearchComplete {
							posts,
							page,
							is_new,
						})
						.await;
				}
				Err(e) => {
					log::error!("API error: page={}, error={}", page, e);
					let _ = sender
						.send(GatewayMessage::SearchError {
							message: e.to_string(),
						})
						.await;
				}
			}
		});
	}

	pub fn is_loading(&self) -> bool {
		self.fetch_pending
	}
}

impl Default for BooruGateway {
	fn default() -> Self {
		Self::new()
	}
}
