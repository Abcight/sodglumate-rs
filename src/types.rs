use eframe::egui;

/// Loaded media content
pub enum LoadedMedia {
	Image { texture: egui::TextureHandle },
}

use serde::{Deserialize, Serialize};

/// Breathing overlay display style
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum BreathingStyle {
	#[default]
	Immersive, // Full progress bar overlay
	Classic, // Quick pop-in animation
}

/// How to fill the image in the view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ImageFillMode {
	#[default]
	Cover,
	Fit,
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
