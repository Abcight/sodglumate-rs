use crate::reactor::{ComponentResponse, Event, MediaEvent, ViewEvent};
use crate::types::LoadedMedia;
use eframe::egui;

use indexmap::IndexMap;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::mpsc;

/// Number of background workers for general loading (samples + prefetch)
const NUM_WORKERS: usize = 4;

pub enum MediaMessage {
	ImageLoaded {
		url: String,
		is_sample: bool,
		full_url: String, // Key for cache lookup
		result: Result<egui::ColorImage, String>,
	},
}

/// A unit of work sent to a loading worker
struct LoadWork {
	url: String,
	is_sample: bool,
	cache_key: String,
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
	cache: IndexMap<String, (LoadedMedia, CacheState)>,
	loading_set: HashSet<String>,
	pending_set: HashSet<String>,

	// Current item being displayed
	current_item: Option<MediaItem>,

	// Pending queues for tiered loading
	pending_samples: VecDeque<MediaItem>, // Breadth-first samples
	pending_full: VecDeque<MediaItem>,    // Depth-first full versions

	// Worker channels
	priority_tx: mpsc::Sender<LoadWork>, // Current item full-res → priority worker
	work_tx: mpsc::Sender<LoadWork>,     // Everything else → general workers

	// Result channel
	receiver: mpsc::Receiver<MediaMessage>,

	egui_ctx: egui::Context,
}

impl MediaCache {
	pub fn new(ctx: &egui::Context) -> Self {
		log::info!(
			"Initializing MediaCache with {} workers + 1 priority worker",
			NUM_WORKERS
		);

		let (result_tx, result_rx) = mpsc::channel(100);

		// Priority channel: dedicated worker for current item full-res
		let (priority_tx, priority_rx) = mpsc::channel::<LoadWork>(8);
		Self::spawn_worker("priority", priority_rx, result_tx.clone(), ctx.clone());

		// General channel: NUM_WORKERS workers for samples + prefetch
		let (work_tx, work_rx) = mpsc::channel::<LoadWork>(128);
		let shared_rx = Arc::new(AsyncMutex::new(work_rx));
		for i in 0..NUM_WORKERS {
			Self::spawn_shared_worker(i, shared_rx.clone(), result_tx.clone(), ctx.clone());
		}

		Self {
			cache: IndexMap::new(),
			loading_set: HashSet::new(),
			pending_set: HashSet::new(),
			current_item: None,
			pending_samples: VecDeque::new(),
			pending_full: VecDeque::new(),
			priority_tx,
			work_tx,
			receiver: result_rx,
			egui_ctx: ctx.clone(),
		}
	}

	/// Spawn a dedicated worker with its own receiver
	fn spawn_worker(
		name: &'static str,
		rx: mpsc::Receiver<LoadWork>,
		result_tx: mpsc::Sender<MediaMessage>,
		ctx: egui::Context,
	) {
		let rx = Arc::new(AsyncMutex::new(rx));
		tokio::spawn(async move {
			log::info!("Media worker [{}] started", name);
			loop {
				let work = {
					let mut rx = rx.lock().await;
					rx.recv().await
				};
				let Some(work) = work else {
					log::info!("Media worker [{}] shutting down", name);
					break;
				};
				log::info!(
					"Worker [{}] loading: {} (sample={})",
					name,
					work.url,
					work.is_sample
				);
				let result = Self::load_image(&work.url).await;
				let _ = result_tx
					.send(MediaMessage::ImageLoaded {
						url: work.url,
						is_sample: work.is_sample,
						full_url: work.cache_key,
						result: result.map_err(|e| e.to_string()),
					})
					.await;
				ctx.request_repaint();
			}
		});
	}

	/// Spawn a worker that shares a receiver with other workers
	fn spawn_shared_worker(
		id: usize,
		rx: Arc<AsyncMutex<mpsc::Receiver<LoadWork>>>,
		result_tx: mpsc::Sender<MediaMessage>,
		ctx: egui::Context,
	) {
		tokio::spawn(async move {
			log::info!("Media worker [general-{}] started", id);
			loop {
				let work = {
					let mut rx = rx.lock().await;
					rx.recv().await
				};
				let Some(work) = work else {
					log::info!("Media worker [general-{}] shutting down", id);
					break;
				};
				log::info!(
					"Worker [general-{}] loading: {} (sample={})",
					id,
					work.url,
					work.is_sample
				);
				let result = Self::load_image(&work.url).await;
				let _ = result_tx
					.send(MediaMessage::ImageLoaded {
						url: work.url,
						is_sample: work.is_sample,
						full_url: work.cache_key,
						result: result.map_err(|e| e.to_string()),
					})
					.await;
				ctx.request_repaint();
			}
		});
	}

