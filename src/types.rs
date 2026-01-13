use eframe::egui;

/// Loaded media content
pub enum LoadedMedia {
	Image { texture: egui::TextureHandle },
}

/// Breathing overlay display style
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BreathingStyle {
	#[default]
	Immersive, // Full progress bar overlay
	Classic, // Quick pop-in animation
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
