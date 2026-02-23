//! Local inference provider — runs AI models directly on your hardware.
//!
//! Uses [Candle](https://github.com/huggingface/candle) (Rust-native ML) to run
//! GGUF-quantized language models with zero internet, zero API keys, zero cost.
//!
//! Supported model families:
//! - **TinyLlama** (1.1B params, Q4_K_M ~670 MB) — great for RPi / edge
//! - **SmolLM2** (135M–1.7B params, Q4 ~80–950 MB) — smallest practical models
//! - **Phi-2 / Phi-3** (2.7B–3.8B params) — good quality on modest hardware
//! - **Llama 2/3** (7B+) — needs more RAM but great quality
//! - **Mistral / Qwen** — also supported via the Llama architecture
//!
//! # Example
//! ```bash
//! rustedclaw agent --local --model tinyllama
//! rustedclaw agent --local --model smollm:135m
//! rustedclaw agent --local --model /path/to/model.gguf
//! ```

use async_trait::async_trait;
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama as qlm;
use hf_hub::api::sync::Api;
use rustedclaw_core::error::ProviderError;
use rustedclaw_core::message::{Message, Role};
use rustedclaw_core::provider::{ProviderRequest, ProviderResponse, StreamChunk, Usage};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizers::Tokenizer;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// ── Well-known model aliases ───────────────────────────────────────────

/// Model presets — friendly aliases that resolve to HuggingFace repos + filenames.
struct ModelPreset {
    repo: &'static str,
    gguf_file: &'static str,
    tokenizer_repo: &'static str,
    chat_template: ChatTemplate,
}

/// Chat template format used to structure messages for the model.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
enum ChatTemplate {
    /// `<|system|>\n{content}</s>\n<|user|>\n{content}</s>\n<|assistant|>\n`
    TinyLlama,
    /// `<|im_start|>system\n{content}<|im_end|>\n<|im_start|>user\n{content}<|im_end|>\n<|im_start|>assistant\n`
    ChatML,
    /// `[INST] {content} [/INST]`
    Llama2,
    /// `<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n{content}<|eot_id|>`
    Llama3,
}

fn resolve_preset(alias: &str) -> Option<ModelPreset> {
    let alias_lower = alias.to_lowercase();
    match alias_lower.as_str() {
        "tinyllama" | "tiny-llama" | "tinyllama-1.1b" => Some(ModelPreset {
            repo: "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
            gguf_file: "tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf",
            tokenizer_repo: "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
            chat_template: ChatTemplate::TinyLlama,
        }),
        "smollm" | "smollm:135m" | "smollm-135m" => Some(ModelPreset {
            repo: "TheBloke/SmolLM-135M-Instruct-GGUF",
            gguf_file: "smollm-135m-instruct.Q4_K_M.gguf",
            tokenizer_repo: "HuggingFaceTB/SmolLM-135M-Instruct",
            chat_template: ChatTemplate::ChatML,
        }),
        "smollm:360m" | "smollm-360m" => Some(ModelPreset {
            repo: "TheBloke/SmolLM-360M-Instruct-GGUF",
            gguf_file: "smollm-360m-instruct.Q4_K_M.gguf",
            tokenizer_repo: "HuggingFaceTB/SmolLM-360M-Instruct",
            chat_template: ChatTemplate::ChatML,
        }),
        "smollm:1.7b" | "smollm-1.7b" => Some(ModelPreset {
            repo: "TheBloke/SmolLM-1.7B-Instruct-GGUF",
            gguf_file: "smollm-1.7b-instruct.Q4_K_M.gguf",
            tokenizer_repo: "HuggingFaceTB/SmolLM-1.7B-Instruct",
            chat_template: ChatTemplate::ChatML,
        }),
        "phi2" | "phi-2" => Some(ModelPreset {
            repo: "TheBloke/phi-2-GGUF",
            gguf_file: "phi-2.Q4_K_M.gguf",
            tokenizer_repo: "microsoft/phi-2",
            chat_template: ChatTemplate::ChatML,
        }),
        "qwen:0.5b" | "qwen-0.5b" | "qwen2-0.5b" => Some(ModelPreset {
            repo: "Qwen/Qwen2-0.5B-Instruct-GGUF",
            gguf_file: "qwen2-0_5b-instruct-q4_k_m.gguf",
            tokenizer_repo: "Qwen/Qwen2-0.5B-Instruct",
            chat_template: ChatTemplate::ChatML,
        }),
        "qwen:1.5b" | "qwen-1.5b" | "qwen2-1.5b" => Some(ModelPreset {
            repo: "Qwen/Qwen2-1.5B-Instruct-GGUF",
            gguf_file: "qwen2-1_5b-instruct-q4_k_m.gguf",
            tokenizer_repo: "Qwen/Qwen2-1.5B-Instruct",
            chat_template: ChatTemplate::ChatML,
        }),
        _ => None,
    }
}

