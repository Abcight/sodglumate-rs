use crate::api::{E621Client, Post};
use eframe::egui;
use egui_video::Player;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppMessage {
	SearchCompleted(anyhow::Result<Vec<Post>>),
	ImageLoaded(String, anyhow::Result<egui::ColorImage>),
}

pub enum LoadedMedia {
	Image(egui::TextureHandle),
	Video(Player),
}

pub struct SodglumateApp {
	// State
	search_query: String,
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

	// Settings
	slide_show_timer: Option<std::time::Instant>,
	auto_play: bool,
	auto_play_delay: std::time::Duration,
}

impl SodglumateApp {
	pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
		let (sender, receiver) = mpsc::channel(100);

		Self {
			search_query: "male ~solo ~gay wolf abs order:score".to_owned(),
			posts: Vec::new(),
			current_index: 0,
			client: Arc::new(E621Client::new()),
			sender,
			receiver,
			is_loading: false,
			error_msg: None,
			current_media: None,
			slide_show_timer: None,
			auto_play: false,
			auto_play_delay: std::time::Duration::from_secs(5),
		}
	}

	fn perform_search(&mut self, ctx: &egui::Context) {
		self.is_loading = true;
		self.error_msg = None;
		self.posts.clear();
		self.current_index = 0;
		self.current_media = None;

		let client = self.client.clone();
		let query = self.search_query.clone();
		let sender = self.sender.clone();
		let ctx_clone = ctx.clone();

		let limit = 50;
		tokio::spawn(async move {
			let result = client.search_posts(&query, limit, 1).await;
			let _ = sender.send(AppMessage::SearchCompleted(result)).await;
			ctx_clone.request_repaint();
		});
	}

	fn load_current_media(&mut self, ctx: &egui::Context) {
		if let Some(post) = self.posts.get(self.current_index) {
			if let Some(url) = &post.file.url {
				// Check if already loaded
				if let Some((current_url, _)) = &self.current_media {
					if current_url == url {
						return;
					}
				}

				// Determine type
				let ext = post.file.ext.to_lowercase();
				let is_video = matches!(ext.as_str(), "mp4" | "webm" | "gif");

				self.current_media = None;
				self.is_loading = true;

				if is_video {
					// Load Video
					match Player::new(ctx, url) {
						Ok(mut player) => {
							player.start();
							self.current_media = Some((url.clone(), LoadedMedia::Video(player)));
							self.is_loading = false;
						}
						Err(e) => {
							log::error!("Failed to create video player: {}", e);
							self.error_msg = Some(format!("Failed to init video: {}", e));
							self.is_loading = false;
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
											url_clone,
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
												let size =
													[img.width() as usize, img.height() as usize];
												let img_buffer = img.to_rgba8();
												let pixels = img_buffer.as_flat_samples();
												let color_image =
													egui::ColorImage::from_rgba_unmultiplied(
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
		}
	}

	fn next_image(&mut self, ctx: &egui::Context) {
		if self.posts.is_empty() {
			return;
		}
		self.current_index = (self.current_index + 1) % self.posts.len();
		self.load_current_media(ctx);
	}

	fn prev_image(&mut self, ctx: &egui::Context) {
		if self.posts.is_empty() {
			return;
		}
		if self.current_index == 0 {
			self.current_index = self.posts.len() - 1;
		} else {
			self.current_index -= 1;
		}
		self.load_current_media(ctx);
	}
}

impl eframe::App for SodglumateApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		while let Ok(msg) = self.receiver.try_recv() {
			match msg {
				AppMessage::SearchCompleted(res) => {
					self.is_loading = false;
					match res {
						Ok(posts) => {
							self.posts = posts;
							if !self.posts.is_empty() {
								self.load_current_media(ctx);
							} else {
								self.error_msg = Some("No posts found".to_string());
							}
						}
						Err(e) => {
							self.error_msg = Some(format!("Search failed: {}", e));
						}
					}
				}
				AppMessage::ImageLoaded(url, res) => {
					if let Some(post) = self.posts.get(self.current_index) {
						if post.file.url.as_ref() == Some(&url) {
							self.is_loading = false;
							match res {
								Ok(img) => {
									let texture = ctx.load_texture(
										"post_image",
										img,
										egui::TextureOptions::LINEAR,
									);
									self.current_media = Some((url, LoadedMedia::Image(texture)));
								}
								Err(e) => {
									self.error_msg = Some(format!("Failed to load image: {}", e));
								}
							}
						}
					}
				}
			}
		}

		// Input handling
		let space_pressed = ctx.input(|i| i.key_pressed(egui::Key::Space));
		let shift_held = ctx.input(|i| i.modifiers.shift);

		if space_pressed {
			if shift_held {
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
				if ui.button("Search").clicked()
					|| (response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
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
			});
		});

		// Main Area
		egui::CentralPanel::default().show(ctx, |ui| {
			if self.is_loading {
				ui.spinner();
			} else if let Some(err) = &self.error_msg {
				ui.label(egui::RichText::new(err).color(egui::Color32::RED));
			} else if let Some((_, media)) = &mut self.current_media {
				let handle_scroll_input = |ui: &mut egui::Ui| {
					let mut scroll_delta = egui::Vec2::ZERO;
					let speed = 20.0;

					if ui.input(|i| i.key_down(egui::Key::ArrowRight)) {
						scroll_delta.x -= speed;
					}
					if ui.input(|i| i.key_down(egui::Key::ArrowLeft)) {
						scroll_delta.x += speed;
					}
					if ui.input(|i| i.key_down(egui::Key::ArrowDown)) {
						scroll_delta.y -= speed;
					}
					if ui.input(|i| i.key_down(egui::Key::ArrowUp)) {
						scroll_delta.y += speed;
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

						egui::ScrollArea::both().show(ui, |ui| {
							handle_scroll_input(ui);
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

							egui::ScrollArea::both().show(ui, |ui| {
								handle_scroll_input(ui);
								player.ui(ui, display_size);
							});
						} else {
							player.ui(ui, available_size);
						}
					}
				}
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
	}
}
