use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_phi::ModelWeights;
use serde::Deserialize;
use std::collections::HashMap;
use tokenizers::Tokenizer;

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
	pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
	IncreaseValue { key: String, amount: i32 },
	SetState { key: String, value: i32 },
	EmitMessage { prompt_template: String },
}

pub struct CoachState {
	pub variables: HashMap<String, i32>,
}

impl CoachState {
	pub fn new() -> Self {
		Self {
			variables: HashMap::new(),
		}
	}

	pub fn get(&self, key: &str) -> i32 {
		self.variables.get(key).copied().unwrap_or(0)
	}

	pub fn set(&mut self, key: String, val: i32) {
		self.variables.insert(key, val);
	}

	pub fn increase(&mut self, key: String, amount: i32) {
		let cur = self.get(&key);
		self.set(key, cur + amount);
	}
}

pub enum CoachEvent {
	NextImage,
	PrevImage,
	PhaseChange(String),
}

impl CoachEvent {
	pub fn as_str(&self) -> &str {
		match self {
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
		let config: CoachConfig = toml::from_str(&config_str).unwrap_or_else(|_| CoachConfig {
			system_prompt: None,
			rules: vec![],
		});

		Self {
			model_path,
			config,
			state: CoachState::new(),
			history: Vec::new(),
			tx,
		}
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
						match ModelWeights::from_gguf(content, &mut file, &Device::Cpu) {
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

		// Listen for events
		while let Ok(event) = rx.recv() {
			self.handle_event(&event, model.as_mut(), tokenizer.as_mut());
		}

		log::info!("Coach worker shutting down");
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
				triggers.extend(rule.actions.clone());
			}
		}

		// Execute actions
		for action in triggers {
			match action {
				Action::IncreaseValue { key, amount } => {
					self.state.increase(key, amount);
				}
				Action::SetState { key, value } => {
					self.state.set(key, value);
				}
				Action::EmitMessage { prompt_template } => {
					// Interpolate template
					let mut prompt = prompt_template.clone();
					for (k, v) in &self.state.variables {
						let placeholder = format!("{{{}}}", k);
						prompt = prompt.replace(&placeholder, &v.to_string());
					}

					log::info!("Coach triggered prompt: {}", prompt);
					if let (Some(m), Some(tok)) = (model.as_deref_mut(), tokenizer.as_deref_mut()) {
						let mut full_prompt = String::new();
						if let Some(sys) = &self.config.system_prompt {
							full_prompt
								.push_str(&format!("<|im_start|>system\n{}<|im_end|>\n", sys));
						}
						for msg in &self.history {
							full_prompt.push_str(&msg.to_chatml());
						}
						full_prompt.push_str(&format!(
							"<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
							prompt
						));

						let prompt_tokens =
							tok.encode(full_prompt, false).unwrap().get_ids().to_vec();

						let mut response_text = String::new();
						let max_new_tokens = 100;
						let mut generated_tokens = 0;
						let mut index_pos = 0;
						let mut current_tokens = prompt_tokens.clone();

						while generated_tokens < max_new_tokens {
							log::info!("Coach generating token {}", generated_tokens);
							let input = Tensor::new(current_tokens.as_slice(), &Device::Cpu)
								.unwrap()
								.unsqueeze(0)
								.unwrap();
							match m.forward(&input, index_pos) {
								Ok(logits) => {
									let seq_len = current_tokens.len();

									// `forward` already returns logits for the last position only: shape (batch, vocab).
									// We just need the vocab dimension here.
									let logits_vec =
										logits.squeeze(0).unwrap().to_vec1::<f32>().unwrap();

									// Greedy argmax naive sampling
									let mut next_token = 0;
									let mut max_prob = f32::NEG_INFINITY;
									for (i, &p) in logits_vec.iter().enumerate() {
										if p > max_prob {
											max_prob = p;
											next_token = i as u32;
										}
									}

									index_pos += seq_len;
									current_tokens = vec![next_token];
									generated_tokens += 1;

									if let Some(s) = tok.decode(&[next_token], true).ok() {
										response_text.push_str(&s);
										if response_text.ends_with("<|im_end|>") {
											log::info!(
												"Coach generated full response: {}",
												response_text
											);
											response_text = response_text
												.replace("<|im_end|>", "")
												.trim()
												.to_string();
											break;
										}
									}
								}
								Err(e) => {
									log::error!("Inference error: {}", e);
									break;
								}
							}
						}

						log::info!("Coach finished generation loop");

						// Save to history
						self.history.push(Message {
							role: "user".to_string(),
							content: prompt.clone(),
						});
						self.history.push(Message {
							role: "assistant".to_string(),
							content: response_text.clone(),
						});

						// Trim history to prevent context explosion
						if self.history.len() > 10 {
							self.history.drain(0..(self.history.len() - 10));
						}

						let response = format!("(Coach): {}", response_text);
						let _ = self.tx.send(response);
					} else {
						let response = format!("(Coach VM): Generated response to '{}'", prompt);
						let _ = self.tx.send(response);
					}
				}
			}
		}
	}
}