// ── Local Provider ─────────────────────────────────────────────────────

/// A provider that runs GGUF-quantized language models locally via Candle.
///
/// Thread-safe: the model is behind a Mutex because Candle inference
/// is inherently single-threaded (CPU tensor ops).
pub struct LocalProvider {
    inner: Arc<Mutex<Option<LocalModelState>>>,
    model_name: String,
}

/// The loaded model state (tokenizer + weights + config).
struct LocalModelState {
    model: qlm::ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    chat_template: ChatTemplate,
    eos_token_id: u32,
}

impl LocalProvider {
    /// Create a new local provider.
    ///
    /// `model_name` can be:
    /// - A preset alias: `"tinyllama"`, `"smollm:135m"`, `"phi2"`
    /// - A path to a local GGUF file: `"/path/to/model.gguf"`
    ///
    /// The model is loaded lazily on first request.
    pub fn new(model_name: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            model_name: model_name.to_string(),
        }
    }

    /// Eagerly load the model (downloads if needed, then loads into memory).
    pub fn load(model_name: &str) -> Result<Self, ProviderError> {
        let state = LocalModelState::load(model_name)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Some(state))),
            model_name: model_name.to_string(),
        })
    }

    /// Get the cache directory for downloaded models.
    #[allow(dead_code)]
    fn cache_dir() -> PathBuf {
        dirs_cache().join("rustedclaw").join("models")
    }
}

/// Platform-appropriate cache directory.
#[allow(dead_code)]
fn dirs_cache() -> PathBuf {
    if let Ok(dir) = std::env::var("RUSTEDCLAW_MODEL_CACHE") {
        return PathBuf::from(dir);
    }
    // Use HF Hub's default cache location
    if let Ok(dir) = std::env::var("HF_HOME") {
        return PathBuf::from(dir);
    }
    // Fallback: ~/.cache on Linux/Mac, %LOCALAPPDATA% on Windows
    #[cfg(windows)]
    {
        if let Ok(dir) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(dir).join("rustedclaw").join("models");
        }
    }
    dirs_fallback()
}

#[allow(dead_code)]
fn dirs_fallback() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".cache")
        .join("rustedclaw")
        .join("models")
}

