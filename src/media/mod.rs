use crate::reactor::{ComponentResponse, Event, MediaEvent, ViewEvent};
use crate::types::LoadedMedia;
use eframe::egui;
use egui_video::{AudioDevice, Player};
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc;

pub enum MediaMessage {
	ImageLoaded {
		url: String,
		result: Result<egui::ColorImage, String>,
	},
}

pub struct MediaCache {
	cache: HashMap<String, LoadedMedia>,
	loading_set: HashSet<String>,
	pending_prefetch: VecDeque<(String, bool)>,
	current_url: Option<String>,
	sender: mpsc::Sender<MediaMessage>,
	receiver: mpsc::Receiver<MediaMessage>,
	audio_device: Option<AudioDevice>,
	egui_ctx: egui::Context,
}

impl MediaCache {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!("[Media] Initializing");
		let (sender, receiver) = mpsc::channel(100);
		Self {
			cache: HashMap::new(),
			loading_set: HashSet::new(),
			pending_prefetch: VecDeque::new(),
			current_url: None,
			sender,
			receiver,
			audio_device: AudioDevice::new().ok(),
			egui_ctx: ctx.clone(),
		}
	}

	pub fn poll(&mut self) -> ComponentResponse {
		let mut responses = Vec::new();
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				MediaMessage::ImageLoaded { url, result } => {
					self.loading_set.remove(&url);
					match result {
						Ok(color_image) => {
							log::info!("[Media] Image loaded: {}", url);
							let texture = self.egui_ctx.load_texture(
								&url,
								color_image,
								egui::TextureOptions::LINEAR,
							);
							self.cache.insert(url.clone(), LoadedMedia::Image(texture));

							if Some(&url) == self.current_url.as_ref() {
								responses.push(Event::View(ViewEvent::MediaReady));
							}
						}
						Err(error) => {
							log::error!("[Media] Image load failed: {} - {}", url, error);
							responses.push(Event::Media(MediaEvent::LoadError { error }));
						}
					}
				}
			}
		}

		// Continuously try to load pending prefetch items
		while self.loading_set.len() < 2 {
			if let Some((url, is_video)) = self.pending_prefetch.pop_front() {
				self.load_media(url, is_video, &mut responses);
			} else {
				break;
			}
		}

		self.prune_cache();

		if responses.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::emit_many(responses)
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		let mut responses = Vec::new();

		match event {
			Event::Media(MediaEvent::LoadRequest { url, is_video }) => {
				log::info!("[Media] LoadRequest: {} (video={})", url, is_video);
				self.current_url = Some(url.clone());
				self.load_media(url.clone(), *is_video, &mut responses);
			}
			Event::Media(MediaEvent::Prefetch { urls }) => {
				log::debug!("[Media] Prefetch requested for {} URLs", urls.len());
				for (url, is_video) in urls {
					if !self.cache.contains_key(url) && !self.loading_set.contains(url) {
						self.pending_prefetch.push_back((url.clone(), *is_video));
					}
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

	fn load_media(&mut self, url: String, is_video: bool, responses: &mut Vec<Event>) {
		// Already cached?
		if self.cache.contains_key(&url) {
			log::debug!("[Media] Cache hit: {}", url);
			if Some(&url) == self.current_url.as_ref() {
				responses.push(Event::View(ViewEvent::MediaReady));
			}
			return;
		}

		// Already loading?
		if self.loading_set.contains(&url) {
			log::debug!("[Media] Already loading: {}", url);
			return;
		}

		// Rate limit to max 2 concurrent
		if self.loading_set.len() >= 2 {
			// Check if already queued
			if self.pending_prefetch.iter().any(|(u, _)| u == &url) {
				return;
			}
			log::debug!("[Media] At limit, queuing: {}", url);
			// Prioritize current_url at front
			if Some(&url) == self.current_url.as_ref() {
				self.pending_prefetch.push_front((url, is_video));
			} else {
				self.pending_prefetch.push_back((url, is_video));
			}
			return;
		}

		self.loading_set.insert(url.clone());
		log::info!("[Media] Starting load: {} (video={})", url, is_video);

		if is_video {
			match Player::new(&self.egui_ctx, &url) {
				Ok(player) => {
					let player = match self.audio_device.as_mut() {
						Some(audio_device) => player.with_audio(audio_device).unwrap(),
						None => player,
					};
					self.cache.insert(url.clone(), LoadedMedia::Video(player));
					self.loading_set.remove(&url);
					log::info!("[Media] Video loaded: {}", url);

					if Some(&url) == self.current_url.as_ref() {
						responses.push(Event::View(ViewEvent::MediaReady));
					}
				}
				Err(e) => {
					log::error!("[Media] Failed to load video {}: {}", url, e);
					self.loading_set.remove(&url);
				}
			}
		} else {
			self.spawn_image_load(url);
		}
	}

	fn spawn_image_load(&self, url: String) {
		log::debug!("[Media] Spawning async image load: {}", url);
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
		const MAX_CACHE_SIZE: usize = 20;
		if self.cache.len() > MAX_CACHE_SIZE {
			let to_remove: Vec<String> = self
				.cache
				.keys()
				.filter(|k| Some(*k) != self.current_url.as_ref())
				.take(self.cache.len() - MAX_CACHE_SIZE)
				.cloned()
				.collect();

			if !to_remove.is_empty() {
				log::debug!("[Media] Pruning {} items from cache", to_remove.len());
			}

			for key in to_remove {
				if let Some(LoadedMedia::Video(mut player)) = self.cache.remove(&key) {
					player.stop();
				} else {
					self.cache.remove(&key);
				}
			}
		}
	}

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
