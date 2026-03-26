use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_path: String,
    pub context_size: u32,
    pub gpu_layers: i32,
    pub use_mlock: bool,
    pub use_mmap: bool,
    pub seed: u64,
    pub threads: u32,
    pub batch_size: u32,
}

impl Default for ModelConfig {
    fn default() -> Self {
        let cpu_count = num_cpus();
        Self {
            model_path: String::new(),
            context_size: 4096,
            gpu_layers: 0,
            use_mlock: false,
            use_mmap: true,
            seed: 0,
            threads: cpu_count,
            batch_size: 512,
        }
    }
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}