impl LocalModelState {
    /// Load a model by name or path.
    fn load(model_name: &str) -> Result<Self, ProviderError> {
        let device = Device::Cpu;

        // Check if it's a local file path
        if Path::new(model_name).exists() && model_name.ends_with(".gguf") {
            return Self::load_from_path(Path::new(model_name), model_name, &device);
        }

        // Resolve preset alias
        let preset = resolve_preset(model_name).ok_or_else(|| {
            ProviderError::ModelNotFound(format!(
                "Unknown local model '{}'. Available presets: tinyllama, smollm, smollm:135m, \
                 smollm:360m, smollm:1.7b, phi2, qwen:0.5b, qwen:1.5b. \
                 Or provide a path to a .gguf file.",
                model_name
            ))
        })?;

        info!(
            model = model_name,
            repo = preset.repo,
            file = preset.gguf_file,
            "Downloading/loading local model"
        );

        // Download via HuggingFace Hub (cached automatically)
        let api = Api::new().map_err(|e| {
            ProviderError::Network(format!("Failed to initialize HuggingFace Hub API: {e}"))
        })?;

        let repo = api.model(preset.repo.to_string());

        let model_path = repo.get(preset.gguf_file).map_err(|e| {
            ProviderError::Network(format!(
                "Failed to download model '{}' from '{}': {e}",
                preset.gguf_file, preset.repo
            ))
        })?;

        info!(path = %model_path.display(), "Model file ready");

        // Download tokenizer
        let tokenizer_repo = api.model(preset.tokenizer_repo.to_string());
        let tokenizer_path = tokenizer_repo.get("tokenizer.json").map_err(|e| {
            ProviderError::Network(format!(
                "Failed to download tokenizer from '{}': {e}",
                preset.tokenizer_repo
            ))
        })?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| ProviderError::NotConfigured(format!("Failed to load tokenizer: {e}")))?;

        // Load GGUF model
        let mut file = std::fs::File::open(&model_path)
            .map_err(|e| ProviderError::NotConfigured(format!("Failed to open model file: {e}")))?;

        let gguf = gguf_file::Content::read(&mut file)
            .map_err(|e| ProviderError::NotConfigured(format!("Failed to parse GGUF file: {e}")))?;

        let model = qlm::ModelWeights::from_gguf(gguf, &mut file, &device).map_err(|e| {
            ProviderError::NotConfigured(format!("Failed to load model weights: {e}"))
        })?;

        // Determine EOS token
        let eos_token_id = tokenizer
            .token_to_id("</s>")
            .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
            .or_else(|| tokenizer.token_to_id("<|im_end|>"))
            .or_else(|| tokenizer.token_to_id("<|eot_id|>"))
            .unwrap_or(2); // fallback to common EOS id

        info!(
            eos_token_id = eos_token_id,
            "Local model loaded successfully"
        );

