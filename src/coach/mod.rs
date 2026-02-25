use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_phi::ModelWeights;
use serde::Deserialize;
use std::collections::HashMap;
use tokenizers::Tokenizer;

#[derive(Debug, Clone)]
pub enum CoachValue {
	Number(f32),
	String(String),
}

impl<'de> Deserialize<'de> for CoachValue {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: serde::Deserializer<'de>,
	{
		struct CoachValueVisitor;

		impl<'de> serde::de::Visitor<'de> for CoachValueVisitor {
			type Value = CoachValue;

			fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
				formatter.write_str("a number or a string")
			}

			fn visit_i64<E>(self, value: i64) -> Result<CoachValue, E>
			where
				E: serde::de::Error,
			{
				Ok(CoachValue::Number(value as f32))
			}

			fn visit_u64<E>(self, value: u64) -> Result<CoachValue, E>
			where
				E: serde::de::Error,
			{
				Ok(CoachValue::Number(value as f32))
			}

			fn visit_f64<E>(self, value: f64) -> Result<CoachValue, E>
			where
				E: serde::de::Error,
			{
				Ok(CoachValue::Number(value as f32))
			}

			fn visit_str<E>(self, value: &str) -> Result<CoachValue, E>
			where
				E: serde::de::Error,
			{
				Ok(CoachValue::String(value.to_owned()))
			}

			fn visit_string<E>(self, value: String) -> Result<CoachValue, E>
			where
				E: serde::de::Error,
			{
				Ok(CoachValue::String(value))
			}
		}

		deserializer.deserialize_any(CoachValueVisitor)
	}
}

