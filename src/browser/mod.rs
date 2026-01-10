use crate::api::Post;
use crate::reactor::{BrowserEvent, ComponentResponse, Event, GatewayEvent, MediaEvent};
use crate::types::NavDirection;

pub struct ContentBrowser {
	posts: Vec<Post>,
	current_index: usize,
	current_page: u32,
}

impl ContentBrowser {
	pub fn new() -> Self {
		Self {
			posts: Vec::new(),
			current_index: 0,
			current_page: 1,
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Browser(BrowserEvent::PostsReceived {
				posts,
				page,
				is_new,
			}) => {
				if *is_new {
					self.posts = posts.clone();
					self.current_index = 0;
					self.current_page = *page;
				} else {
					self.posts.extend(posts.clone());
					self.current_page = *page;
				}

				if !self.posts.is_empty() {
					self.emit_current_post_changed()
				} else {
					ComponentResponse::none()
				}
			}
			Event::Browser(BrowserEvent::Navigate { direction }) => {
				if self.posts.is_empty() {
					return ComponentResponse::none();
				}

				match direction {
					NavDirection::Next => {
						self.current_index = (self.current_index + 1) % self.posts.len();
					}
					NavDirection::Prev => {
						if self.current_index == 0 {
							self.current_index = self.posts.len().saturating_sub(1);
						} else {
							self.current_index -= 1;
						}
					}
					NavDirection::Skip(count) => {
						let count = *count;
						if count > 0 {
							self.current_index = (self.current_index + count as usize)
								.min(self.posts.len().saturating_sub(1));
						} else {
							self.current_index =
								self.current_index.saturating_sub((-count) as usize);
						}
					}
				}

				self.emit_current_post_changed()
			}
			_ => ComponentResponse::none(),
		}
	}

	fn emit_current_post_changed(&self) -> ComponentResponse {
		let post = self.posts.get(self.current_index).cloned();
		let mut events = Vec::new();

		if let Some(post) = post {
			// Request media load if URL available
			if let Some(url) = &post.file.url {
				let ext = post.file.ext.to_lowercase();
				let is_video = matches!(ext.as_str(), "mp4" | "webm" | "gif");
				events.push(Event::Media(MediaEvent::LoadRequest {
					url: url.clone(),
					is_video,
				}));
			}

			// Check if near end for prefetching
			let remaining = self.posts.len().saturating_sub(self.current_index + 1);
			if remaining < 5 {
				events.push(Event::Gateway(GatewayEvent::FetchNextPage));
			}

			// Emit prefetch hints for next 2 posts
			let prefetch_urls: Vec<(String, bool)> = (1..=2)
				.filter_map(|i| {
					let idx = (self.current_index + i) % self.posts.len();
					self.posts.get(idx).and_then(|p| {
						p.file.url.as_ref().map(|url| {
							let ext = p.file.ext.to_lowercase();
							let is_video = matches!(ext.as_str(), "mp4" | "webm" | "gif");
							(url.clone(), is_video)
						})
					})
				})
				.collect();

			if !prefetch_urls.is_empty() {
				events.push(Event::Media(MediaEvent::Prefetch {
					urls: prefetch_urls,
				}));
			}
		}

		ComponentResponse::emit_many(events)
	}

	pub fn current_post(&self) -> Option<&Post> {
		self.posts.get(self.current_index)
	}

	pub fn is_empty(&self) -> bool {
		self.posts.is_empty()
	}
}

impl Default for ContentBrowser {
	fn default() -> Self {
		Self::new()
	}
}
