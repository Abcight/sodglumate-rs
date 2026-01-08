use crate::api::{E621Client, Post};
use eframe::egui::{self, Shadow, Stroke};
use egui_video::{AudioDevice, Player};
use rand::Rng;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppMessage {
	PostsFetched {
		posts: anyhow::Result<Vec<Post>>,
		page: u32,
		is_new_search: bool,
	},
	ImageLoaded(String, anyhow::Result<egui::ColorImage>),
}

pub enum LoadedMedia {
	Image(egui::TextureHandle),
	Video(Player),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BreathingPhase {
	Prepare,
	Inhale,
	Release,
	Idle,
}

#[derive(Debug, Clone, Copy)]
pub struct BreathingState {
	phase: BreathingPhase,
	start_time: Instant,
	duration: Duration,
}

pub struct SodglumateApp {
	// State
	search_query: String,
	search_page_input: String,
	posts: Vec<Post>,
	current_index: usize,

	// API & Async
	client: Arc<E621Client>,
	sender: mpsc::Sender<AppMessage>,
	receiver: mpsc::Receiver<AppMessage>,

	// UI State
	is_loading: bool,
	error_msg: Option<String>,
	current_media: Option<(String, LoadedMedia)>,

	// Caching & Prefetching
	media_cache: HashMap<String, LoadedMedia>,
	loading_set: HashSet<String>,
	current_page: u32,
	fetch_pending: bool,

	// Settings
	slide_show_timer: Option<std::time::Instant>,
	auto_play: bool,
	auto_play_delay: std::time::Duration,

	// Breathing Overlay
	show_breathing_overlay: bool,
	breathing_state: BreathingState,

	// Auto-Panning
	image_load_time: Instant,
	user_has_panned: bool,
	auto_pan_cycle_duration: f32,

	// Audio
	audio_device: AudioDevice,
}

impl SodglumateApp {
	pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
		let (sender, receiver) = mpsc::channel(100);

		Self {
			search_query: "~gay ~male solo abs wolf order:score -video".to_owned(),
			search_page_input: "1".to_owned(),
			posts: Vec::new(),
			current_index: 0,
			client: Arc::new(E621Client::new()),
			sender,
			receiver,
			is_loading: false,
			error_msg: None,
			current_media: None,
			media_cache: HashMap::new(),
			loading_set: HashSet::new(),
			current_page: 1,
			fetch_pending: false,
			slide_show_timer: None,
			auto_play: false,
			auto_play_delay: std::time::Duration::from_secs(16),
			show_breathing_overlay: false,
			breathing_state: BreathingState {
				phase: BreathingPhase::Prepare,
				start_time: Instant::now(),
				duration: Duration::from_secs(5),
			},
			image_load_time: Instant::now(),
			user_has_panned: false,
			auto_pan_cycle_duration: 10.0,
			audio_device: AudioDevice::new().expect("Failed to create audio device"),
		}
	}

	fn perform_search(&mut self, ctx: &egui::Context) {
		// Parse page input, default to 1
		let start_page = self.search_page_input.parse::<u32>().unwrap_or(1).max(1);

		self.is_loading = true;
		self.error_msg = None;
		self.posts.clear();
		self.current_index = 0;
		self.current_media = None;
		self.media_cache.clear();
		self.loading_set.clear();
		self.current_page = start_page;
		self.fetch_pending = false;

		let client = self.client.clone();
		let query = self.search_query.clone();
		let sender = self.sender.clone();
		let ctx_clone = ctx.clone();

		let limit = 50;
		tokio::spawn(async move {
			let result = client.search_posts(&query, limit, start_page).await;
			let _ = sender
				.send(AppMessage::PostsFetched {
					posts: result,
					page: start_page,
					is_new_search: true,
				})
				.await;
			ctx_clone.request_repaint();
		});
	}

	fn fetch_next_page(&mut self, ctx: &egui::Context) {
		if self.fetch_pending {
			return;
		}
		self.fetch_pending = true;

		let client = self.client.clone();
		let query = self.search_query.clone();
		let sender = self.sender.clone();
		let ctx_clone = ctx.clone();
		let next_page = self.current_page + 1;

		let limit = 50;
		tokio::spawn(async move {
			let result = client.search_posts(&query, limit, next_page).await;
			let _ = sender
				.send(AppMessage::PostsFetched {
					posts: result,
					page: next_page,
					is_new_search: false,
				})
				.await;
			ctx_clone.request_repaint();
		});
	}