        Ok(Self {
            model,
            tokenizer,
            device,
            chat_template: preset.chat_template,
            eos_token_id,
        })
    }

    /// Load from an explicit GGUF file path.
    fn load_from_path(
        path: &Path,
        _model_name: &str,
        device: &Device,
    ) -> Result<Self, ProviderError> {
        info!(path = %path.display(), "Loading local GGUF model");

        let mut file = std::fs::File::open(path)
            .map_err(|e| ProviderError::NotConfigured(format!("Failed to open GGUF file: {e}")))?;

        let gguf = gguf_file::Content::read(&mut file)
            .map_err(|e| ProviderError::NotConfigured(format!("Failed to parse GGUF file: {e}")))?;

        let model = qlm::ModelWeights::from_gguf(gguf, &mut file, device).map_err(|e| {
            ProviderError::NotConfigured(format!("Failed to load model weights: {e}"))
        })?;

        // Try to find tokenizer.json next to the GGUF file
        let tokenizer_path = path.with_file_name("tokenizer.json");
        let tokenizer = if tokenizer_path.exists() {
            Tokenizer::from_file(&tokenizer_path).map_err(|e| {
                ProviderError::NotConfigured(format!("Failed to load tokenizer: {e}"))
            })?
        } else {
            // Fall back to a basic tokenizer download
            warn!(
                "No tokenizer.json found next to GGUF file, \
                 attempting to download TinyLlama tokenizer as fallback"
            );
            let api = Api::new()
                .map_err(|e| ProviderError::Network(format!("HuggingFace Hub API error: {e}")))?;
            let repo = api.model("TinyLlama/TinyLlama-1.1B-Chat-v1.0".to_string());
            let tok_path = repo.get("tokenizer.json").map_err(|e| {
                ProviderError::Network(format!("Failed to download fallback tokenizer: {e}"))
            })?;
            Tokenizer::from_file(&tok_path).map_err(|e| {
                ProviderError::NotConfigured(format!("Failed to load tokenizer: {e}"))
            })?
        };

        let eos_token_id = tokenizer
            .token_to_id("</s>")
            .or_else(|| tokenizer.token_to_id("<|endoftext|>"))
            .or_else(|| tokenizer.token_to_id("<|im_end|>"))
            .unwrap_or(2);

        Ok(Self {
            model,
            tokenizer,
            device: device.clone(),
            chat_template: ChatTemplate::ChatML,
            eos_token_id,
        })
    }

    /// Format messages using the model's chat template.
    fn format_prompt(&self, messages: &[Message]) -> String {
        match self.chat_template {
            ChatTemplate::TinyLlama => Self::format_tinyllama(messages),
            ChatTemplate::ChatML => Self::format_chatml(messages),
            ChatTemplate::Llama2 => Self::format_llama2(messages),
            ChatTemplate::Llama3 => Self::format_llama3(messages),
        }
    }

    fn format_tinyllama(messages: &[Message]) -> String {
        let mut prompt = String::new();
        for msg in messages {
            match msg.role {
                Role::System => {
                    prompt.push_str("<|system|>\n");
                    prompt.push_str(&msg.content);
                    prompt.push_str("</s>\n");
                }
                Role::User => {
                    prompt.push_str("<|user|>\n");
                    prompt.push_str(&msg.content);
                    prompt.push_str("</s>\n");
                }
                Role::Assistant => {
                    prompt.push_str("<|assistant|>\n");
                    prompt.push_str(&msg.content);
                    prompt.push_str("</s>\n");
                }
                Role::Tool => {
                    prompt.push_str("<|user|>\n[Tool Result] ");
                    prompt.push_str(&msg.content);
                    prompt.push_str("</s>\n");
                }
            }
        }
        prompt.push_str("<|assistant|>\n");
        prompt
    }

    fn format_chatml(messages: &[Message]) -> String {
        let mut prompt = String::new();
        for msg in messages {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "user", // tool results become user messages
            };
            prompt.push_str("<|im_start|>");
            prompt.push_str(role);
            prompt.push('\n');
            if msg.role == Role::Tool {
                prompt.push_str("[Tool Result] ");
            }
            prompt.push_str(&msg.content);
            prompt.push_str("<|im_end|>\n");
        }
        prompt.push_str("<|im_start|>assistant\n");
        prompt
    }

    fn format_llama2(messages: &[Message]) -> String {
        let mut prompt = String::new();
        let mut system_prompt = String::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    system_prompt = msg.content.clone();
                }
                Role::User => {
                    prompt.push_str("[INST] ");
                    if !system_prompt.is_empty() {
                        prompt.push_str("<<SYS>>\n");
                        prompt.push_str(&system_prompt);
                        prompt.push_str("\n<</SYS>>\n\n");
                        system_prompt.clear();
                    }
                    prompt.push_str(&msg.content);
                    prompt.push_str(" [/INST]");
                }
                Role::Assistant => {
                    prompt.push(' ');
                    prompt.push_str(&msg.content);
                    prompt.push_str(" </s>");
                }
                Role::Tool => {
                    prompt.push_str("[INST] [Tool Result] ");
                    prompt.push_str(&msg.content);
                    prompt.push_str(" [/INST]");
                }
            }
        }
        prompt
    }

    fn format_llama3(messages: &[Message]) -> String {
        let mut prompt = String::from("<|begin_of_text|>");
        for msg in messages {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "user",
            };
            prompt.push_str("<|start_header_id|>");
            prompt.push_str(role);
            prompt.push_str("<|end_header_id|>\n\n");
            if msg.role == Role::Tool {
                prompt.push_str("[Tool Result] ");
            }
            prompt.push_str(&msg.content);
            prompt.push_str("<|eot_id|>");
        }
        prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
        prompt
    }

    /// Run inference: tokenize → generate tokens → decode.
    fn generate(
        &mut self,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<(String, u32, u32), ProviderError> {
        let encoding =
            self.tokenizer
                .encode(prompt, true)
                .map_err(|e| ProviderError::ApiError {
                    status_code: 500,
                    message: format!("Tokenization failed: {e}"),
                })?;

        let prompt_tokens = encoding.get_ids();
        let prompt_token_count = prompt_tokens.len() as u32;

        debug!(
            prompt_tokens = prompt_token_count,
            max_tokens = max_tokens,
            temperature = temperature,
            "Starting local generation"
        );

        let mut input_ids = Tensor::new(prompt_tokens, &self.device).map_err(map_candle_err)?;
        input_ids = input_ids.unsqueeze(0).map_err(map_candle_err)?;

        let mut logits_processor = if temperature <= 0.0 {
            LogitsProcessor::new(42, None, None)
        } else {
            LogitsProcessor::new(42, Some(temperature as f64), None)
        };

        let mut generated_tokens: Vec<u32> = Vec::new();
        let mut next_token_tensor = input_ids;

        for _ in 0..max_tokens {
            let logits = self
                .model
                .forward(&next_token_tensor, generated_tokens.len())
                .map_err(map_candle_err)?;

            // Get logits for the last position
            let logits = logits.squeeze(0).map_err(map_candle_err)?;
            let logits = logits
                .get(logits.dim(0).map_err(map_candle_err)? - 1)
                .map_err(map_candle_err)?;

            let next_token = logits_processor.sample(&logits).map_err(map_candle_err)?;

            // Check for EOS
            if next_token == self.eos_token_id {
                break;
            }

            generated_tokens.push(next_token);

            // Prepare input for next iteration (just the new token)
            next_token_tensor = Tensor::new(&[next_token][..], &self.device)
                .map_err(map_candle_err)?
                .unsqueeze(0)
                .map_err(map_candle_err)?;
        }

        let completion_token_count = generated_tokens.len() as u32;

        // Decode generated tokens
        let output = self
            .tokenizer
            .decode(&generated_tokens, true)
            .map_err(|e| ProviderError::ApiError {
                status_code: 500,
                message: format!("Detokenization failed: {e}"),
            })?;

        debug!(
            completion_tokens = completion_token_count,
            output_len = output.len(),
            "Generation complete"
        );

        Ok((output, prompt_token_count, completion_token_count))
    }
}

