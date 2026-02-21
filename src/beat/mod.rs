use crate::reactor::{BeatEvent, ComponentResponse, Event, ViewEvent};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;
use std::time::Instant;

/// Size of energy analysis window in samples
const WINDOW_SIZE: usize = 441;

/// Number of history windows for rolling average
const HISTORY_LEN: usize = 43;

/// Energy threshold multiplier over rolling average to trigger a beat
const BEAT_THRESHOLD: f32 = 1.5;

/// Minimum time between beats to avoid double-triggers
const BEAT_COOLDOWN_MS: u128 = 200;

pub struct SystemBeat {
	/// Raw audio samples from cpal stream
	sample_rx: mpsc::Receiver<Vec<f32>>,
	/// Sender cloned into cpal stream callback
	sample_tx: mpsc::Sender<Vec<f32>>,
	/// Active cpal stream (must be kept alive)
	stream: Option<cpal::Stream>,
	/// Available device names
	device_names: Vec<String>,
	/// Currently selected device name (None = default)
	selected_device: Option<String>,
	/// Energy detection state
	sample_buffer: Vec<f32>,
	energy_history: Vec<f32>,
	history_index: usize,
	last_beat: Instant,
}

impl SystemBeat {
	pub fn new() -> Self {
		let (sample_tx, sample_rx) = mpsc::channel();

		let device_names = Self::enumerate_devices();
		let stream = Self::start_stream_default(&sample_tx);

		Self {
			sample_rx,
			sample_tx,
			stream,
			device_names,
			selected_device: None,
			sample_buffer: Vec::with_capacity(WINDOW_SIZE * 2),
			energy_history: vec![0.0; HISTORY_LEN],
			history_index: 0,
			last_beat: Instant::now(),
		}
	}

	/// Enumerate all available input devices
	fn enumerate_devices() -> Vec<String> {
		let host = cpal::default_host();
		let mut names = Vec::new();
		if let Ok(devices) = host.input_devices() {
			for device in devices {
				if let Ok(name) = device.name() {
					names.push(name);
				}
			}
		}
		log::info!("Enumerated {} audio input devices", names.len());
		for name in &names {
			log::debug!("  Audio device: {}", name);
		}
		names
	}

	/// Start capture on the default input device
	fn start_stream_default(tx: &mpsc::Sender<Vec<f32>>) -> Option<cpal::Stream> {
		let host = cpal::default_host();
		let device = match host.default_input_device() {
			Some(d) => {
				let name = d.name().unwrap_or_else(|_| "unknown".into());
				log::info!("Using default audio input: {}", name);
				d
			}
			None => {
				log::warn!("No default audio input device found");
				return None;
			}
		};
		Self::start_stream_on_device(&device, tx)
	}

	/// Start capture on a named device
	fn start_stream_named(name: &str, tx: &mpsc::Sender<Vec<f32>>) -> Option<cpal::Stream> {
		let host = cpal::default_host();
		let devices = match host.input_devices() {
			Ok(d) => d,
			Err(e) => {
				log::error!("Failed to enumerate devices: {}", e);
				return None;
			}
		};
		for device in devices {
			if let Ok(dev_name) = device.name() {
				if dev_name == name {
					log::info!("Using audio device: {}", name);
					return Self::start_stream_on_device(&device, tx);
				}
			}
		}
		log::warn!("Audio device '{}' not found, falling back to default", name);
		Self::start_stream_default(tx)
	}