	fn load_media_internal(&mut self, ctx: &egui::Context, url: String, is_video: bool) {
		if self.loading_set.contains(&url) || self.media_cache.contains_key(&url) {
			return;
		}

		// Rate Limiting: Max 2 concurrent downloads
		if self.loading_set.len() >= 2 {
			return;
		}

		self.loading_set.insert(url.clone());

		if is_video {
			match Player::new(ctx, &url) {
				Ok(player) => {
					// Enable Audio
					let player = match player.with_audio(&mut self.audio_device) {
						Ok(p) => p,
						Err(e) => {
							log::error!("Failed to enable audio for video: {} ({})", url, e);
							self.loading_set.remove(&url);
							return;
						}
					};
					// We don't start it yet.
					self.media_cache
						.insert(url.clone(), LoadedMedia::Video(player));
					self.loading_set.remove(&url);
				}
				Err(e) => {
					log::error!("Failed to prefetch video {}: {}", url, e);
					self.loading_set.remove(&url);
				}
			}
		} else {
			// Load Image logic
			let url_clone = url.clone();
			let sender = self.sender.clone();
			let ctx_clone = ctx.clone();
			log::info!("Fetching image from URL: {}", url_clone);

			tokio::spawn(async move {
				let resp = reqwest::get(&url_clone).await;
				match resp {
					Ok(r) => {
						let status = r.status();
						if !status.is_success() {
							let _ = sender
								.send(AppMessage::ImageLoaded(
									url_clone.clone(),
									Err(anyhow::anyhow!("HTTP Status: {}", status)),
								))
								.await;
							ctx_clone.request_repaint();
							return;
						}
						match r.bytes().await {
							Ok(bytes) => {
								// Decode image
								match image::load_from_memory(&bytes) {
									Ok(img) => {
										let size = [img.width() as usize, img.height() as usize];
										let img_buffer = img.to_rgba8();
										let pixels = img_buffer.as_flat_samples();
										let color_image = egui::ColorImage::from_rgba_unmultiplied(
											size,
											pixels.as_slice(),
										);
										let _ = sender
											.send(AppMessage::ImageLoaded(
												url_clone,
												Ok(color_image),
											))
											.await;
										ctx_clone.request_repaint();
									}
									Err(e) => {
										let _ = sender
											.send(AppMessage::ImageLoaded(
												url_clone,
												Err(anyhow::anyhow!("Decode error: {}", e)),
											))
											.await;
										ctx_clone.request_repaint();
									}
								}
							}
							Err(e) => {
								let _ = sender
									.send(AppMessage::ImageLoaded(
										url_clone,
										Err(anyhow::anyhow!("Bytes error: {}", e)),
									))
									.await;
								ctx_clone.request_repaint();
							}
						}
					}
					Err(e) => {
						let _ = sender
							.send(AppMessage::ImageLoaded(
								url_clone,
								Err(anyhow::anyhow!("Network error: {}", e)),
							))
							.await;
						ctx_clone.request_repaint();
					}
				}
			});
		}
	}

	fn prefetch_next(&mut self, ctx: &egui::Context, count: usize) {
		if self.posts.is_empty() {
			return;
		}

		let mut targets = Vec::new();
		for i in 1..=count {
			let idx = (self.current_index + i) % self.posts.len();
			if let Some(post) = self.posts.get(idx) {
				if let Some(url) = &post.file.url {
					let ext = post.file.ext.to_lowercase();
					let is_video = matches!(ext.as_str(), "mp4" | "webm" | "gif");
					targets.push((url.clone(), is_video));
				}
			}
		}

		for (url, is_video) in targets {
			self.load_media_internal(ctx, url, is_video);
		}
	}

