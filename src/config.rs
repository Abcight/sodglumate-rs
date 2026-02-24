use crate::types::BreathingStyle;
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSettings {
	pub search_query: String,
	pub search_page_input: String,
	pub auto_play: bool,
	pub auto_play_delay_secs: f32,
	pub cap_by_breathing: bool,
	pub breathing_idle_multiplier: f32,
	pub breathing_style: BreathingStyle,
	pub auto_pan_cycle_duration: f32,
	pub selected_audio_device: Option<String>,
	pub beat_pulse_enabled: bool,
	pub beat_pulse_scale: f32,
}

impl Default for SavedSettings {
	fn default() -> Self {
		Self {
			search_query: "~gay ~male solo abs wolf order:score".to_owned(),
			search_page_input: "1".to_owned(),
			auto_play: false,
			auto_play_delay_secs: 16.0,
			cap_by_breathing: false,
			breathing_idle_multiplier: 1.0,
			breathing_style: BreathingStyle::default(),
			auto_pan_cycle_duration: 10.0,
			selected_audio_device: None,
			beat_pulse_enabled: false,
			beat_pulse_scale: 0.03,
		}
	}
}

pub fn get_config_dir() -> Option<PathBuf> {
	if cfg!(target_os = "windows") {
		ProjectDirs::from("", "", "sodglumate").map(|p| p.config_dir().to_path_buf())
	} else {
		BaseDirs::new().map(|b| b.home_dir().join(".sodglumate"))
	}
}

pub fn load_settings() -> SavedSettings {
	if let Some(dir) = get_config_dir() {
		let path = dir.join("settings.toml");
		if let Ok(content) = fs::read_to_string(&path) {
			match toml::from_str(&content) {
				Ok(settings) => return settings,
				Err(e) => log::warn!("Failed to parse settings.toml: {}", e),
			}
		}
	}
	SavedSettings::default()
}

pub fn save_settings(settings: &SavedSettings) {
	if let Some(dir) = get_config_dir() {
		if let Err(e) = fs::create_dir_all(&dir) {
			log::warn!("Failed to create config directory: {}", e);
			return;
		}
		let path = dir.join("settings.toml");
		match toml::to_string(settings) {
			Ok(content) => {
				if let Err(e) = fs::write(&path, content) {
					log::warn!("Failed to write settings.toml: {}", e);
				}
			}
			Err(e) => log::warn!("Failed to serialize settings: {}", e),
		}
	}
}
