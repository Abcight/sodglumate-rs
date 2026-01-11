use eframe::egui;

/// Renders text with simple formatting.
///
/// Supports:
/// - `*text*` for bold white text
/// - standard text as light gray
pub fn render_rich_text(ui: &mut egui::Ui, text: &str) {
	ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
		let mut job = egui::text::LayoutJob::default();
		job.wrap = egui::text::TextWrapping {
			max_width: ui.available_width(),
			..Default::default()
		};
		job.halign = egui::Align::LEFT;
		let mut in_bold = false;
		let mut current_text = String::new();

		for ch in text.chars() {
			if ch == '*' {
				// Flush current text
				if !current_text.is_empty() {
					let format = if in_bold {
						egui::TextFormat {
							font_id: egui::FontId::monospace(14.0),
							color: egui::Color32::WHITE,
							..Default::default()
						}
					} else {
						egui::TextFormat {
							font_id: egui::FontId::monospace(14.0),
							color: egui::Color32::LIGHT_GRAY,
							..Default::default()
						}
					};
					job.append(&current_text, 0.0, format);
					current_text.clear();
				}
				in_bold = !in_bold;
			} else {
				current_text.push(ch);
			}
		}
		// Flush remaining text
		if !current_text.is_empty() {
			let format = egui::TextFormat {
				font_id: egui::FontId::monospace(14.0),
				color: egui::Color32::LIGHT_GRAY,
				..Default::default()
			};
			job.append(&current_text, 0.0, format);
		}

		ui.label(job);
	});
}
