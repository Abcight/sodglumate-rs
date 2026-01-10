use crate::reactor::{ComponentResponse, Event, MediaEvent, ViewEvent};
use crate::types::{LoadedMedia, MediaHandle};
use eframe::egui;
use egui_video::{AudioDevice, Player};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;

/// Message from async image loading tasks
pub enum MediaMessage {
	ImageLoaded {
		url: String,
		result: Result<egui::ColorImage, String>,
	},
}

pub struct MediaCache {
	cache: HashMap<String, LoadedMedia>,
	loading_set: HashSet<String>,
	current_url: Option<String>,
	sender: mpsc::Sender<MediaMessage>,
	receiver: mpsc::Receiver<MediaMessage>,
	audio_device: AudioDevice,
	egui_ctx: egui::Context,
}

impl MediaCache {
	pub fn new(ctx: &egui::Context) -> Self {
		let (sender, receiver) = mpsc::channel(100);
		Self {
			cache: HashMap::new(),
			loading_set: HashSet::new(),
			current_url: None,
			sender,
			receiver,
			audio_device: AudioDevice::new().expect("Failed to create audio device"),
			egui_ctx: ctx.clone(),
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		// Drain completed async loads
		let mut responses = Vec::new();
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				MediaMessage::ImageLoaded { url, result } => {
					self.loading_set.remove(&url);
					match result {
						Ok(color_image) => {
							let texture = self.egui_ctx.load_texture(
								&url,
								color_image,
								egui::TextureOptions::LINEAR,
							);
							self.cache.insert(url.clone(), LoadedMedia::Image(texture));

							// If this is the current URL, notify ViewManager
							if Some(&url) == self.current_url.as_ref() {
								responses.push(Event::View(ViewEvent::MediaReady {
									handle: MediaHandle {
										url: url.clone(),
										is_video: false,
									},
								}));
							}
						}
						Err(error) => {
							responses.push(Event::Media(MediaEvent::LoadError { url, error }));
						}
					}
				}
			}
		}

		// Handle incoming events
		match event {
			Event::Media(MediaEvent::LoadRequest { url, is_video }) => {
				self.current_url = Some(url.clone());
				self.load_media(url.clone(), *is_video, &mut responses);
			}
			Event::Media(MediaEvent::Prefetch { urls }) => {
				for (url, is_video) in urls {
					self.load_media(url.clone(), *is_video, &mut responses);
				}
			}
			_ => {}
		}

		// Prune cache if needed
		self.prune_cache();

		if responses.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::emit_many(responses)
		}
	}

	fn load_media(&mut self, url: String, is_video: bool, responses: &mut Vec<Event>) {
		// Already cached?
		if self.cache.contains_key(&url) {
			if Some(&url) == self.current_url.as_ref() {
				responses.push(Event::View(ViewEvent::MediaReady {
					handle: MediaHandle {
						url: url.clone(),
						is_video,
					},
				}));
			}
			return;
		}

		// Already loading?
		if self.loading_set.contains(&url) {
			return;
		}

		// Rate limit to max 2 concurrent
		if self.loading_set.len() >= 2 {
			return;
		}

		self.loading_set.insert(url.clone());

		if is_video {
			// Video loading is synchronous via egui-video
			match Player::new(&self.egui_ctx, &url) {
				Ok(player) => {
					let player = match player.with_audio(&mut self.audio_device) {
						Ok(p) => p,
						Err(e) => {
							log::error!("Failed to enable audio for video: {} ({})", url, e);
							self.loading_set.remove(&url);
							return;
						}
					};
					self.cache.insert(url.clone(), LoadedMedia::Video(player));
					self.loading_set.remove(&url);

					if Some(&url) == self.current_url.as_ref() {
						responses.push(Event::View(ViewEvent::MediaReady {
							handle: MediaHandle {
								url,
								is_video: true,
							},
						}));
					}
				}
				Err(e) => {
					log::error!("Failed to load video {}: {}", url, e);
					self.loading_set.remove(&url);
				}
			}
		} else {
			// Image loading is async
			self.spawn_image_load(url);
		}
	}

	fn spawn_image_load(&self, url: String) {
		let sender = self.sender.clone();
		let ctx = self.egui_ctx.clone();

		tokio::spawn(async move {
			let result = async {
				let resp = reqwest::get(&url).await?;
				if !resp.status().is_success() {
					anyhow::bail!("HTTP Status: {}", resp.status());
				}
				let bytes = resp.bytes().await?;
				let img = image::load_from_memory(&bytes)?;
				let size = [img.width() as usize, img.height() as usize];
				let img_buffer = img.to_rgba8();
				let pixels = img_buffer.as_flat_samples();
				let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
				Ok::<_, anyhow::Error>(color_image)
			}
			.await;

			let _ = sender
				.send(MediaMessage::ImageLoaded {
					url,
					result: result.map_err(|e| e.to_string()),
				})
				.await;
			ctx.request_repaint();
		});
	}

	fn prune_cache(&mut self) {
		// Keep cache size reasonable (max 20 items)
		const MAX_CACHE_SIZE: usize = 20;
		if self.cache.len() > MAX_CACHE_SIZE {
			// Remove oldest entries
			let to_remove: Vec<String> = self
				.cache
				.keys()
				.filter(|k| Some(*k) != self.current_url.as_ref())
				.take(self.cache.len() - MAX_CACHE_SIZE)
				.cloned()
				.collect();

			for key in to_remove {
				if let Some(LoadedMedia::Video(mut player)) = self.cache.remove(&key) {
					player.stop();
				} else {
					self.cache.remove(&key);
				}
			}
		}
	}

	// Accessors for ViewManager
	pub fn get_media(&mut self, url: &str) -> Option<&mut LoadedMedia> {
		self.cache.get_mut(url)
	}

	pub fn current_url(&self) -> Option<&str> {
		self.current_url.as_deref()
	}

	pub fn is_loading(&self) -> bool {
		!self.loading_set.is_empty()
	}
}