	fn load_current_media(&mut self, ctx: &egui::Context) {
		let target = if let Some(post) = self.posts.get(self.current_index) {
			if let Some(url) = &post.file.url {
				let ext = post.file.ext.to_lowercase();
				let is_video = matches!(ext.as_str(), "mp4" | "webm" | "gif");
				Some((url.clone(), is_video))
			} else {
				None
			}
		} else {
			None
		};

		if let Some((url, is_video)) = target {
			// Check if already loaded
			if let Some((current_url, _)) = &self.current_media {
				if current_url == &url {
					return;
				}
			}

			self.current_media = None;
			self.image_load_time = Instant::now();
			self.user_has_panned = false;

			// 1. Check Cache
			if let Some(media) = self.media_cache.remove(&url) {
				match media {
					LoadedMedia::Video(mut player) => {
						player.start();
						self.current_media = Some((url.clone(), LoadedMedia::Video(player)));
					}
					LoadedMedia::Image(texture) => {
						self.current_media = Some((url.clone(), LoadedMedia::Image(texture)));
					}
				}
				self.is_loading = false;
			} else {
				// Not in cache, Load it.
				self.is_loading = true;

				self.load_media_internal(ctx, url.clone(), is_video);

				// If it was synchronous video load
				if is_video {
					if let Some(LoadedMedia::Video(mut player)) = self.media_cache.remove(&url) {
						player.start();
						self.current_media = Some((url.clone(), LoadedMedia::Video(player)));
						self.is_loading = false;
					}
				}
			}

			// Prefetch Next 2
			self.prefetch_next(ctx, 2);

			// Pagination Check
			if self.posts.len() >= 5 {
				if self.current_index >= self.posts.len() - 5 {
					self.fetch_next_page(ctx);
				}
			}
		}
	}

	fn cache_current_media(&mut self) {
		if let Some((url, media)) = self.current_media.take() {
			match media {
				LoadedMedia::Video(mut player) => {
					player.stop();
					self.media_cache.insert(url, LoadedMedia::Video(player));
				}
				LoadedMedia::Image(handle) => {
					self.media_cache.insert(url, LoadedMedia::Image(handle));
				}
			}
		}
	}

	fn next_image(&mut self, ctx: &egui::Context) {
		if self.posts.is_empty() {
			return;
		}
		self.cache_current_media();
		self.current_index = (self.current_index + 1) % self.posts.len();
		self.load_current_media(ctx);
	}

	fn prev_image(&mut self, ctx: &egui::Context) {
		if self.posts.is_empty() {
			return;
		}
		self.cache_current_media();
		if self.current_index == 0 {
			self.current_index = self.posts.len() - 1;
		} else {
			self.current_index -= 1;
		}
		self.load_current_media(ctx);
	}

	fn update_breathing(&mut self, ctx: &egui::Context) {
		let elapsed = self.breathing_state.start_time.elapsed();
		if elapsed >= self.breathing_state.duration {
			// Transition
			let mut rng = rand::rng();
			match self.breathing_state.phase {
				BreathingPhase::Prepare => {
					// -> Inhale (5-12s)
					let duration_secs = rng.random_range(5..=12);
					self.breathing_state = BreathingState {
						phase: BreathingPhase::Inhale,
						start_time: Instant::now(),
						duration: Duration::from_secs(duration_secs),
					};
				}
				BreathingPhase::Inhale => {
					// -> Release (4s)
					self.breathing_state = BreathingState {
						phase: BreathingPhase::Release,
						start_time: Instant::now(),
						duration: Duration::from_secs(4),
					};
				}
				BreathingPhase::Release => {
					// 20% -> Inhale
					// 80% -> Idle (17-28s)
					if rng.random_bool(0.2) {
						let duration_secs = rng.random_range(5..=12);
						self.breathing_state = BreathingState {
							phase: BreathingPhase::Inhale,
							start_time: Instant::now(),
							duration: Duration::from_secs(duration_secs),
						};
					} else {
						let duration_secs = rng.random_range(17..=28);
						self.breathing_state = BreathingState {
							phase: BreathingPhase::Idle,
							start_time: Instant::now(),
							duration: Duration::from_secs(duration_secs),
						};
					}
				}
				BreathingPhase::Idle => {
					// -> Prepare (5s)
					self.breathing_state = BreathingState {
						phase: BreathingPhase::Prepare,
						start_time: Instant::now(),
						duration: Duration::from_secs(5),
					};
				}
			}
		}

		// Continuous repaint if overlay is shown
		if self.show_breathing_overlay {
			ctx.request_repaint();
		}
	}
}

