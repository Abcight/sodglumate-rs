use crate::api::E621Client;
use crate::reactor::{BrowserEvent, ComponentResponse, Event, GatewayEvent};
use std::sync::Arc;
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
}

impl BooruGateway {
	pub fn new() -> Self {
		let (sender, receiver) = mpsc::channel(100);
		Self {
			client: Arc::new(E621Client::new()),
			sender,
			receiver,
			current_query: String::new(),
			current_page: 1,
			fetch_pending: false,
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		// Drain any completed async messages
		let mut responses = Vec::new();
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				GatewayMessage::SearchComplete {
					posts,
					page,
					is_new,
				} => {
					self.fetch_pending = false;
					self.current_page = page;
					responses.push(Event::Browser(BrowserEvent::PostsReceived {
						posts,
						page,
						is_new,
					}));
				}
				GatewayMessage::SearchError { message } => {
					self.fetch_pending = false;
					responses.push(Event::Gateway(GatewayEvent::SearchError { message }));
				}
			}
		}

		// Handle incoming event
		match event {
			Event::Gateway(GatewayEvent::SearchRequest { query, page, limit }) => {
				self.current_query = query.clone();
				self.current_page = *page;
				self.fetch_pending = true;
				self.spawn_search(query.clone(), *page, *limit, true);
			}
			Event::Gateway(GatewayEvent::FetchNextPage) => {
				if !self.fetch_pending && !self.current_query.is_empty() {
					let next_page = self.current_page + 1;
					self.fetch_pending = true;
					self.spawn_search(self.current_query.clone(), next_page, 50, false);
				}
			}
			_ => {}
		}

		if responses.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::emit_many(responses)
		}
	}

	fn spawn_search(&self, query: String, page: u32, limit: u32, is_new: bool) {
		let client = self.client.clone();
		let sender = self.sender.clone();

		tokio::spawn(async move {
			match client.search_posts(&query, limit, page).await {
				Ok(posts) => {
					let _ = sender
						.send(GatewayMessage::SearchComplete {
							posts,
							page,
							is_new,
						})
						.await;
				}
				Err(e) => {
					let _ = sender
						.send(GatewayMessage::SearchError {
							message: e.to_string(),
						})
						.await;
				}
			}
		});
	}

	/// Check if a fetch is in progress
	pub fn is_loading(&self) -> bool {
		self.fetch_pending
	}
}

impl Default for BooruGateway {
	fn default() -> Self {
		Self::new()
	}
}