	/// Shared image loading logic used by all workers
	async fn load_image(url: &str) -> Result<egui::ColorImage, anyhow::Error> {
		let resp = reqwest::get(url).await?;
		if !resp.status().is_success() {
			anyhow::bail!("HTTP Status: {}", resp.status());
		}
		let bytes = resp.bytes().await?;
		let img = image::load_from_memory(&bytes)?;
		let size = [img.width() as usize, img.height() as usize];
		let img_buffer = img.to_rgba8();
		let pixels = img_buffer.as_flat_samples();
		let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels.as_slice());
		Ok(color_image)
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
		self.process_loading_queue();

		self.prune_cache();

		if responses.is_empty() {
			ComponentResponse::none()
		} else {
			ComponentResponse::emit_many(responses)
		}
	}

	fn process_loading_queue(&mut self) {
		// Always try to load both sample and full for the currently displayed item
		if let Some(ref current) = self.current_item.clone() {
			let cache_key = self.get_cache_key(current);
			let (has_sample, has_full) = self
				.cache
				.get(&cache_key)
				.map(|(_, state)| {
					(
						true,
						matches!(state, CacheState::Full), // Full implies sample content too
					)
				})
				.unwrap_or((false, false));

			let sample_loading = current
				.sample_url
				.as_ref()
				.map(|u| self.loading_set.contains(u))
				.unwrap_or(false);
			let full_loading = current
				.full_url
				.as_ref()
				.map(|u| self.loading_set.contains(u))
				.unwrap_or(false);

			// Kick off sample via general workers
			if !has_sample && !current.is_video {
				if let Some(ref sample_url) = current.sample_url {
					if !sample_loading {
						self.enqueue_load(sample_url.clone(), true, cache_key.clone(), false);
					}
				} else if let Some(ref full_url) = current.full_url {
					// No sample available; treat full as the first-tier load
					if !full_loading {
						self.enqueue_load(full_url.clone(), false, cache_key.clone(), true);
					}
				}
			}

			// Kick off full-res via priority worker
			if !has_full {
				if let Some(ref full_url) = current.full_url {
					if !full_loading {
						self.enqueue_load(full_url.clone(), false, cache_key.clone(), true);
					}
				}
			}
		}

		// Drain pending samples into general workers
		while let Some(item) = self.pending_samples.pop_front() {
			let cache_key = self.get_cache_key(&item);
			if self.cache.contains_key(&cache_key) {
				continue;
			}

			if let Some(ref sample_url) = item.sample_url {
				if !self.loading_set.contains(sample_url) {
					self.enqueue_load(sample_url.clone(), true, cache_key, false);
					self.pending_full.push_back(item);
				}
			} else if let Some(ref full_url) = item.full_url {
				if !self.loading_set.contains(full_url) {
					self.enqueue_load(full_url.clone(), false, cache_key, false);
				}
			}
		}

		// Drain pending full versions into general workers
		while let Some(item) = self.pending_full.pop_front() {
			let cache_key = self.get_cache_key(&item);
			let has_full = self
				.cache
				.get(&cache_key)
				.map(|(_, state)| matches!(state, CacheState::Full))
				.unwrap_or(false);
			if has_full {
				continue;
			}
			if let Some(ref full_url) = item.full_url {
				if !self.loading_set.contains(full_url) {
					self.enqueue_load(full_url.clone(), false, cache_key, false);
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

	/// Enqueue a load to either the priority or general work channel.
	fn enqueue_load(&mut self, url: String, is_sample: bool, cache_key: String, priority: bool) {
		if self.loading_set.contains(&url) {
			return;
		}
		let work = LoadWork {
			url: url.clone(),
			is_sample,
			cache_key,
		};
		let tx = if priority {
			&self.priority_tx
		} else {
			&self.work_tx
		};
		match tx.try_send(work) {
			Ok(()) => {
				self.loading_set.insert(url.clone());
				log::info!(
					"Enqueued load: {} (sample={}, priority={})",
					url,
					is_sample,
					priority
				);
			}
			Err(e) => {
				log::warn!("Work queue full, deferring: {} ({})", url, e);
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

				// Clear old pending items and reset
				self.pending_samples.clear();
				self.pending_full.clear();
				self.pending_set.clear();

				for (sample_url, full_url, is_video) in urls {
					let item = MediaItem {
						sample_url: sample_url.clone(),
						full_url: full_url.clone(),
						is_video: *is_video,
					};
					let cache_key = self.get_cache_key(&item);

					if !self.cache.contains_key(&cache_key)
						&& !self.loading_set.contains(&cache_key)
						&& !self.pending_set.contains(&cache_key)
					{
						self.pending_set.insert(cache_key);
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

	fn prune_cache(&mut self) {
		const MAX_CACHE_SIZE: usize = 100;
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
				self.cache.shift_remove(&key);
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
