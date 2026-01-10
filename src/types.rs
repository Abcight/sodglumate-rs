use eframe::egui;

/// Loaded media content
pub enum LoadedMedia {
	Image(egui::TextureHandle),
	Video(egui_video::Player),
}

/// Handle to loaded media
#[derive(Clone, Debug)]
pub struct MediaHandle {
	pub url: String,
	pub is_video: bool,
}

/// Breathing exercise phases
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BreathingPhase {
	Prepare,
	Inhale,
	Hold,
	Release,
	Idle,
}

/// Navigation direction
#[derive(Debug, Clone, Copy)]
pub enum NavDirection {
	Next,
	Prev,
	Skip(i32),
}
