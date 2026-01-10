use crate::reactor::{ComponentResponse, Event, MediaEvent, ViewEvent};
use crate::types::LoadedMedia;
use eframe::egui;
use egui_video::{AudioDevice, Player};
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc;

pub enum MediaMessage {
	ImageLoaded {
		url: String,
		is_sample: bool,
		full_url: String, // Key for cache lookup
		result: Result<egui::ColorImage, String>,
	},
}

/// Represents a media item's loading state
#[derive(Clone, Debug)]
pub struct MediaItem {
	pub sample_url: Option<String>,
	pub full_url: Option<String>,
	pub is_video: bool,
}

/// State of an item in the cache
#[derive(Clone, Debug)]
pub enum CacheState {
	SampleOnly,
	Full,
}

pub struct MediaCache {
	// Cache keyed by full_url (or sample_url if no full)
	cache: HashMap<String, (LoadedMedia, CacheState)>,
	loading_set: HashSet<String>,

	// Current item being displayed
	current_item: Option<MediaItem>,

	// Pending queues for tiered loading
	pending_samples: VecDeque<MediaItem>, // Breadth-first samples
	pending_full: VecDeque<MediaItem>,    // Depth-first full versions

	sender: mpsc::Sender<MediaMessage>,
	receiver: mpsc::Receiver<MediaMessage>,
	audio_device: Option<AudioDevice>,
	egui_ctx: egui::Context,
}

