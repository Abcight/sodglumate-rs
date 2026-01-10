use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::gateway::BooruGateway;
use crate::media::MediaCache;
use crate::reactor::{
	BreathingEvent, ComponentResponse, Event, GatewayEvent, MediaEvent, SettingsEvent, SourceEvent,
	ViewEvent,
};
use crate::settings::SettingsManager;
use crate::types::{BreathingPhase, LoadedMedia, NavDirection};
use eframe::egui;
use std::time::{Duration, Instant};

pub struct ViewManager {
	// Display state
	image_load_time: Instant,
	user_has_panned: bool,
	auto_pan_cycle_duration: f32,

	// UI state
	search_query: String,
	search_page_input: String,
	error_msg: Option<String>,
}

impl ViewManager {
	pub fn new() -> Self {
		Self {
			image_load_time: Instant::now(),
			user_has_panned: false,
			auto_pan_cycle_duration: 10.0,
			search_query: "~gay ~male solo abs wolf order:score -video".to_owned(),
			search_page_input: "1".to_owned(),
			error_msg: None,
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::View(ViewEvent::MediaReady) => {
				self.image_load_time = Instant::now();
				self.user_has_panned = false;
				self.error_msg = None;
				ComponentResponse::none()
			}
			Event::Gateway(GatewayEvent::SearchError { message }) => {
				self.error_msg = Some(message.clone());
				ComponentResponse::none()
			}
			Event::Media(MediaEvent::LoadError { error }) => {
				self.error_msg = Some(format!("Failed to load: {}", error));
				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	/// Main render function of the whole thing
	pub fn render(
		&mut self,
		ctx: &egui::Context,
		gateway: &BooruGateway,
		browser: &ContentBrowser,
		media: &mut MediaCache,
		breathing: &BreathingOverlay,
		settings: &SettingsManager,
	) -> Vec<Event> {
		let mut events = Vec::new();

		// Handle input
		let is_typing = ctx.memory(|m| m.focused().is_some());

		if !is_typing {
			self.handle_keyboard_input(ctx, media, &mut events);
		}

		// Top panel
		self.render_top_panel(ctx, gateway, settings, breathing, &mut events);

		// Central panel
		self.render_central_panel(ctx, browser, media, gateway);

		// Overlays
		self.render_breathing_overlay(ctx, breathing);
		self.render_breathing_pulse(ctx, breathing);
		self.render_info_overlay(ctx, browser);

		events
	}

	fn handle_keyboard_input(
		&mut self,
		ctx: &egui::Context,
		media: &mut MediaCache,
		events: &mut Vec<Event>,
	) {
		let space_pressed = ctx.input(|i| i.key_pressed(egui::Key::Space));
		let shift_pressed = ctx.input(|i| i.modifiers.shift);
		let ctrl_pressed = ctx.input(|i| i.modifiers.ctrl);
		let c_pressed = ctx.input(|i| i.key_pressed(egui::Key::C));

		if c_pressed {
			events.push(Event::Settings(SettingsEvent::ToggleAutoPlay));
		}

		if space_pressed {
			if ctrl_pressed {
				events.push(Event::Source(SourceEvent::Navigate(NavDirection::Skip(10))));
			} else if shift_pressed {
				events.push(Event::Source(SourceEvent::Navigate(NavDirection::Prev)));
			} else {
				events.push(Event::Source(SourceEvent::Navigate(NavDirection::Next)));
			}
		}

		// Video controls
		let current_url = media.current_url().map(|s| s.to_string());
		if let Some(url) = current_url {
			if let Some(LoadedMedia::Video(player)) = media.get_media(&url) {
				if ctx.input(|i| i.key_pressed(egui::Key::Z)) {
					let current = player.elapsed_ms();
					let duration = player.duration_ms;
					if duration > 0 {
						let new_time = current.saturating_sub(1000);
						let frac = new_time as f32 / duration as f32;
						player.seek(frac);
					}
				}
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
		}
	}

	fn render_top_panel(
		&mut self,
		ctx: &egui::Context,
		_gateway: &BooruGateway,
		settings: &SettingsManager,
		breathing: &BreathingOverlay,
		events: &mut Vec<Event>,
	) {
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
					let page = self.search_page_input.parse::<u32>().unwrap_or(1).max(1);
					events.push(Event::Source(SourceEvent::Search {
						query: self.search_query.clone(),
						page,
					}));
				}
			});

			ui.horizontal(|ui| {
				ui.label("Settings:");

				let mut auto_play = settings.auto_play();
				if ui.checkbox(&mut auto_play, "Auto-play").changed() {
					events.push(Event::Settings(SettingsEvent::ToggleAutoPlay));
				}

				if settings.auto_play() {
					let mut seconds = settings.auto_play_delay().as_secs_f32();
					if ui
						.add(egui::Slider::new(&mut seconds, 1.0..=60.0).text("Interval (s)"))
						.changed()
					{
						events.push(Event::Settings(SettingsEvent::SetDelay {
							duration: Duration::from_secs_f32(seconds),
						}));
					}
				}

				ui.separator();

				let mut breathing_enabled = breathing.is_visible();
				if ui.checkbox(&mut breathing_enabled, "Breathing").changed() {
					events.push(Event::Breathing(BreathingEvent::Toggle));
				}

				if breathing.is_visible() {
					let mut idle_mult = breathing.idle_multiplier();
					if ui
						.add(egui::Slider::new(&mut idle_mult, 0.5..=3.0).text("Idle"))
						.changed()
					{
						events.push(Event::Breathing(BreathingEvent::SetIdleMultiplier {
							value: idle_mult,
						}));
					}
				}

				ui.separator();

				let mut pan_speed = self.auto_pan_cycle_duration;
				if ui
					.add(egui::Slider::new(&mut pan_speed, 10.0..=120.0).text("Pan Speed (s)"))
					.changed()
				{
					self.auto_pan_cycle_duration = pan_speed;
				}
			});
		});
	}

	fn render_central_panel(
		&mut self,
		ctx: &egui::Context,
		browser: &ContentBrowser,
		media: &mut MediaCache,
		gateway: &BooruGateway,
	) {
		egui::CentralPanel::default().show(ctx, |ui| {
			if gateway.is_loading() && browser.is_empty() {
				ui.centered_and_justified(|ui| {
					ui.spinner();
				});
			} else if let Some(err) = &self.error_msg {
				ui.label(egui::RichText::new(err).color(egui::Color32::RED));
			} else if let Some(url) = media.current_url() {
				self.render_media(ui, ctx, media, url.to_string());
			} else {
				ui.centered_and_justified(|ui| {
					ui.label("Enter a query and search to start.");
				});
			}
		});
	}

	fn render_media(
		&mut self,
		ui: &mut egui::Ui,
		ctx: &egui::Context,
		media: &mut MediaCache,
		url: String,
	) {
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

		if let Some(loaded_media) = media.get_media(&url) {
			match loaded_media {
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

					// Auto
					if !user_panned {
						let elapsed = load_time.elapsed().as_secs_f32();
						let cycle = (elapsed * 2.0 * std::f32::consts::PI) / pan_cycle;
						let factor = (1.0 - cycle.cos()) * 0.5;

						let overflow = display_size - available_size;
						if overflow.x > 0.0 {
							scroll_area = scroll_area.horizontal_scroll_offset(overflow.x * factor);
						}
						if overflow.y > 0.0 {
							scroll_area = scroll_area.vertical_scroll_offset(overflow.y * factor);
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
		} else if media.is_loading() {
			ui.centered_and_justified(|ui| {
				ui.spinner();
			});
		}

		self.user_has_panned = user_panned;
	}

	fn render_breathing_overlay(&self, ctx: &egui::Context, breathing: &BreathingOverlay) {
		if !breathing.is_visible() {
			return;
		}

		let screen_height = ctx.screen_rect().height();
		let font_size = (screen_height * 0.05).max(16.0);
		let margin_offset = -(screen_height * 0.03).max(10.0);

		egui::Area::new(egui::Id::new("breathing_overlay"))
			.anchor(
				egui::Align2::RIGHT_BOTTOM,
				egui::vec2(margin_offset, margin_offset),
			)
			.interactable(false)
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					let state = breathing.state();
					let elapsed = state.start_time.elapsed();
					let remaining = state.duration.saturating_sub(elapsed).as_secs() + 1;

					let (text, color) = match state.phase {
						BreathingPhase::Prepare => {
							(format!("PREPARE {}", remaining), egui::Color32::RED)
						}
						BreathingPhase::Inhale => ("INHALE".to_string(), egui::Color32::YELLOW),
						BreathingPhase::Hold => ("HOLD".to_string(), egui::Color32::YELLOW),
						BreathingPhase::Release => ("RELEASE".to_string(), egui::Color32::GREEN),
						BreathingPhase::Idle => ("".to_string(), egui::Color32::TRANSPARENT),
					};

					if !text.is_empty() {
						let font_id = egui::FontId::monospace(font_size);
						let stroke_width = (font_size * 0.05).max(1.0);
						Self::draw_outlined_text(ui, &text, font_id, color, stroke_width);
					}
				});
			});
	}

	fn render_breathing_pulse(&self, ctx: &egui::Context, breathing: &BreathingOverlay) {
		if !breathing.is_visible() {
			return;
		}

		let state = breathing.state();
		let elapsed = state.start_time.elapsed().as_secs_f32();
		let pulse_duration = 1.5;

		if elapsed < pulse_duration {
			let t = elapsed / pulse_duration;
			let opacity = (t * std::f32::consts::PI).sin();
			let scale = 0.3 + 1.0 * (1.0 - (1.0 - t).powi(4));

			let (text, color) = match state.phase {
				BreathingPhase::Prepare => ("PREPARE", egui::Color32::RED),
				BreathingPhase::Inhale => ("INHALE", egui::Color32::YELLOW),
				BreathingPhase::Hold => ("HOLD", egui::Color32::YELLOW),
				BreathingPhase::Release => ("RELEASE", egui::Color32::GREEN),
				BreathingPhase::Idle => return,
			};

			let screen_rect = ctx.screen_rect();
			let center = screen_rect.center();
			let font_size = (screen_rect.height() * 0.15) * scale;

			egui::Area::new(egui::Id::new("breathing_pulse"))
				.fixed_pos(center)
				.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
				.interactable(false)
				.order(egui::Order::Foreground)
				.show(ctx, |ui| {
					let font_id = egui::FontId::proportional(font_size);
					let shadow_color = egui::Color32::BLACK.gamma_multiply(opacity);
					let text_color = color.gamma_multiply(opacity);

					let galley =
						ui.painter()
							.layout_no_wrap(text.to_string(), font_id.clone(), text_color);

					let stroke_width = (font_size * 0.02).max(1.0);
					let offsets = [
						egui::vec2(-stroke_width, -stroke_width),
						egui::vec2(0.0, -stroke_width),
						egui::vec2(stroke_width, -stroke_width),
						egui::vec2(-stroke_width, 0.0),
						egui::vec2(stroke_width, 0.0),
						egui::vec2(-stroke_width, stroke_width),
						egui::vec2(0.0, stroke_width),
						egui::vec2(stroke_width, stroke_width),
					];

					let text_size = galley.size();
					let draw_pos = center - (text_size / 2.0);

					for offset in offsets {
						let shadow_galley = ui.painter().layout_no_wrap(
							text.to_string(),
							font_id.clone(),
							shadow_color,
						);
						ui.painter()
							.galley(draw_pos + offset, shadow_galley, shadow_color);
					}
					ui.painter().galley(draw_pos, galley, text_color);
				});

			ctx.request_repaint();
		}
	}

	fn render_info_overlay(&self, ctx: &egui::Context, browser: &ContentBrowser) {
		if browser.is_empty() {
			return;
		}

		let post = match browser.current_post() {
			Some(p) => p,
			None => return,
		};

		let screen_height = ctx.screen_rect().height();
		let font_size = (screen_height * 0.02).max(12.0);
		let margin = (screen_height * 0.03).max(10.0);
		let stroke_width = (font_size * 0.05).max(1.0);

		egui::Area::new(egui::Id::new("image_info_overlay"))
			.anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(margin, -margin))
			.interactable(false)
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				let text_color = egui::Color32::WHITE;
				let font_id = egui::FontId::proportional(font_size);

				let add_text_line = |ui: &mut egui::Ui, label: &str, content: &str| {
					if !content.is_empty() {
						ui.horizontal(|ui| {
							Self::draw_outlined_text(
								ui,
								label,
								font_id.clone(),
								egui::Color32::LIGHT_GRAY,
								stroke_width,
							);
							Self::draw_outlined_text(
								ui,
								" ",
								font_id.clone(),
								egui::Color32::TRANSPARENT,
								0.0,
							);
							Self::draw_outlined_text(
								ui,
								content,
								font_id.clone(),
								text_color,
								stroke_width,
							);
						});
					}
				};

				ui.vertical(|ui| {
					add_text_line(ui, "Post ID:", &post.id.to_string());

					let artist_str = post.tags.artist.join(", ");
					if !artist_str.is_empty() && artist_str != "invalid_artist" {
						add_text_line(ui, "Artist:", &artist_str);
					}

					let copyright_str = post.tags.copyright.join(", ");
					if !copyright_str.is_empty() && copyright_str != "invalid_copyright" {
						add_text_line(ui, "Copyright:", &copyright_str);
					}
				});
			});
	}

	fn draw_outlined_text(
		ui: &mut egui::Ui,
		text: &str,
		font_id: egui::FontId,
		color: egui::Color32,
		stroke_width: f32,
	) {
		let galley = ui
			.painter()
			.layout_no_wrap(text.to_string(), font_id.clone(), color);
		let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());

		let shadow_color = egui::Color32::BLACK;
		let offsets = [
			egui::vec2(-stroke_width, -stroke_width),
			egui::vec2(0.0, -stroke_width),
			egui::vec2(stroke_width, -stroke_width),
			egui::vec2(-stroke_width, 0.0),
			egui::vec2(stroke_width, 0.0),
			egui::vec2(-stroke_width, stroke_width),
			egui::vec2(0.0, stroke_width),
			egui::vec2(stroke_width, stroke_width),
		];

		for offset in offsets {
			let shadow_galley =
				ui.painter()
					.layout_no_wrap(text.to_string(), font_id.clone(), shadow_color);
			ui.painter()
				.galley(rect.min + offset, shadow_galley, shadow_color);
		}

		ui.painter().galley(rect.min, galley, color);
	}
}

impl Default for ViewManager {
	fn default() -> Self {
		Self::new()
	}
}