impl eframe::App for SodglumateApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// Update Breathing State (Always running)
		self.update_breathing(ctx);

		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				AppMessage::PostsFetched {
					posts: res,
					page,
					is_new_search,
				} => {
					self.fetch_pending = false;
					match res {
						Ok(new_posts) => {
							if is_new_search {
								self.is_loading = false;
								self.posts = new_posts;
								self.current_page = page;
								if !self.posts.is_empty() {
									self.load_current_media(ctx);
								} else {
									self.error_msg = Some("No posts found".to_string());
								}
							} else {
								// Append
								if !new_posts.is_empty() {
									self.posts.extend(new_posts);
									self.current_page = page;
								}
							}
						}
						Err(e) => {
							if is_new_search {
								self.is_loading = false;
								self.error_msg = Some(format!("Search failed: {}", e));
							} else {
								log::error!("Failed to fetch page {}: {}", page, e);
							}
						}
					}
				}
				AppMessage::ImageLoaded(url, res) => {
					self.loading_set.remove(&url);
					match res {
						Ok(img) => {
							let texture =
								ctx.load_texture("post_image", img, egui::TextureOptions::LINEAR);

							// Add to cache
							self.media_cache
								.insert(url.clone(), LoadedMedia::Image(texture.clone()));

							// If this is the CURRENT image we are waiting for, set it.
							if let Some(post) = self.posts.get(self.current_index) {
								if post.file.url.as_ref() == Some(&url) {
									// Move from cache to current
									if let Some(media) = self.media_cache.remove(&url) {
										self.current_media = Some((url, media));
										self.is_loading = false;
									}
								}
							}
						}
						Err(e) => {
							// Only show error if it's the current one
							if let Some(post) = self.posts.get(self.current_index) {
								if post.file.url.as_ref() == Some(&url) {
									self.error_msg = Some(format!("Failed to load image: {}", e));
									self.is_loading = false;
								}
							} else {
								log::warn!("Failed to load prefetched image {}: {}", url, e);
							}
						}
					}
				}
			}
		}

		// Input handling
		let space_pressed = ctx.input(|i| i.key_pressed(egui::Key::Space));
		let shift_pressed = ctx.input(|i| i.modifiers.shift);
		let c_pressed = ctx.input(|i| i.key_pressed(egui::Key::C));

		if c_pressed {
			self.auto_play = !self.auto_play;
		}

		if space_pressed {
			let ctrl_pressed = ctx.input(|i| i.modifiers.ctrl);

			if ctrl_pressed {
				// Skip 10 posts Logic
				if !self.posts.is_empty() {
					let target = (self.current_index + 10).min(self.posts.len().saturating_sub(1));
					if target != self.current_index {
						self.cache_current_media();
						self.current_index = target;
						self.load_current_media(ctx);
					}
				}
			} else if shift_pressed {
				self.prev_image(ctx);
			} else {
				self.next_image(ctx);
			}
		}

		// Video Controls (Z/X)
		if let Some((_, LoadedMedia::Video(ref mut player))) = self.current_media {
			// Z - Rewind
			if ctx.input(|i| i.key_pressed(egui::Key::Z)) {
				let current = player.elapsed_ms();
				let duration = player.duration_ms;
				if duration > 0 {
					let new_time = (current - 1000).max(0);
					let frac = new_time as f32 / duration as f32;
					player.seek(frac);
				}
			}
			// X - Forward
			if ctx.input(|i| i.key_pressed(egui::Key::X)) {
				let current = player.elapsed_ms();
				let duration = player.duration_ms;
				if duration > 0 {
					let new_time = (current + 1000).min(duration);
					let frac = new_time as f32 / duration as f32;
					player.seek(frac);
				}
			}
		}

		// Top Panel: Query
		egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
			ui.horizontal(|ui| {
				ui.label("Query:");
				let response = ui.text_edit_singleline(&mut self.search_query);

				ui.label("Page:");
				let page_response = ui.add(
					egui::TextEdit::singleline(&mut self.search_page_input).desired_width(40.0),
				);

				if ui.button("Search").clicked()
					|| (response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
					|| (page_response.lost_focus()
						&& ctx.input(|i| i.key_pressed(egui::Key::Enter)))
				{
					self.perform_search(ctx);
				}
			});
			ui.horizontal(|ui| {
				ui.label("Settings:");
				ui.checkbox(&mut self.auto_play, "Auto-play");
				if self.auto_play {
					let mut seconds = self.auto_play_delay.as_secs_f32();
					if ui
						.add(egui::Slider::new(&mut seconds, 1.0..=60.0).text("Interval (s)"))
						.changed()
					{
						self.auto_play_delay = std::time::Duration::from_secs_f32(seconds);
					}
				}
				ui.separator();
				ui.separator();
				ui.checkbox(&mut self.show_breathing_overlay, "Breathing Overlay");

				ui.separator();
				let mut speed = self.auto_pan_cycle_duration;
				if ui
					.add(egui::Slider::new(&mut speed, 10.0..=120.0).text("Pan Speed (s)"))
					.changed()
				{
					self.auto_pan_cycle_duration = speed;
				}
			});
		});

		// Main Area
		egui::CentralPanel::default().show(ctx, |ui| {
			if self.is_loading {
				ui.spinner();
			} else if let Some(err) = &self.error_msg {
				ui.label(egui::RichText::new(err).color(egui::Color32::RED));
			} else if let Some((_, media)) = &mut self.current_media {
				let pan_cycle = self.auto_pan_cycle_duration;
				let load_time = self.image_load_time;
				let mut user_panned = self.user_has_panned;

				let handle_scroll_input = |ui: &mut egui::Ui, input_active: &mut bool| {
					let mut scroll_delta = egui::Vec2::ZERO;
					let speed = 20.0;

					if ui.input(|i| i.key_down(egui::Key::ArrowRight) || i.key_down(egui::Key::D)) {
						scroll_delta.x -= speed;
						*input_active = true;
					}
					if ui.input(|i| i.key_down(egui::Key::ArrowLeft) || i.key_down(egui::Key::A)) {
						scroll_delta.x += speed;
						*input_active = true;
					}
					if ui.input(|i| i.key_down(egui::Key::ArrowDown) || i.key_down(egui::Key::S)) {
						scroll_delta.y -= speed;
						*input_active = true;
					}
					if ui.input(|i| i.key_down(egui::Key::ArrowUp) || i.key_down(egui::Key::W)) {
						scroll_delta.y += speed;
						*input_active = true;
					}

					if scroll_delta != egui::Vec2::ZERO {
						ui.scroll_with_delta(scroll_delta);
					}
				};

				match media {
					LoadedMedia::Image(texture) => {
						let available_size = ui.available_size();
						let img_size = texture.size_vec2();

						let width_ratio = available_size.x / img_size.x;
						let height_ratio = available_size.y / img_size.y;
						let scale = width_ratio.max(height_ratio);

						let display_size = img_size * scale;

						let mut scroll_area = egui::ScrollArea::both().scroll_bar_visibility(
							egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
						);

						// Auto-Pan
						if !user_panned {
							let elapsed = load_time.elapsed().as_secs_f32();
							let cycle = (elapsed * 2.0 * std::f32::consts::PI) / pan_cycle;
							let factor = cycle.sin() * 0.5 + 0.5;

							let overflow = display_size - available_size;
							if overflow.x > 0.0 {
								scroll_area =
									scroll_area.horizontal_scroll_offset(overflow.x * factor);
							}
							if overflow.y > 0.0 {
								scroll_area =
									scroll_area.vertical_scroll_offset(overflow.y * factor);
							}
							ctx.request_repaint();
						}

						scroll_area.show(ui, |ui| {
							handle_scroll_input(ui, &mut user_panned);
							ui.add(egui::Image::new(&*texture).fit_to_exact_size(display_size));
						});
					}
					LoadedMedia::Video(player) => {
						let available_size = ui.available_size();
						let width = player.size.x;
						let height = player.size.y;

						if width > 0.0 && height > 0.0 {
							let img_size = egui::vec2(width, height);
							let width_ratio = available_size.x / img_size.x;
							let height_ratio = available_size.y / img_size.y;
							let scale = width_ratio.max(height_ratio);
							let display_size = img_size * scale;

							let mut scroll_area = egui::ScrollArea::both().scroll_bar_visibility(
								egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
							);

							if !user_panned {
								let elapsed = load_time.elapsed().as_secs_f32();
								let cycle = (elapsed * 2.0 * std::f32::consts::PI) / pan_cycle;
								let factor = cycle.sin() * 0.5 + 0.5;

								let overflow = display_size - available_size;
								if overflow.x > 0.0 {
									scroll_area =
										scroll_area.horizontal_scroll_offset(overflow.x * factor);
								}
								if overflow.y > 0.0 {
									scroll_area =
										scroll_area.vertical_scroll_offset(overflow.y * factor);
								}
								ctx.request_repaint();
							}

							scroll_area.show(ui, |ui| {
								handle_scroll_input(ui, &mut user_panned);
								player.ui(ui, display_size);
							});
						} else {
							player.ui(ui, available_size);
						}
					}
				}
				self.user_has_panned = user_panned;
			} else {
				ui.centered_and_justified(|ui| {
					ui.label("Enter a query and search to start.");
				});
			}
		});

		// Auto-Play logic (Slideshow)
		if self.auto_play {
			if let Some(timer) = self.slide_show_timer {
				if timer.elapsed() > self.auto_play_delay {
					self.next_image(ctx);
					self.slide_show_timer = Some(std::time::Instant::now());
				}
			} else {
				self.slide_show_timer = Some(std::time::Instant::now());
			}
		} else {
			self.slide_show_timer = None;
		}

		// Breathing Overlay UI
		if self.show_breathing_overlay {
			// Dynamic Scaling
			let screen_height = ctx.screen_rect().height();
			let font_size = (screen_height * 0.05).max(16.0); // 5% of screen height
			let min_width = (screen_height * 0.3).max(200.0); // 30% of height
			let margin_offset = -(screen_height * 0.03).max(10.0); // 3% margin from edge

			egui::Area::new(egui::Id::new("breathing_overlay"))
				.anchor(
					egui::Align2::RIGHT_BOTTOM,
					egui::vec2(margin_offset, margin_offset),
				)
				.show(ctx, |ui| {
					// Background for readability
					egui::Frame::popup(ui.style())
						.fill(egui::Color32::TRANSPARENT)
						.stroke(Stroke::new(0.0, egui::Color32::TRANSPARENT))
						.shadow(Shadow::NONE)
						.inner_margin(egui::Margin::same(font_size * 0.5))
						.show(ui, |ui| {
							ui.with_layout(
								egui::Layout::right_to_left(egui::Align::Center),
								|ui| {
									let state = &self.breathing_state;
									let elapsed = state.start_time.elapsed();
									let remaining =
										state.duration.saturating_sub(elapsed).as_secs() + 1;

									let (text, color) = match state.phase {
										BreathingPhase::Prepare => {
											(format!("PREPARE {}", remaining), egui::Color32::RED)
										}
										BreathingPhase::Inhale => {
											("INHALE".to_string(), egui::Color32::YELLOW)
										}
										BreathingPhase::Release => {
											("RELEASE".to_string(), egui::Color32::GREEN)
										}
										BreathingPhase::Idle => {
											("".to_string(), egui::Color32::TRANSPARENT)
										}
									};

									if !text.is_empty() {
										let font_id = egui::FontId::monospace(font_size);

										let shadow_galley = ui.painter().layout_no_wrap(
											text.clone(),
											font_id.clone(),
											egui::Color32::BLACK,
										);

										let galley =
											ui.painter().layout_no_wrap(text, font_id, color);

										// Enforce min width
										let width = galley.size().x.max(min_width);
										let size = egui::vec2(width, galley.size().y);

										let (rect, _) =
											ui.allocate_exact_size(size, egui::Sense::hover());

										let shadow_size = (font_size * 0.05).max(1.0);
										let offsets = [
											egui::vec2(-shadow_size, -shadow_size),
											egui::vec2(0.0, -shadow_size),
											egui::vec2(shadow_size, -shadow_size),
											egui::vec2(-shadow_size, 0.0),
											egui::vec2(shadow_size, 0.0),
											egui::vec2(-shadow_size, shadow_size),
											egui::vec2(0.0, shadow_size),
											egui::vec2(shadow_size, shadow_size),
										];

										// Right align drawing position
										let draw_pos =
											egui::pos2(rect.max.x - galley.size().x, rect.min.y);

										for offset in offsets {
											ui.painter().galley(
												draw_pos + offset,
												shadow_galley.clone(),
												egui::Color32::BLACK,
											);
										}

										ui.painter().galley(draw_pos, galley, color);
									}
								},
							);
						});
				});
		}
	}
}