	/// Start a cpal input stream on a specific device
	fn start_stream_on_device(
		device: &cpal::Device,
		tx: &mpsc::Sender<Vec<f32>>,
	) -> Option<cpal::Stream> {
		let config = match device.default_input_config() {
			Ok(c) => c,
			Err(e) => {
				log::error!("Failed to get input config: {}", e);
				return None;
			}
		};

		log::info!(
			"Audio config: {} channels, {}Hz, {:?}",
			config.channels(),
			config.sample_rate().0,
			config.sample_format()
		);

		let tx = tx.clone();
		let channels = config.channels() as usize;

		let stream = match device.build_input_stream(
			&config.into(),
			move |data: &[f32], _: &cpal::InputCallbackInfo| {
				// Mix down to mono
				let mono: Vec<f32> = if channels > 1 {
					data.chunks(channels)
						.map(|frame| frame.iter().sum::<f32>() / channels as f32)
						.collect()
				} else {
					data.to_vec()
				};
				let _ = tx.send(mono);
			},
			move |err| {
				log::error!("Audio stream error: {}", err);
			},
			None,
		) {
			Ok(s) => s,
			Err(e) => {
				log::error!("Failed to build audio stream: {}", e);
				return None;
			}
		};

		if let Err(e) = stream.play() {
			log::error!("Failed to start audio stream: {}", e);
			return None;
		}

		Some(stream)
	}

	/// Poll for new audio data and detect beats
	pub fn poll(&mut self) -> ComponentResponse {
		// Drain all available samples
		while let Ok(samples) = self.sample_rx.try_recv() {
			self.sample_buffer.extend(samples);
		}

		let mut beat_detected = None;

		// Process complete windows
		while self.sample_buffer.len() >= WINDOW_SIZE {
			let window: Vec<f32> = self.sample_buffer.drain(..WINDOW_SIZE).collect();

			// Compute energy for this window
			let energy: f32 = window.iter().map(|s| s * s).sum::<f32>() / WINDOW_SIZE as f32;

			// Compute rolling average
			let avg_energy: f32 =
				self.energy_history.iter().sum::<f32>() / self.energy_history.len() as f32;

			// Update history ring buffer
			self.energy_history[self.history_index] = energy;
			self.history_index = (self.history_index + 1) % HISTORY_LEN;

			// Beat detection with cooldown
			if energy > avg_energy * BEAT_THRESHOLD
				&& avg_energy > 1e-8 // Avoid triggering on silence
				&& self.last_beat.elapsed().as_millis() > BEAT_COOLDOWN_MS
			{
				let scale = (energy / (avg_energy * BEAT_THRESHOLD)).min(3.0);
				beat_detected = Some(scale);
				self.last_beat = Instant::now();
			}
		}

		if let Some(scale) = beat_detected {
			log::debug!("Beat detected! scale={:.2}", scale);
			ComponentResponse::emit_many(vec![
				Event::Beat(BeatEvent::Beat { scale }),
				Event::View(ViewEvent::BeatPulse { scale }),
			])
		} else {
			ComponentResponse::none()
		}
	}

	pub fn handle(&mut self, event: &Event) -> ComponentResponse {
		match event {
			Event::Beat(BeatEvent::SetDevice { name }) => {
				log::info!("Switching audio device to: {:?}", name);
				// Drop old stream
				self.stream = None;
				self.selected_device = name.clone();

				// Reset detection state
				self.sample_buffer.clear();
				self.energy_history = vec![0.0; HISTORY_LEN];
				self.history_index = 0;

				// Start new stream
				self.stream = match name.as_deref() {
					Some(device_name) => Self::start_stream_named(device_name, &self.sample_tx),
					None => Self::start_stream_default(&self.sample_tx),
				};

				// Re-enumerate in case device list changed
				self.device_names = Self::enumerate_devices();

				ComponentResponse::none()
			}
			_ => ComponentResponse::none(),
		}
	}

	// Accessors for UI
	pub fn device_names(&self) -> &[String] {
		&self.device_names
	}

	pub fn selected_device(&self) -> &Option<String> {
		&self.selected_device
	}

	pub fn selected_device_label(&self) -> &str {
		self.selected_device.as_deref().unwrap_or("Default")
	}

	pub fn is_active(&self) -> bool {
		self.stream.is_some()
	}
}

impl Default for SystemBeat {
	fn default() -> Self {
		Self::new()
	}
}
