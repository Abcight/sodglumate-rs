use crate::beat::SystemBeat;
use crate::breathing::BreathingOverlay;
use crate::browser::ContentBrowser;
use crate::gateway::BooruGateway;
use crate::media::MediaCache;
use crate::reactor::{
	BeatEvent, BreathingEvent, ComponentResponse, Event, GatewayEvent, MediaEvent, SettingsEvent,
	SourceEvent, ViewEvent,
};
use crate::settings::SettingsManager;
use crate::types::{BreathingPhase, BreathingStyle, LoadedMedia, NavDirection};
use eframe::egui::{self, ScrollArea};
use std::time::{Duration, Instant};

pub mod island;
pub mod text_utils;

use island::{IslandAction, IslandCtx, IslandWidget, ROOT_ISLAND};

/// Content for modal popups
#[derive(Clone)]
pub enum ModalContent {
	None,
	Hello,
	BreathingDisclaimer,
}

pub struct ViewManager {
	// Display state
	image_load_time: Instant,
	user_has_panned: bool,
	auto_pan_cycle_duration: f32,

	// UI state
	search_query: String,
	search_page_input: String,
	error_msg: Option<String>,
	user_is_adult: bool,
	user_accepted_tos: bool,

	// Modal state
	modal: ModalContent,
	breathing_disclaimer_accepted: bool,
	breathing_disclaimer_checked: bool,

	// Island navigation state
	island_ctx: IslandCtx,
	prev_shift_held: bool,

	// Beat debug state
	beat_intensity: f32,
	last_beat_time: Instant,
}

