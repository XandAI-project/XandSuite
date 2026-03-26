/// Local LLM inference engine using llama.cpp
///
/// Two implementations are provided:
/// 1. Real inference via `llama-cpp-2` crate (requires cmake + C++ toolchain)
///    - Enable with: cargo build --features local-llm
/// 2. Stub (default) for development builds without C++ toolchain

use anyhow::{Context, Result};
use std::path::Path;
use tokio::sync::mpsc;

use crate::models::InferenceConfig;

pub struct LocalEngine {
    model_path: String,
    #[cfg(feature = "local-llm")]
    inner: RealLocalEngine,
}

#[cfg(feature = "local-llm")]
struct RealLocalEngine {
    backend: llama_cpp_2::llama_backend::LlamaBackend,
    model: llama_cpp_2::model::LlamaModel,
}

impl LocalEngine {
    pub fn new(model_path: String) -> Result<Self> {
        if !Path::new(&model_path).exists() {
            anyhow::bail!("Model file not found: {}", model_path);
        }

        #[cfg(feature = "local-llm")]
        {
            use llama_cpp_2::{
                llama_backend::LlamaBackend,
                model::{params::LlamaModelParams, LlamaModel},
            };

            let backend = LlamaBackend::init().context("Failed to initialize llama backend")?;
            let model_params = LlamaModelParams::default();
            let model = LlamaModel::load_from_file(&backend, Path::new(&model_path), &model_params)
                .context("Failed to load model")?;

            return Ok(Self {
                model_path,
                inner: RealLocalEngine { backend, model },
            });
        }

        #[cfg(not(feature = "local-llm"))]
        Ok(Self { model_path })
    }

    pub fn validate(&self) -> Result<()> {
        let metadata = std::fs::metadata(&self.model_path)
            .context("Cannot access model file")?;
        if metadata.len() == 0 {
            anyhow::bail!("Model file is empty");
        }
        Ok(())
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<(String, String)>,
        config: &InferenceConfig,
        tx: mpsc::Sender<String>,
    ) -> Result<()> {
        #[cfg(feature = "local-llm")]
        {
            return self.real_chat_stream(messages, config, tx).await;
        }

        #[cfg(not(feature = "local-llm"))]
        {
            self.stub_chat_stream(messages, config, tx).await
        }
    }

    #[cfg(feature = "local-llm")]
    async fn real_chat_stream(
        &self,
        messages: Vec<(String, String)>,
        config: &InferenceConfig,
        tx: mpsc::Sender<String>,
    ) -> Result<()> {
        use llama_cpp_2::{
            context::params::LlamaContextParams,
            llama_batch::LlamaBatch,
            model::Special,
            sampling::{params::LlamaSamplerChainParams, LlamaSampler},
        };

        // Build chat prompt using the model's chat template, or fall back to a simple format
        let prompt = self.build_prompt(&messages);

        let n_ctx = config.context_length.unwrap_or(4096) as u32;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(n_ctx))
            .with_n_threads(crate::engine::config::num_cpus() as u32);

        let mut ctx = self.inner.model
            .new_context(&self.inner.backend, ctx_params)
            .context("Failed to create llama context")?;

        // Tokenize prompt
        let tokens = self.inner.model
            .str_to_token(&prompt, Special::Tokenize)
            .context("Failed to tokenize prompt")?;

        let n_tokens = tokens.len();
        let max_new = config.max_tokens.unwrap_or(512) as usize;

        // Create initial batch
        let mut batch = LlamaBatch::new(n_tokens, 1);
        for (i, token) in tokens.iter().enumerate() {
            batch.add(*token, i as i32, &[0], i == n_tokens - 1)?;
        }
        ctx.decode(&mut batch).context("Decode failed")?;

        // Set up sampler chain
        let sampler_params = LlamaSamplerChainParams::default();
        let mut sampler = LlamaSampler::new(sampler_params)?;
        sampler.add_dist(config.seed.unwrap_or(42) as u32);
        sampler.add_top_p(config.top_p.unwrap_or(0.95) as f32, 1);
        sampler.add_temp(config.temperature.unwrap_or(0.7) as f32);

        let eos_token = self.inner.model.token_eos();
        let mut n_cur = n_tokens as i32;

        for _ in 0..max_new {
            let token = sampler.sample(&ctx, n_tokens as i32 - 1);
            sampler.accept(token);

            if token == eos_token {
                break;
            }

            let token_str = self.inner.model
                .token_to_str(token, Special::Tokenize)
                .unwrap_or_default();

            if tx.send(token_str).await.is_err() {
                break;
            }

            batch.clear();
            batch.add(token, n_cur, &[0], true)?;
            ctx.decode(&mut batch).context("Decode failed on new token")?;
            n_cur += 1;
        }

        let _ = tx.send("[DONE]".to_string()).await;
        Ok(())
    }

    #[cfg(feature = "local-llm")]
    fn build_prompt(&self, messages: &[(String, String)]) -> String {
        // Use ChatML format (works with most modern models)
        let mut prompt = String::new();
        for (role, content) in messages {
            prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", role, content));
        }
        prompt.push_str("<|im_start|>assistant\n");
        prompt
    }

    /// Stub implementation returned when `local-llm` feature is not enabled
    async fn stub_chat_stream(
        &self,
        _messages: Vec<(String, String)>,
        _config: &InferenceConfig,
        tx: mpsc::Sender<String>,
    ) -> Result<()> {
        let model_name = Path::new(&self.model_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let stub_msg = format!(
            "⚠️ Local inference stub: model '{}' is loaded but real inference requires \
            building with `--features local-llm` (needs cmake + C++ toolchain). \
            You can use the remote server option for immediate chat functionality.",
            model_name
        );

        for word in stub_msg.split_whitespace() {
            let _ = tx.send(format!("{} ", word)).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        }
        let _ = tx.send("[DONE]".to_string()).await;
        Ok(())
    }

    pub fn model_path(&self) -> &str {
        &self.model_path
    }
}