impl MediaCache {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!("Initializing MediaCache with tiered loading");
		let (sender, receiver) = mpsc::channel(100);
		Self {
			cache: HashMap::new(),
			loading_set: HashSet::new(),
			current_item: None,
			pending_samples: VecDeque::new(),
			pending_full: VecDeque::new(),
			sender,
			receiver,
			audio_device: AudioDevice::new().ok(),
			egui_ctx: ctx.clone(),
		}
	}

	pub fn poll(&mut self) -> ComponentResponse {
		let mut responses = Vec::new();

		// Process completed loads
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				MediaMessage::ImageLoaded {
					url,
					is_sample,
					full_url,
					result,
				} => {
					self.loading_set.remove(&url);
					match result {
						Ok(color_image) => {
							log::info!("Image loaded: {} (sample={})", url, is_sample);
							let texture = self.egui_ctx.load_texture(
								&url,
								color_image,
								egui::TextureOptions::LINEAR,
							);
							let state = if is_sample {
								CacheState::SampleOnly
							} else {
								CacheState::Full
							};
							self.cache
								.insert(full_url.clone(), (LoadedMedia::Image { texture }, state));

							let is_initial_load = if let Some(ref current) = self.current_item {
								if is_sample {
									true // Sample is always initial
								} else {
									// Full is initial only if there's no sample
									current.sample_url.is_none()
								}
							} else {
								false
							};

							if is_initial_load {
								if let Some(ref current) = self.current_item {
									if current.full_url.as_ref() == Some(&full_url)
										|| current.sample_url.as_ref() == Some(&full_url)
									{
										responses.push(Event::View(ViewEvent::MediaReady));
									}
								}
							}
						}
						Err(error) => {
							log::error!("Image load failed: {} - {}", url, error);
							responses.push(Event::Media(MediaEvent::LoadError { error }));
						}
					}
				}
			}
		}

		// Process loading queue with priority logic
		self.process_loading_queue(&mut responses);

		self.prune_cache();

		if responses.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::emit_many(responses)
		}
	}

	fn process_loading_queue(&mut self, responses: &mut Vec<Event>) {
		// Current item sample
		if let Some(ref current) = self.current_item.clone() {
			let cache_key = self.get_cache_key(&current);
			let has_sample = self.cache.contains_key(&cache_key);
			let has_full = self
				.cache
				.get(&cache_key)
				.map(|(_, state)| matches!(state, CacheState::Full))
				.unwrap_or(false);
			let full_loading = current
				.full_url
				.as_ref()
				.map(|u| self.loading_set.contains(u))
				.unwrap_or(false);

			if !has_sample && !current.is_video {
				// Load sample first
				if let Some(ref sample_url) = current.sample_url {
					if !self.loading_set.contains(sample_url) && self.loading_set.len() < 5 {
						self.start_image_load(sample_url.clone(), true, cache_key.clone());
						return; // Give sample priority
					}
				} else if let Some(ref full_url) = current.full_url {
					// No sample, load full directly
					if !self.loading_set.contains(full_url) && self.loading_set.len() < 5 {
						self.start_image_load(full_url.clone(), false, cache_key.clone());
						return;
					}
				}
			}

			// Current item full (after sample is loaded or loading)
			if has_sample && !has_full && !full_loading {
				if let Some(ref full_url) = current.full_url {
					if self.loading_set.len() < 5 {
						self.start_image_load(full_url.clone(), false, cache_key.clone());
					}
				}
			}
		}

		// Prefetch samples (breadth-first)
		while self.loading_set.len() < 5 {
			if let Some(item) = self.pending_samples.pop_front() {
				let cache_key = self.get_cache_key(&item);
				if self.cache.contains_key(&cache_key) {
					continue; // Already cached
				}
				if item.is_video {
					self.load_video(&item, responses);
				} else if let Some(ref sample_url) = item.sample_url {
					if !self.loading_set.contains(sample_url) {
						self.start_image_load(sample_url.clone(), true, cache_key);
						// Queue for full version after sample
						self.pending_full.push_back(item);
					}
				} else if let Some(ref full_url) = item.full_url {
					if !self.loading_set.contains(full_url) {
						self.start_image_load(full_url.clone(), false, cache_key);
					}
				}
			} else {
				break;
			}
		}

		// Prefetch full versions (depth-first, only after all samples done)
		if self.pending_samples.is_empty() {
			while self.loading_set.len() < 5 {
				if let Some(item) = self.pending_full.pop_front() {
					let cache_key = self.get_cache_key(&item);
					let has_full = self
						.cache
						.get(&cache_key)
						.map(|(_, state)| matches!(state, CacheState::Full))
						.unwrap_or(false);

					if has_full {
						continue; // Already have full
					}
					if let Some(ref full_url) = item.full_url {
						if !self.loading_set.contains(full_url) {
							self.start_image_load(full_url.clone(), false, cache_key);
						}
					}
				} else {
					break;
				}
			}
		}
	}

	fn get_cache_key(&self, item: &MediaItem) -> String {
		item.full_url
			.clone()
			.or_else(|| item.sample_url.clone())
			.unwrap_or_default()
	}

	fn start_image_load(&mut self, url: String, is_sample: bool, cache_key: String) {
		if self.loading_set.contains(&url) {
			return;
		}
		self.loading_set.insert(url.clone());
		log::info!("Starting load: {} (sample={})", url, is_sample);
		self.spawn_image_load(url, is_sample, cache_key);
	}

	fn load_video(&mut self, item: &MediaItem, responses: &mut Vec<Event>) {
		let url = match &item.full_url {
			Some(u) => u.clone(),
			None => return,
		};
		if self.cache.contains_key(&url) || self.loading_set.contains(&url) {
			return;
		}
		self.loading_set.insert(url.clone());
		log::info!("Loading video: {}", url);

		match Player::new(&self.egui_ctx, &url) {
			Ok(player) => {
				let player = match self.audio_device.as_mut() {
					Some(audio_device) => player.with_audio(audio_device).unwrap(),
					None => player,
				};
				self.cache
					.insert(url.clone(), (LoadedMedia::Video(player), CacheState::Full));
				self.loading_set.remove(&url);
				log::info!("Video loaded: {}", url);

				if let Some(ref current) = self.current_item {
					if current.full_url.as_ref() == Some(&url) {
						responses.push(Event::View(ViewEvent::MediaReady));
					}
				}
			}
			Err(e) => {
				log::error!("Failed to load video {}: {}", url, e);
				self.loading_set.remove(&url);
			}
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		let mut responses = Vec::new();

		match event {
			Event::Media(MediaEvent::LoadRequest {
				sample_url,
				full_url,
				is_video,
			}) => {
				log::info!(
					"LoadRequest: sample={:?}, full={:?} (video={})",
					sample_url,
					full_url,
					is_video
				);
				let item = MediaItem {
					sample_url: sample_url.clone(),
					full_url: full_url.clone(),
					is_video: *is_video,
				};
				self.current_item = Some(item.clone());

				// Check if already cached
				let cache_key = self.get_cache_key(&item);
				if self.cache.contains_key(&cache_key) {
					responses.push(Event::View(ViewEvent::MediaReady));
				}
			}
			Event::Media(MediaEvent::Prefetch { urls }) => {
				log::debug!("Prefetch requested for {} items", urls.len());
				for (sample_url, full_url, is_video) in urls {
					let item = MediaItem {
						sample_url: sample_url.clone(),
						full_url: full_url.clone(),
						is_video: *is_video,
					};
					let cache_key = self.get_cache_key(&item);
					if !self.cache.contains_key(&cache_key) {
						self.pending_samples.push_back(item);
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

	fn spawn_image_load(&self, url: String, is_sample: bool, cache_key: String) {
		log::debug!("Spawning async image load: {}", url);
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
					is_sample,
					full_url: cache_key,
					result: result.map_err(|e| e.to_string()),
				})
				.await;
			ctx.request_repaint();
		});
	}

	fn prune_cache(&mut self) {
		const MAX_CACHE_SIZE: usize = 20;
		if self.cache.len() > MAX_CACHE_SIZE {
			let current_key = self.current_item.as_ref().map(|i| self.get_cache_key(i));
			let to_remove: Vec<String> = self
				.cache
				.keys()
				.filter(|k| Some(*k) != current_key.as_ref())
				.take(self.cache.len() - MAX_CACHE_SIZE)
				.cloned()
				.collect();

			if !to_remove.is_empty() {
				log::debug!("Pruning {} items from cache", to_remove.len());
			}

			for key in to_remove {
				if let Some((LoadedMedia::Video(mut player), _)) = self.cache.remove(&key) {
					player.stop();
				} else {
					self.cache.remove(&key);
				}
			}
		}
	}

	/// Get the best available media for the current item
	pub fn get_current_media(&mut self) -> Option<&mut LoadedMedia> {
		let cache_key = self.current_item.as_ref().map(|i| self.get_cache_key(i))?;
		self.cache.get_mut(&cache_key).map(|(media, _)| media)
	}

	pub fn current_url(&self) -> Option<&str> {
		self.current_item
			.as_ref()
			.and_then(|i| i.full_url.as_deref().or(i.sample_url.as_deref()))
	}

	pub fn is_loading(&self) -> bool {
		!self.loading_set.is_empty()
	}
}