impl ViewManager {
	pub fn new() -> Self {
		Self {
			image_load_time: Instant::now(),
			user_has_panned: false,
			auto_pan_cycle_duration: 10.0,
			search_query: "~gay ~male solo abs wolf order:score".to_owned(),
			search_page_input: "1".to_owned(),
			error_msg: None,
			user_is_adult: false,
			user_accepted_tos: false,
			modal: ModalContent::Hello,
			breathing_disclaimer_accepted: false,
			breathing_disclaimer_checked: false,
			island_ctx: IslandCtx::new(),
			prev_shift_held: false,
			beat_intensity: 0.0,
			last_beat_time: Instant::now(),
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
			Event::View(ViewEvent::BeatPulse) => {
				self.beat_intensity = 1.0;
				self.last_beat_time = Instant::now();
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
		beat: &SystemBeat,
	) -> Vec<Event> {
		let mut events = Vec::new();
		let modal_active = !matches!(self.modal, ModalContent::None);

		// Handle input only when no modal is active
		if !modal_active {
			let is_typing = ctx.memory(|m| m.focused().is_some());
			if !is_typing {
				self.handle_keyboard_input(ctx, media, &mut events);
			}
		}

		// Top panel
		self.render_top_panel(
			ctx,
			gateway,
			settings,
			breathing,
			beat,
			&mut events,
			!modal_active,
		);

		// Central panel
		self.render_central_panel(ctx, browser, media, gateway, !modal_active);

		// Overlays
		match breathing.style() {
			BreathingStyle::Classic => {
				self.render_breathing_overlay(ctx, breathing);
				self.render_breathing_pulse(ctx, breathing);
			}
			BreathingStyle::Immersive => {
				self.render_immersive_breathing_overlay(ctx, breathing);
			}
		}
		self.render_info_overlay(ctx, browser);

		// Beat debug dot
		self.render_beat_debug(ctx, beat);

		// Island navigation overlay
		self.render_island_overlay(ctx, &mut events);

		// Modal popup (on top of everything)
		self.render_modal(ctx, &mut events);

		events
	}

	fn handle_keyboard_input(
		&mut self,
		ctx: &egui::Context,
		_media: &mut MediaCache,
		events: &mut Vec<Event>,
	) {
		// Detect shift press/release edges for island activation
		let shift_held = ctx.input(|i| i.modifiers.shift);
		if shift_held && !self.prev_shift_held {
			self.island_ctx.activate(&ROOT_ISLAND, 2);
		} else if !shift_held && self.prev_shift_held {
			self.island_ctx.deactivate();
		}
		self.prev_shift_held = shift_held;

		// Island overlay consumes all input when active or just closed
		if self.island_ctx.active || self.island_ctx.in_cooldown() {
			return;
		}

		let space_pressed = ctx.input(|i| i.key_pressed(egui::Key::Space));
		let ctrl_pressed = ctx.input(|i| i.modifiers.ctrl);
		let c_pressed = ctx.input(|i| i.key_pressed(egui::Key::C));

		if c_pressed {
			events.push(Event::Settings(SettingsEvent::ToggleAutoPlay));
		}

		if space_pressed {
			if ctrl_pressed {
				events.push(Event::Source(SourceEvent::Navigate(NavDirection::Skip(10))));
			} else {
				events.push(Event::Source(SourceEvent::Navigate(NavDirection::Next)));
			}
		}
	}

	fn render_top_panel(
		&mut self,
		ctx: &egui::Context,
		_gateway: &BooruGateway,
		settings: &SettingsManager,
		breathing: &BreathingOverlay,
		beat: &SystemBeat,
		events: &mut Vec<Event>,
		enabled: bool,
	) {
		egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
			if !enabled {
				ui.disable();
			}
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
				ui.label("Quick settings:");

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

				if ui.checkbox(&mut breathing_enabled, "Breathing").clicked() {
					if breathing_enabled && !self.breathing_disclaimer_accepted {
						self.modal = ModalContent::BreathingDisclaimer;
					} else {
						events.push(Event::Breathing(BreathingEvent::Toggle));
					}
				}

				if breathing_enabled {
					let mut idle_mult = breathing.idle_multiplier();
					if ui
						.add(egui::Slider::new(&mut idle_mult, 0.5..=3.0).text("Idle"))
						.changed()
					{
						events.push(Event::Breathing(BreathingEvent::SetIdleMultiplier {
							value: idle_mult,
						}));
					}

					let current_style = breathing.style();
					let style_label = match current_style {
						BreathingStyle::Classic => "Classic",
						BreathingStyle::Immersive => "Immersive",
					};
					egui::ComboBox::from_id_salt("breathing_style")
						.selected_text(style_label)
						.show_ui(ui, |ui| {
							if ui
								.selectable_label(
									current_style == BreathingStyle::Classic,
									"Classic",
								)
								.clicked()
							{
								events.push(Event::Breathing(BreathingEvent::SetStyle {
									style: BreathingStyle::Classic,
								}));
							}
							if ui
								.selectable_label(
									current_style == BreathingStyle::Immersive,
									"Immersive",
								)
								.clicked()
							{
								events.push(Event::Breathing(BreathingEvent::SetStyle {
									style: BreathingStyle::Immersive,
								}));
							}
						});
				}

				ui.separator();

				let mut pan_speed = self.auto_pan_cycle_duration;
				if ui
					.add(egui::Slider::new(&mut pan_speed, 10.0..=120.0).text("Pan Speed (s)"))
					.changed()
				{
					self.auto_pan_cycle_duration = pan_speed;
				}

				ui.separator();

				ui.label("Audio:");
				let selected_label = beat.selected_device_label();
				egui::ComboBox::from_id_salt("audio_device")
					.selected_text(selected_label)
					.show_ui(ui, |ui| {
						if ui
							.selectable_label(beat.selected_device().is_none(), "Default")
							.clicked()
						{
							events.push(Event::Beat(BeatEvent::SetDevice { name: None }));
						}
						for device_name in beat.device_names() {
							let is_selected =
								beat.selected_device().as_deref() == Some(device_name.as_str());
							if ui.selectable_label(is_selected, device_name).clicked() {
								events.push(Event::Beat(BeatEvent::SetDevice {
									name: Some(device_name.clone()),
								}));
							}
						}
					});
				if beat.is_active() {
					ui.label(
						egui::RichText::new("●")
							.color(egui::Color32::GREEN)
							.size(10.0),
					);
				} else {
					ui.label(
						egui::RichText::new("●")
							.color(egui::Color32::RED)
							.size(10.0),
					);
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
		enabled: bool,
	) {
		egui::CentralPanel::default().show(ctx, |ui| {
			if !enabled {
				ui.disable();
			}
			if gateway.is_loading() && browser.is_empty() {
				ui.centered_and_justified(|ui| {
					ui.spinner();
				});
			} else if let Some(err) = &self.error_msg {
				ui.label(egui::RichText::new(err).color(egui::Color32::RED));
			} else if let Some(_url) = media.current_url() {
				self.render_media(ui, ctx, media);
			} else {
				ui.centered_and_justified(|ui| {
					ui.label("Enter a query and search to start.");
				});
			}
		});
	}

	fn render_media(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, media: &mut MediaCache) {
		let pan_cycle = self.auto_pan_cycle_duration;
		let load_time = self.image_load_time;
		let mut user_panned = self.user_has_panned;
		let island_active = self.island_ctx.active || self.island_ctx.in_cooldown();

		let handle_scroll_input = |ui: &mut egui::Ui, input_active: &mut bool| {
			// Don't process scroll input when island overlay is active or just closed
			if island_active {
				return;
			}

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

		if let Some(loaded_media) = media.get_current_media() {
			match loaded_media {
				LoadedMedia::Image { texture } => {
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

	fn render_immersive_breathing_overlay(
		&self,
		ctx: &egui::Context,
		breathing: &BreathingOverlay,
	) {
		if !breathing.is_visible() {
			return;
		}

		let state = breathing.state();
		let elapsed = state.start_time.elapsed().as_secs_f32();
		let duration = state.duration.as_secs_f32();
		let progress = (elapsed / duration).clamp(0.0, 1.0);

		let screen_rect = ctx.screen_rect();
		let screen_width = screen_rect.width();
		let screen_height = screen_rect.height();

		// Calculate visual properties based on phase
		let (text, text_color, bar_fill, bar_bg_alpha, text_alpha) = match state.phase {
			BreathingPhase::Prepare => {
				// Text fades in fast, background fades in gradually
				let text_alpha = (progress * 4.0).min(1.0);
				let bg_alpha = progress * 0.4;
				("PREPARE", egui::Color32::RED, 0.0, bg_alpha, text_alpha)
			}
			BreathingPhase::Inhale => {
				// Fill bar from 0% to 100%
				("INHALE", egui::Color32::YELLOW, progress, 0.4, 1.0)
			}
			BreathingPhase::Hold => {
				// Bar stays full
				("HOLD", egui::Color32::YELLOW, 1.0, 0.4, 1.0)
			}
			BreathingPhase::Release => {
				// Empty the bar, fade out background and text
				let fade = 1.0 - progress;
				let bg_alpha = 0.4 * fade;
				("RELEASE", egui::Color32::GREEN, fade, bg_alpha, fade)
			}
			BreathingPhase::Idle => {
				// Fade everything out quickly
				let alpha = (1.0 - progress * 2.0).max(0.0);
				("", egui::Color32::TRANSPARENT, 0.0, 0.0, alpha)
			}
		};

		// Skip rendering if completely transparent
		if text_alpha <= 0.001 && bar_bg_alpha <= 0.001 {
			return;
		}

		ctx.request_repaint();

		// Render semi-transparent background overlay
		egui::Area::new(egui::Id::new("immersive_breathing_bg"))
			.fixed_pos(screen_rect.min)
			.order(egui::Order::Foreground)
			.interactable(false)
			.show(ctx, |ui| {
				let bg_alpha = (bar_bg_alpha * text_alpha * 180.0) as u8;
				ui.painter().rect_filled(
					screen_rect,
					0.0,
					egui::Color32::from_rgba_unmultiplied(0, 0, 0, bg_alpha),
				);
			});

		// Render progress bar just below the centered text
		let font_size = screen_height * 0.08;
		let bar_height = screen_height * 0.015;
		let text_center_y = screen_height / 2.0;
		let bar_y = text_center_y + (font_size * 0.6); // Small gap below text
		let bar_width = screen_width * 0.4;
		let bar_x = (screen_width - bar_width) / 2.0;
		let bar_rect =
			egui::Rect::from_min_size(egui::pos2(bar_x, bar_y), egui::vec2(bar_width, bar_height));

		if bar_bg_alpha > 0.001 {
			egui::Area::new(egui::Id::new("immersive_breathing_bar"))
				.fixed_pos(bar_rect.min)
				.order(egui::Order::Foreground)
				.interactable(false)
				.show(ctx, |ui| {
					let painter = ui.painter();
					let rounding = bar_height * 0.5;

					// Background track
					let bg_alpha = (text_alpha * 100.0) as u8;
					painter.rect_filled(
						bar_rect,
						rounding,
						egui::Color32::from_rgba_unmultiplied(40, 40, 50, bg_alpha),
					);

					// Filled portion
					if bar_fill > 0.001 {
						let fill_width = bar_rect.width() * bar_fill;
						let fill_rect = egui::Rect::from_min_size(
							bar_rect.min,
							egui::vec2(fill_width, bar_height),
						);
						let fill_color = text_color.gamma_multiply(text_alpha);
						painter.rect_filled(fill_rect, rounding, fill_color);
					}
				});
		}

		// Render centered text
		if !text.is_empty() {
			egui::Area::new(egui::Id::new("immersive_breathing_text"))
				.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
				.order(egui::Order::Foreground)
				.interactable(false)
				.show(ctx, |ui| {
					let font_id = egui::FontId::proportional(font_size);
					let display_color = text_color.gamma_multiply(text_alpha);
					let stroke_width = (font_size * 0.03).max(1.0);
					Self::draw_outlined_text(ui, text, font_id, display_color, stroke_width);
				});
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

		let num_passes = offsets.len() as f32;
		let base_alpha = color.a() as f32;
		let per_pass_alpha = (base_alpha / num_passes).max(1.0) as u8;
		let shadow_color = egui::Color32::from_rgba_unmultiplied(0, 0, 0, per_pass_alpha);

		for offset in offsets {
			let shadow_galley =
				ui.painter()
					.layout_no_wrap(text.to_string(), font_id.clone(), shadow_color);
			ui.painter()
				.galley(rect.min + offset, shadow_galley, shadow_color);
		}

		ui.painter().galley(rect.min, galley, color);
	}

	/// Render debug beat dot, pulses on beat detection
	fn render_beat_debug(&mut self, ctx: &egui::Context, _beat: &SystemBeat) {
		let elapsed = self.last_beat_time.elapsed().as_secs_f32();
		let decay_rate = 4.6;
		self.beat_intensity = (1.0_f32).min((-decay_rate * elapsed).exp());

		if self.beat_intensity < 0.01 {
			return;
		}

		ctx.request_repaint();

		let screen_rect = ctx.screen_rect();
		let margin = 20.0;
		let base_radius = 6.0;
		let bounce = 10.0;
		let radius = base_radius + self.beat_intensity * bounce;

		let center = egui::pos2(
			screen_rect.right() - margin - base_radius,
			screen_rect.bottom() - margin - base_radius,
		);

		let alpha = (self.beat_intensity * 255.0) as u8;
		let color = egui::Color32::from_rgba_unmultiplied(0, 220, 255, alpha);

		egui::Area::new(egui::Id::new("beat_debug_dot"))
			.fixed_pos(center)
			.order(egui::Order::Foreground)
			.interactable(false)
			.show(ctx, |ui| {
				ui.painter().circle_filled(center, radius, color);
				// Outer glow ring
				let glow_alpha = (self.beat_intensity * 100.0) as u8;
				let glow_color = egui::Color32::from_rgba_unmultiplied(0, 220, 255, glow_alpha);
				ui.painter().circle_stroke(
					center,
					radius + 3.0,
					egui::Stroke::new(2.0, glow_color),
				);
			});
	}

	/// Render island navigation overlay and handle actions
	fn render_island_overlay(&mut self, ctx: &egui::Context, events: &mut Vec<Event>) {
		if !matches!(self.modal, ModalContent::None) {
			return;
		}

		if let Some(action) = IslandWidget::new(&mut self.island_ctx).show(ctx) {
			match action {
				IslandAction::Emit(factory) => {
					let event = factory();
					// Intercept breathing toggle request to check disclaimer
					if matches!(event, Event::View(ViewEvent::RequestBreathingToggle)) {
						if !self.breathing_disclaimer_accepted {
							self.modal = ModalContent::BreathingDisclaimer;
						} else {
							events.push(Event::Breathing(BreathingEvent::Toggle));
						}
					} else {
						events.push(event);
					}
				}
				IslandAction::Push(island) => self.island_ctx.push(island),
				IslandAction::Pop => {
					self.island_ctx.pop();
				}
			}
		}
	}

	/// Render modal popup overlay
	fn render_modal(&mut self, ctx: &egui::Context, events: &mut Vec<Event>) {
		if matches!(self.modal, ModalContent::None) {
			return;
		}

		let screen_rect = ctx.screen_rect();

		// Draw semi-transparent dark overlay
		egui::Area::new(egui::Id::new("modal_backdrop"))
			.fixed_pos(screen_rect.min)
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				let painter = ui.painter();
				painter.rect_filled(
					screen_rect,
					0.0,
					egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180),
				);
			});

		// Draw centered popup window
		egui::Window::new("popup_modal")
			.title_bar(false)
			.resizable(false)
			.collapsible(false)
			.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
			.order(egui::Order::Foreground)
			.show(ctx, |ui| {
				ui.set_width(450.0);
				ui.vertical_centered(|ui| match &self.modal.clone() {
					ModalContent::Hello => {
						ui.add_space(10.0);
						ui.heading("Welcome! Please read the Terms of Use.");
						ui.label("Make sure you are of legal age to view this content.");
						ui.add_space(10.0);

						// Framed ScrollArea for legal text
						egui::Frame::none()
							.fill(egui::Color32::from_gray(40))
							.inner_margin(12.0)
							.rounding(4.0)
							.show(ui, |ui| {
								ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
									ui.with_layout(
										egui::Layout::top_down(egui::Align::LEFT),
										|ui| {
											text_utils::render_rich_text(ui, include_str!("resources/legal.txt"));
										},
									);
								});
							});

						ui.add_space(10.0);
						ui.label("If you do not meet these requirements or do not agree to these terms, you must not access or use the Application.");
						ui.add_space(10.0);

						ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
							ui.checkbox(&mut self.user_is_adult, "I am 18 years of age or older.");
							ui.checkbox(
								&mut self.user_accepted_tos,
								"I have read and accept the Terms of Use.",
							);
						});

						ui.add_space(10.0);

						ui.horizontal(|ui| {
							if ui.button("   Decline   ").clicked() {
								std::process::exit(0);
							}
							ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
								if !self.user_accepted_tos || !self.user_is_adult {
									ui.disable();
								}
								if ui.button("   Enter   ").clicked() {
									self.modal = ModalContent::None;
								}
							});
						});
					}
					ModalContent::BreathingDisclaimer => {
						ui.add_space(10.0);
						ui.heading("Breathing Disclaimer");
						ui.label("Please read the disclaimer below before using this functionality.");
						ui.add_space(10.0);

						egui::Frame::none()
							.fill(egui::Color32::from_gray(40))
							.inner_margin(12.0)
							.rounding(4.0)
							.show(ui, |ui| {
								ScrollArea::vertical()
									.scroll_bar_visibility(
										egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
									)
									.max_height(200.0)
									.show(ui, |ui| {
										ui.set_min_width(ui.available_width());
										ui.with_layout(
											egui::Layout::top_down(egui::Align::LEFT),
											|ui| {
												text_utils::render_rich_text(
													ui,
													include_str!("resources/breathing.txt"),
												);
											},
										);
									});
							});

						ui.add_space(10.0);
						ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
							ui.checkbox(
								&mut self.breathing_disclaimer_checked,
								"I understand the above disclaimer and proceed at my own risk.",
							);
						});
						ui.add_space(10.0);

						ui.horizontal(|ui| {
							if ui.button("   Decline   ").clicked() {
								self.modal = ModalContent::None;
								self.breathing_disclaimer_checked = false;
							}
							ui.with_layout(
								egui::Layout::right_to_left(egui::Align::Center),
								|ui| {
									if !self.breathing_disclaimer_checked {
										ui.disable();
									}
									if ui.button("   Accept   ").clicked() {
										self.breathing_disclaimer_accepted = true;
										self.modal = ModalContent::None;
										events.push(Event::Breathing(BreathingEvent::Toggle));
									}
								},
							);
						});
					},
					ModalContent::None => {}
				});
			});
	}
}

impl Default for ViewManager {
	fn default() -> Self {
		Self::new()
	}
}