/// Map Candle errors to ProviderError.
fn map_candle_err(e: candle_core::Error) -> ProviderError {
    ProviderError::ApiError {
        status_code: 500,
        message: format!("Candle inference error: {e}"),
    }
}

// ── Provider trait implementation ──────────────────────────────────────

#[async_trait]
impl rustedclaw_core::provider::Provider for LocalProvider {
    fn name(&self) -> &str {
        "local"
    }

    async fn complete(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<ProviderResponse, ProviderError> {
        let model_name = self.model_name.clone();

        // Ensure model is loaded (lazy initialization)
        {
            let state = self.inner.lock().await;
            if state.is_none() {
                drop(state);
                info!(model = %model_name, "Loading local model on first request...");
                let name_clone = model_name.clone();
                let loaded =
                    tokio::task::spawn_blocking(move || LocalModelState::load(&name_clone))
                        .await
                        .map_err(|e| ProviderError::ApiError {
                            status_code: 500,
                            message: format!("Model loading task failed: {e}"),
                        })??;

                let mut state = self.inner.lock().await;
                *state = Some(loaded);
            }
        }

        let max_tokens = request.max_tokens.unwrap_or(512);
        let temperature = request.temperature;
        let messages = request.messages.clone();
        let model_label = request.model.clone();

        // Run inference on a blocking thread (Candle is CPU-bound)
        let inner = self.inner.clone();
        let (output, prompt_tokens, completion_tokens) = tokio::task::spawn_blocking(move || {
            let mut guard = inner.blocking_lock();
            let state = guard.as_mut().expect("model must be loaded");
            let prompt = state.format_prompt(&messages);
            state.generate(&prompt, max_tokens, temperature)
        })
        .await
        .map_err(|e| ProviderError::ApiError {
            status_code: 500,
            message: format!("Inference task panicked: {e}"),
        })??;

        // Clean up the output (remove any trailing special tokens)
        let clean_output = output
            .trim()
            .trim_end_matches("</s>")
            .trim_end_matches("<|im_end|>")
            .trim_end_matches("<|eot_id|>")
            .trim()
            .to_string();

        Ok(ProviderResponse {
            message: Message::assistant(&clean_output),
            usage: Some(Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            }),
            model: format!("local/{}", model_label),
            metadata: {
                let mut meta = serde_json::Map::new();
                meta.insert("provider".into(), serde_json::Value::String("local".into()));
                meta.insert("engine".into(), serde_json::Value::String("candle".into()));
                meta.insert(
                    "quantization".into(),
                    serde_json::Value::String("GGUF/Q4_K_M".into()),
                );
                meta
            },
        })
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<
        tokio::sync::mpsc::Receiver<std::result::Result<StreamChunk, ProviderError>>,
        ProviderError,
    > {
        // For local models, we do token-by-token streaming
        // For now, fall back to the default (complete → single chunk)
        // TODO: implement real token-by-token streaming with Candle
        let response = self.complete(request).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let _ = tx
            .send(Ok(StreamChunk {
                content: Some(response.message.content),
                tool_calls: response.message.tool_calls,
                done: true,
                usage: response.usage,
            }))
            .await;
        Ok(rx)
    }

    async fn list_models(&self) -> std::result::Result<Vec<String>, ProviderError> {
        Ok(vec![
            "tinyllama".into(),
            "smollm".into(),
            "smollm:135m".into(),
            "smollm:360m".into(),
            "smollm:1.7b".into(),
            "phi2".into(),
            "qwen:0.5b".into(),
            "qwen:1.5b".into(),
        ])
    }

    async fn health_check(&self) -> std::result::Result<bool, ProviderError> {
        // Local provider is always available (no network needed)
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_preset_aliases() {
        assert!(resolve_preset("tinyllama").is_some());
        assert!(resolve_preset("TinyLlama").is_some());
        assert!(resolve_preset("smollm:135m").is_some());
        assert!(resolve_preset("phi2").is_some());
        assert!(resolve_preset("qwen:0.5b").is_some());
        assert!(resolve_preset("nonexistent").is_none());
    }

    #[test]
    fn chat_template_tinyllama() {
        let messages = vec![Message::system("You are helpful."), Message::user("Hello!")];
        let prompt = LocalModelState::format_tinyllama(&messages);
        assert!(prompt.contains("<|system|>"));
        assert!(prompt.contains("You are helpful."));
        assert!(prompt.contains("<|user|>"));
        assert!(prompt.contains("Hello!"));
        assert!(prompt.ends_with("<|assistant|>\n"));
    }

    #[test]
    fn chat_template_chatml() {
        let messages = vec![Message::system("You are helpful."), Message::user("Hi")];
        let prompt = LocalModelState::format_chatml(&messages);
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("<|im_start|>user"));
        assert!(prompt.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn chat_template_llama2() {
        let messages = vec![Message::system("Be helpful."), Message::user("Question")];
        let prompt = LocalModelState::format_llama2(&messages);
        assert!(prompt.contains("<<SYS>>"));
        assert!(prompt.contains("Be helpful."));
        assert!(prompt.contains("[INST]"));
        assert!(prompt.contains("Question"));
    }

    #[test]
    fn chat_template_llama3() {
        let messages = vec![Message::user("Hello")];
        let prompt = LocalModelState::format_llama3(&messages);
        assert!(prompt.contains("<|begin_of_text|>"));
        assert!(prompt.contains("<|start_header_id|>user<|end_header_id|>"));
        assert!(prompt.contains("Hello"));
    }
}
