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

const NO_ENGINE_MSG: &str =
    "No model loaded. Please load a local model or connect to a remote server.";

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

    pub async fn connect_remote(
        &self,
        server_url: String,
        api_key: Option<String>,
        model_name: Option<String>,
    ) -> Result<()> {
        let engine = remote::RemoteEngine::new(server_url, api_key, model_name);
        // Await the lock directly instead of spawning a detached task. The old
        // spawn approach returned immediately, so a chat request sent right
        // after connecting could observe `engine == None` (a race). Awaiting
        // here guarantees the engine is installed before this call returns.
        let mut lock = self.engine.lock().await;
        *lock = Some(Engine::Remote(engine));
        Ok(())
    }

    pub async fn chat_stream(
        &self,
        messages: Vec<(String, String)>,
        config: InferenceConfig,
        tx: mpsc::Sender<String>,
    ) -> Result<()> {
        // Take a clone of the remote engine and release the mutex before
        // streaming. Holding it for the whole response serialised every other
        // engine reader (title summarisation, `get_remote`, status checks)
        // behind the stream.
        let remote = {
            let lock = self.engine.lock().await;
            match &*lock {
                Some(Engine::Remote(engine)) => Some(engine.clone()),
                Some(Engine::Local(_)) => None,
                None => anyhow::bail!(NO_ENGINE_MSG),
            }
        };

        if let Some(engine) = remote {
            return engine.chat_stream(messages, &config, tx).await;
        }

        // `LocalEngine` is not clonable, so in-process inference keeps the lock.
        let lock = self.engine.lock().await;
        match &*lock {
            Some(Engine::Local(engine)) => engine.chat_stream(messages, &config, tx).await,
            Some(Engine::Remote(engine)) => engine.chat_stream(messages, &config, tx).await,
            None => anyhow::bail!(NO_ENGINE_MSG),
        }
    }

    /// Return a clone of the inner RemoteEngine if one is active.
    /// Used by the agentic executor which needs direct access for tool-call completions.
    pub async fn get_remote(&self) -> Option<remote::RemoteEngine> {
        let lock = self.engine.lock().await;
        if let Some(Engine::Remote(re)) = &*lock {
            return Some(re.clone());
        }
        None
    }

    /// The base URL of the active remote engine, or `None` when no engine is
    /// loaded or an in-process local model is active. Callers use this to detect
    /// that the engine still points at a server the settings no longer name.
    pub async fn remote_url(&self) -> Option<String> {
        let lock = self.engine.lock().await;
        match &*lock {
            Some(Engine::Remote(re)) => Some(re.server_url().to_string()),
            _ => None,
        }
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
