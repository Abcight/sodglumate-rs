use eframe::egui;

/// Loaded media content
pub enum LoadedMedia {
	Image { texture: egui::TextureHandle },
	Video(egui_video::Player),
}

/// Breathing timer phases
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