impl std::fmt::Display for CoachValue {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			CoachValue::Number(n) => write!(f, "{}", n),
			CoachValue::String(s) => write!(f, "{}", s),
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoachConfig {
	pub system_prompt: Option<String>,
	pub rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
pub struct Message {
	pub role: String,
	pub content: String,
}

impl Message {
	pub fn to_chatml(&self) -> String {
		format!("<|im_start|>{}\n{}<|im_end|>\n", self.role, self.content)
	}
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
	pub on_event: String,
	pub conditions: Option<Vec<Condition>>,
	pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
	SetValue {
		key: String,
		value: CoachValue,
	},
	IncreaseValue {
		key: String,
		amount: f32,
	},
	IncreaseValueByValue {
		target_key: String,
		by_key: String,
	},
	SetState {
		key: String,
		value: CoachValue,
	},
	EmitMessage {
		prompt_template: String,
		max_tokens: Option<usize>,
	},
	StoreMessage {
		prompt_template: String,
		max_tokens: Option<usize>,
		store_at: String,
	},
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Condition {
	Equal { key: String, value: f32 },
	NotEqual { key: String, value: f32 },
	Greater { key: String, value: f32 },
	GreaterOrEqual { key: String, value: f32 },
	Less { key: String, value: f32 },
	LessOrEqual { key: String, value: f32 },
}

pub struct CoachState {
	pub variables: HashMap<String, CoachValue>,
}

impl CoachState {
	pub fn new() -> Self {
		Self {
			variables: HashMap::new(),
		}
	}

	pub fn get_number(&self, key: &str) -> f32 {
		match self.variables.get(key) {
			Some(CoachValue::Number(n)) => *n,
			_ => 0.0,
		}
	}

	pub fn set(&mut self, key: String, val: CoachValue) {
		self.variables.insert(key, val);
	}

	pub fn increase(&mut self, key: String, amount: f32) {
		let cur = self.get_number(&key);
		self.set(key, CoachValue::Number(cur + amount));
	}
}

pub enum CoachEvent {
	Load,
	NextImage,
	PrevImage,
	PhaseChange(String),
}

impl CoachEvent {
	pub fn as_str(&self) -> &str {
		match self {
			CoachEvent::Load => "Load",
			CoachEvent::NextImage => "NextImage",
			CoachEvent::PrevImage => "PrevImage",
			CoachEvent::PhaseChange(phase) => phase.as_str(),
		}
	}
}

pub struct CoachManager {
	tx: std::sync::mpsc::Sender<CoachEvent>,
	rx: std::sync::mpsc::Receiver<String>,
}

impl CoachManager {
	pub fn new(model_path: std::path::PathBuf, preset_path: std::path::PathBuf) -> Self {
		let (event_tx, event_rx) = std::sync::mpsc::channel();
		let (msg_tx, msg_rx) = std::sync::mpsc::channel();

		std::thread::spawn(move || {
			let mut worker = CoachWorker::new(model_path, preset_path, msg_tx);
			worker.run(event_rx);
		});

		Self {
			tx: event_tx,
			rx: msg_rx,
		}
	}

	pub fn send_event(&self, event: CoachEvent) {
		let _ = self.tx.send(event);
	}

	pub fn try_recv(&self) -> Option<String> {
		self.rx.try_recv().ok()
	}
}

struct CoachWorker {
	model_path: std::path::PathBuf,
	config: CoachConfig,
	state: CoachState,
	history: Vec<Message>,
	device: Device,
	tx: std::sync::mpsc::Sender<String>,
}

impl CoachWorker {
	fn new(
		model_path: std::path::PathBuf,
		preset_path: std::path::PathBuf,
		tx: std::sync::mpsc::Sender<String>,
	) -> Self {
		// Load config
		let config_str = std::fs::read_to_string(&preset_path).unwrap_or_default();
		let config: CoachConfig = toml::from_str(&config_str).unwrap_or_else(|e| {
			log::error!("Failed to parse Coach config TOML: {}", e);
			CoachConfig {
				system_prompt: None,
				rules: vec![],
			}
		});

		let device = Self::select_inference_device();

		Self {
			model_path,
			config,
			state: CoachState::new(),
			history: Vec::new(),
			device,
			tx,
		}
	}

	fn select_inference_device() -> Device {
		#[cfg(feature = "cuda")]
		{
			match Device::new_cuda(0) {
				Ok(device) => {
					log::info!("Coach inference using CUDA GPU (device 0)");
					return device;
				}
				Err(err) => {
					log::warn!("CUDA device unavailable, falling back: {}", err);
				}
			}
		}

		#[cfg(all(feature = "metal", target_os = "macos"))]
		{
			match Device::new_metal(0) {
				Ok(device) => {
					log::info!("Coach inference using Metal GPU (device 0)");
					return device;
				}
				Err(err) => {
					log::warn!("Metal device unavailable, falling back: {}", err);
				}
			}
		}

		if cfg!(feature = "cuda") || cfg!(feature = "metal") {
			log::info!("Coach falling back to CPU inference (GPU init failed or not present)");
		} else {
			log::info!("Coach built without GPU features; using CPU inference");
		}

		Device::Cpu
	}

	fn run(&mut self, rx: std::sync::mpsc::Receiver<CoachEvent>) {
		// Initialize the candle model
		let mut model: Option<ModelWeights> = None;
		let mut tokenizer: Option<Tokenizer> = None;

		// Load Tokenizer
		let tokenizer_path = self.model_path.with_extension("json");
		if tokenizer_path.exists() {
			match Tokenizer::from_file(&tokenizer_path) {
				Ok(tok) => {
					log::info!("Successfully loaded Tokenizer");
					tokenizer = Some(tok);
				}
				Err(e) => log::error!("Failed to load Tokenizer: {}", e),
			}
		} else {
			log::warn!("Tokenizer file not found at {:?}", tokenizer_path);
		}

		log::info!("Coach worker initializing with model {:?}", self.model_path);
		if self.model_path.exists() {
			match std::fs::File::open(&self.model_path) {
				Ok(mut file) => match gguf_file::Content::read(&mut file) {
					Ok(content) => {
						match ModelWeights::from_gguf(content, &mut file, &self.device) {
							Ok(w) => {
								log::info!("Successfully loaded GGUF model");
								model = Some(w);
							}
							Err(e) => log::error!("Failed to parse GGUF model weights: {}", e),
						}
					}
					Err(e) => log::error!("Failed to read GGUF content: {}", e),
				},
				Err(e) => log::error!("Failed to open model file: {}", e),
			}
		} else {
			log::warn!("Coach model file does not exist");
		}

		// Initial synchronous Load event processing
		log::info!("Executing initial Load event rules...");
		self.handle_event(&CoachEvent::Load, model.as_mut(), tokenizer.as_mut());

		// Listen for events
		log::info!("Entering CoachWorker event loop");
		while let Ok(event) = rx.recv() {
			self.handle_event(&event, model.as_mut(), tokenizer.as_mut());
		}

		log::info!("Coach worker shutting down");
	}

	fn generate_text(
		&mut self,
		prompt: &str,
		_max_tokens: Option<usize>,
		model: &mut ModelWeights,
		tokenizer: &mut Tokenizer,
		save_history: bool,
	) -> String {
		let end_token_ids = Self::end_token_ids(tokenizer);

		let mut full_prompt = String::new();
		if let Some(sys) = &self.config.system_prompt {
			full_prompt.push_str(&format!("<|im_start|>system\n{}<|im_end|>\n", sys));
		}
		for msg in &self.history {
			full_prompt.push_str(&msg.to_chatml());
		}
		full_prompt.push_str(&format!(
			"<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
			prompt
		));

		let prompt_tokens = tokenizer
			.encode(full_prompt, false)
			.unwrap()
			.get_ids()
			.to_vec();

		let mut response_text = String::new();
		let max_new_tokens = 100;
		let mut generated_tokens = 0;
		let mut output_tokens = Vec::new();
		let mut index_pos = 0;
		let mut current_tokens = prompt_tokens.clone();

		while generated_tokens < max_new_tokens {
			let input = Tensor::new(current_tokens.as_slice(), &self.device)
				.unwrap()
				.unsqueeze(0)
				.unwrap();
			match model.forward(&input, index_pos) {
				Ok(logits) => {
					let seq_len = current_tokens.len();
					let logits_vec = logits.squeeze(0).unwrap().to_vec1::<f32>().unwrap();

					let mut next_token = 0;
					let mut max_prob = f32::NEG_INFINITY;
					for (i, &p) in logits_vec.iter().enumerate() {
						if p > max_prob {
							max_prob = p;
							next_token = i as u32;
						}
					}

					if max_prob == f32::NEG_INFINITY {
						log::error!(
							"Model generated NaNs! Hardware acceleration/correct CPU features may be required."
						);
						break;
					}

					index_pos += seq_len;
					current_tokens = vec![next_token];
					output_tokens.push(next_token);
					generated_tokens += 1;

					if end_token_ids.iter().any(|&id| id == next_token) {
						break;
					}
				}
				Err(e) => {
					log::error!("Inference error: {}", e);
					break;
				}
			}
		}

		if save_history {
			self.history.push(Message {
				role: "user".to_string(),
				content: prompt.to_string(),
			});
			self.history.push(Message {
				role: "assistant".to_string(),
				content: response_text.clone(),
			});

			if self.history.len() > 10 {
				self.history.drain(0..(self.history.len() - 10));
			}
		}

		if response_text.is_empty() {
			if let Ok(decoded) = tokenizer.decode(&output_tokens, false) {
				response_text = decoded;
			}
		}

		if response_text.ends_with("<|im_end|>") {
			response_text = response_text.replace("<|im_end|>", "").trim().to_string();
		}

		response_text
	}

	fn handle_event(
		&mut self,
		event: &CoachEvent,
		mut model: Option<&mut ModelWeights>,
		mut tokenizer: Option<&mut Tokenizer>,
	) {
		let event_str = event.as_str();
		let mut triggers = Vec::new();

		// Find matching rules
		for rule in &self.config.rules {
			if rule.on_event == event_str {
				// Evaluate conditions
				let mut conditions_met = true;
				if let Some(conditions) = &rule.conditions {
					for cond in conditions {
						match cond {
							Condition::Equal { key, value } => {
								if self.state.get_number(key) != *value {
									conditions_met = false;
									break;
								}
							}
							Condition::NotEqual { key, value } => {
								if self.state.get_number(key) == *value {
									conditions_met = false;
									break;
								}
							}
							Condition::Greater { key, value } => {
								if self.state.get_number(key) <= *value {
									conditions_met = false;
									break;
								}
							}
							Condition::GreaterOrEqual { key, value } => {
								if self.state.get_number(key) < *value {
									conditions_met = false;
									break;
								}
							}
							Condition::Less { key, value } => {
								if self.state.get_number(key) >= *value {
									conditions_met = false;
									break;
								}
							}
							Condition::LessOrEqual { key, value } => {
								if self.state.get_number(key) > *value {
									conditions_met = false;
									break;
								}
							}
						}
					}
				}

				if conditions_met {
					triggers.extend(rule.actions.clone());
				}
			}
		}

		// Execute actions
		for action in triggers {
			match action {
				Action::SetValue { key, value } => {
					self.state.set(key, value);
				}
				Action::IncreaseValue { key, amount } => {
					self.state.increase(key, amount);
				}
				Action::IncreaseValueByValue { target_key, by_key } => {
					let amount = self.state.get_number(&by_key);
					self.state.increase(target_key, amount);
				}
				Action::SetState { key, value } => {
					self.state.set(key, value);
				}
				Action::EmitMessage {
					prompt_template,
					max_tokens,
				} => {
					// Interpolate template
					let mut prompt = prompt_template.clone();
					if let Some(limit) = max_tokens {
						if limit > 0 {
							let word_limit = limit * 3 / 4;
							prompt.push_str(&format!(
								" (Important: Keep your response short, around {} words maximum. Do not cut off abruptly.)",
								word_limit
							));
						}
					}
					for (k, v) in &self.state.variables {
						let placeholder = format!("{{{}}}", k);
						prompt = prompt.replace(&placeholder, &v.to_string());
					}

					log::info!("Coach triggered prompt: {}", prompt);
					if max_tokens == Some(0) {
						let response = format!("(Coach): {}", prompt);
						let _ = self.tx.send(response);
					} else if let (Some(m), Some(tok)) =
						(model.as_deref_mut(), tokenizer.as_deref_mut())
					{
						let response_text = self.generate_text(&prompt, max_tokens, m, tok, true);
						let response = format!("(Coach): {}", response_text);
						let _ = self.tx.send(response);
					} else {
						let response = format!("(Coach VM): Generated response to '{}'", prompt);
						let _ = self.tx.send(response);
					}
				}
				Action::StoreMessage {
					prompt_template,
					max_tokens,
					store_at,
				} => {
					let mut prompt = prompt_template.clone();
					if let Some(limit) = max_tokens {
						if limit > 0 {
							let word_limit = limit * 3 / 4;
							prompt.push_str(&format!(
								" (Important: Keep your response short, around {} words maximum. Do not cut off abruptly.)",
								word_limit
							));
						}
					}
					for (k, v) in &self.state.variables {
						let placeholder = format!("{{{}}}", k);
						prompt = prompt.replace(&placeholder, &v.to_string());
					}

					log::info!("Coach triggered StoreMessage prompt: {}", prompt);
					if max_tokens == Some(0) {
						self.state.set(store_at.clone(), CoachValue::String(prompt));
					} else if let (Some(m), Some(tok)) =
						(model.as_deref_mut(), tokenizer.as_deref_mut())
					{
						let response_text = self.generate_text(&prompt, max_tokens, m, tok, false);
						self.state
							.set(store_at.clone(), CoachValue::String(response_text));
					} else {
						log::error!("Failed to generate response for StoreMessage action");
					}
				}
			}
		}
	}

	fn end_token_ids(tokenizer: &Tokenizer) -> Vec<u32> {
		let mut ids = Vec::new();
		for token in ["<|im_end|>", "</s>", "<eos>"] {
			if let Some(id) = tokenizer.token_to_id(token) {
				ids.push(id);
			}
		}
		ids
	}
}
