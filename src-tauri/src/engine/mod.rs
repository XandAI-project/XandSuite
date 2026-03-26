pub mod config;
pub mod local;
pub mod remote;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::models::InferenceConfig;

pub enum Engine {
    Local(local::LocalEngine),
    Remote(remote::RemoteEngine),
}

pub struct EngineManager {
    pub engine: Arc<Mutex<Option<Engine>>>,
}

impl EngineManager {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn load_local(&self, model_path: String) -> Result<()> {
        let engine = local::LocalEngine::new(model_path)?;
        engine.validate()?;
        let mut lock = self.engine.lock().await;
        *lock = Some(Engine::Local(engine));
        Ok(())
    }

    pub fn connect_remote(
        &self,
        server_url: String,
        api_key: Option<String>,
        model_name: Option<String>,
    ) -> Result<()> {
        let engine = remote::RemoteEngine::new(server_url, api_key, model_name);
        let engine_clone = self.engine.clone();
        tokio::spawn(async move {
            let mut lock = engine_clone.lock().await;
            *lock = Some(Engine::Remote(engine));
        });
        Ok(())
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<(String, String)>,
        config: InferenceConfig,
        tx: mpsc::Sender<String>,
    ) -> Result<()> {
        let lock = self.engine.lock().await;
        match &*lock {
            Some(Engine::Local(engine)) => {
                engine.chat_stream(messages, &config, tx).await
            }
            Some(Engine::Remote(engine)) => {
                engine.chat_stream(messages, &config, tx).await
            }
            None => {
                anyhow::bail!("No model loaded. Please load a local model or connect to a remote server.")
            }
        }
    }

    /// Return a clone of the inner RemoteEngine if one is active.
    /// Used by the agentic executor which needs direct access for tool-call completions.
    pub fn get_remote(&self) -> Option<remote::RemoteEngine> {
        // We need a synchronous peek — try_lock to avoid blocking the caller.
        if let Ok(lock) = self.engine.try_lock() {
            if let Some(Engine::Remote(re)) = &*lock {
                return Some(re.clone());
            }
        }
        None
    }

    pub async fn is_loaded(&self) -> bool {
        let lock = self.engine.lock().await;
        lock.is_some()
    }

    pub async fn unload(&self) {
        let mut lock = self.engine.lock().await;
        *lock = None;
    }
}

impl Default for EngineManager {
    fn default() -> Self {
        Self::new()
    }
}
