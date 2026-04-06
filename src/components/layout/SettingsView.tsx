import { useEffect, useState } from "react";
import { Save, Loader2, Download, CheckCircle2, Plus, Trash2, Pencil, ChevronDown, ChevronRight, Mic, Play, Square } from "lucide-react";
import { invoke } from "@/lib/tauri";
import { listen } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useServerStore } from "@/stores/serverStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { cn } from "@/lib/utils";
import type { AppSettings, ComfyWorkflow } from "@/lib/tauri";

export function SettingsView() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [dataDir, setDataDir] = useState("");

  const {
    status: serverStatus, gpuInfo, isDownloading, downloadProgress, error: serverError,
    fetchStatus, detectGpu, downloadBinary, listenToProgress,
  } = useServerStore();

  // Keep the global settings store in sync so other components (e.g. InputBar)
  // always reflect the latest saved values.
  const { fetchSettings: syncSettingsStore } = useSettingsStore();

  useEffect(() => {
    invoke<AppSettings>("get_settings").then(setSettings).catch(console.error);
    invoke<string>("get_data_dir").then(setDataDir).catch(console.error);
    fetchStatus();
    detectGpu();
    const unlistenPromise = listenToProgress();
    return () => { unlistenPromise.then((fn) => fn()); };
  }, []);

  const handleSave = async () => {
    if (!settings) return;
    setIsSaving(true);
    try {
      await invoke("save_settings", { settings });
      // Refresh the global store so InputBar and other consumers
      // see the new values (e.g. whisper_enabled) immediately.
      await syncSettingsStore();
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error(e);
    } finally {
      setIsSaving(false);
    }
  };

  const update = (key: keyof AppSettings, value: unknown) => {
    setSettings((s) => s ? { ...s, [key]: value } : null);
  };

  if (!settings) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="px-6 py-4 border-b border-border flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">Settings</h1>
          <p className="text-sm text-muted-foreground mt-1">Configure XandSuite preferences</p>
        </div>
        <Button onClick={handleSave} disabled={isSaving}>
          {isSaving ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : <Save className="w-4 h-4 mr-2" />}
          {saved ? "Saved!" : "Save Settings"}
        </Button>
      </div>

      <ScrollArea className="flex-1 px-6 py-6">
        <div className="max-w-xl space-y-8">
          {/* HuggingFace */}
          <Section title="HuggingFace">
            <Field label="API Token (for gated models)" description="Get your token at huggingface.co/settings/tokens">
              <Input
                type="password"
                placeholder="hf_..."
                value={settings.hf_api_token || ""}
                onChange={(e) => update("hf_api_token", e.target.value || null)}
              />
            </Field>
            <Field label="Auto-sync model catalog" description="Refresh available models every 24 hours">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.auto_sync_models}
                  onChange={(e) => update("auto_sync_models", e.target.checked)}
                />
                <span className="text-sm">Enable auto-sync</span>
              </label>
            </Field>
          </Section>

          {/* Model storage */}
          <Section title="Model Storage">
            <Field label="Models directory" description={`Currently stored in: ${dataDir}`}>
              <Input
                placeholder="models"
                value={settings.models_directory}
                onChange={(e) => update("models_directory", e.target.value)}
              />
            </Field>
            <Field label="Default engine mode">
              <select
                className="flex h-9 w-full rounded-md border border-input bg-background text-foreground px-3 py-1 text-sm [&>option]:bg-background [&>option]:text-foreground"
                value={settings.default_engine_mode}
                onChange={(e) => update("default_engine_mode", e.target.value)}
              >
                <option value="local">Local (llama.cpp)</option>
                <option value="remote">Remote server</option>
              </select>
            </Field>
          </Section>

          {/* Local server */}
          <Section title="Local Server (llama-server)">
            {/* Binary download */}
            <Field
              label="llama-server binary"
              description="The internal server that runs GGUF models locally. Download once, use forever."
            >
              <div className="space-y-3">
                {/* GPU detection card */}
                <div className="rounded-md border border-border bg-secondary/30 px-3 py-2 space-y-1">
                  {gpuInfo ? (
                    <>
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium">{gpuInfo.name || "Unknown GPU"}</span>
                        <span className="text-[10px] px-1.5 py-0.5 rounded bg-primary/20 text-primary font-mono">
                          {gpuInfo.recommended_variant === "cpu" ? "CPU" :
                           gpuInfo.recommended_variant === "cuda12" ? "CUDA 12" :
                           gpuInfo.recommended_variant === "cuda13" ? "CUDA 13" : "Vulkan"}
                          {" "}recommended
                        </span>
                      </div>
                      <p className="text-[11px] text-muted-foreground">{gpuInfo.reason}</p>
                      {gpuInfo.recommended_variant.startsWith("cuda") && (
                        <p className="text-[10px] text-muted-foreground/70 italic">
                          Note: llama.cpp CUDA builds require compatible NVIDIA drivers (≥ 525 for CUDA 12,
                          ≥ 570 for CUDA 13). CUDA 11 is no longer distributed — CUDA 12 supports all
                          RTX 20/30/40 series cards.
                        </p>
                      )}
                    </>
                  ) : (
                    <span className="text-xs text-muted-foreground flex items-center gap-1.5">
                      <Loader2 className="w-3 h-3 animate-spin" />
                      Detecting GPU…
                    </span>
                  )}
                </div>

                {/* Status + download buttons */}
                <div className="flex items-center gap-2">
                  {serverStatus.binary_exists ? (
                    <span className="flex items-center gap-1.5 text-xs text-emerald-400">
                      <CheckCircle2 className="w-3.5 h-3.5" />
                      Binary installed
                    </span>
                  ) : (
                    <span className="text-xs text-muted-foreground">Not installed</span>
                  )}
                </div>

                <div className="flex flex-wrap gap-2">
                  {([
                    { id: "cpu",    label: "CPU only",  subtitle: "Any hardware" },
                    { id: "cuda12", label: "CUDA 12",   subtitle: "RTX 20/30/40" },
                    { id: "cuda13", label: "CUDA 13",   subtitle: "RTX 50 / Blackwell" },
                    { id: "vulkan", label: "Vulkan",    subtitle: "AMD / Intel Arc" },
                  ] as const).map(({ id, label, subtitle }) => {
                    const isRecommended = gpuInfo?.recommended_variant === id;
                    return (
                      <Button
                        key={id}
                        size="sm"
                        variant={isRecommended ? "default" : "outline"}
                        disabled={isDownloading}
                        onClick={() => downloadBinary(id)}
                        className={cn("flex-col h-auto py-1.5 px-3 gap-0", isRecommended && "ring-1 ring-primary")}
                        title={isRecommended ? "Recommended for your GPU" : undefined}
                      >
                        <span className="flex items-center gap-1">
                          {isDownloading ? (
                            <Loader2 className="w-3 h-3 animate-spin" />
                          ) : (
                            <Download className="w-3 h-3" />
                          )}
                          {label}
                          {isRecommended && <span className="text-[9px] ml-0.5">★</span>}
                        </span>
                        <span className="text-[9px] opacity-60">{subtitle}</span>
                      </Button>
                    );
                  })}
                </div>

                {/* Download progress */}
                {isDownloading && (
                  <div className="space-y-0.5">
                    <Progress
                      className="h-1.5"
                      value={
                        downloadProgress?.total_bytes
                          ? (downloadProgress.downloaded_bytes / downloadProgress.total_bytes) * 100
                          : undefined
                      }
                    />
                    <p className="text-[10px] text-muted-foreground">
                      {downloadProgress
                        ? `${(downloadProgress.downloaded_bytes / 1024 / 1024).toFixed(1)} MB${downloadProgress.total_bytes ? ` / ${(downloadProgress.total_bytes / 1024 / 1024).toFixed(0)} MB` : ""}`
                        : "Contacting GitHub…"}
                    </p>
                  </div>
                )}

                {/* Error banner */}
                {serverError && !isDownloading && (
                  <p className="text-xs text-destructive break-all">
                    {serverError}
                  </p>
                )}
              </div>
            </Field>

            <Field label="Port" description="Port the internal server listens on (default: 11434)">
              <Input
                type="number"
                min={1024}
                max={65535}
                value={settings.llama_server_port}
                onChange={(e) => update("llama_server_port", parseInt(e.target.value) || 11434)}
              />
            </Field>

            <Field
              label={`GPU Layers: ${settings.n_gpu_layers === -1 ? "All (full GPU)" : settings.n_gpu_layers === 0 ? "0 (CPU only)" : settings.n_gpu_layers}`}
              description="-1 = all layers on GPU, 0 = CPU only, 1–N = partial offload"
            >
              <input
                type="range"
                min={-1}
                max={128}
                step={1}
                className="w-full accent-primary"
                value={settings.n_gpu_layers}
                onChange={(e) => update("n_gpu_layers", parseInt(e.target.value))}
              />
              <div className="flex justify-between text-[10px] text-muted-foreground mt-0.5">
                <span>CPU only (0)</span>
                <span>Partial</span>
                <span>Full GPU (-1)</span>
              </div>
            </Field>

            <Field label="CPU Threads" description="0 = auto-detect">
              <Input
                type="number"
                min={0}
                max={128}
                value={settings.server_threads}
                onChange={(e) => update("server_threads", parseInt(e.target.value) || 0)}
              />
            </Field>

            <Field label="Context Size (tokens)">
              <select
                className="flex h-9 w-full rounded-md border border-input bg-background text-foreground px-3 py-1 text-sm [&>option]:bg-background [&>option]:text-foreground"
                value={settings.server_context_size}
                onChange={(e) => update("server_context_size", parseInt(e.target.value))}
              >
                {[512, 1024, 2048, 4096, 8192, 16384, 32768].map((n) => (
                  <option key={n} value={n}>{n.toLocaleString()} tokens</option>
                ))}
              </select>
            </Field>

            <Field label="Batch Size" description="Prompt processing batch size">
              <Input
                type="number"
                min={64}
                max={4096}
                step={64}
                value={settings.server_batch_size}
                onChange={(e) => update("server_batch_size", parseInt(e.target.value) || 512)}
              />
            </Field>

            <Field
              label={`Keep model loaded: ${settings.model_keep_alive_mins === 0 ? "Forever" : `${settings.model_keep_alive_mins} min`}`}
              description="After this many minutes of inactivity the server stops automatically to free VRAM. Set to 0 to keep it running forever."
            >
              <input
                type="range"
                min={0}
                max={60}
                step={1}
                className="w-full accent-primary"
                value={settings.model_keep_alive_mins}
                onChange={(e) => update("model_keep_alive_mins", parseInt(e.target.value))}
              />
              <div className="flex justify-between text-[10px] text-muted-foreground mt-0.5">
                <span>Forever (0)</span>
                <span>5 min</span>
                <span>30 min</span>
                <span>1 hr</span>
              </div>
            </Field>

            <Field label="Flash Attention" description="Speeds up attention computation. Recommended for RTX 2000+ / RDNA3+. When off, llama-server uses standard attention.">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.flash_attention}
                  onChange={(e) => update("flash_attention", e.target.checked)}
                />
                <span className="text-sm">Enable flash attention <span className="text-muted-foreground text-xs">(--flash-attn on)</span></span>
              </label>
            </Field>

            <Field label="Memory Map (mmap)" description="Load model weights via mmap (recommended)">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.use_mmap}
                  onChange={(e) => update("use_mmap", e.target.checked)}
                />
                <span className="text-sm">Enable mmap</span>
              </label>
            </Field>
          </Section>

          {/* Reasoning */}
          <Section title="Reasoning / Chain-of-Thought">
            <Field
              label="Reasoning format"
              description="How llama-server parses <think> tokens. 'deepseek' puts reasoning in a separate field (works for Qwen3, DeepSeek-R1, and most thinking models). Requires server restart."
            >
              <select
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm"
                value={settings.reasoning_format}
                onChange={(e) => update("reasoning_format", e.target.value)}
              >
                <option value="none">none — disable (raw &lt;think&gt; tags stay in content)</option>
                <option value="deepseek">deepseek — separate reasoning_content field (recommended)</option>
                <option value="deepseek-legacy">deepseek-legacy — both content and reasoning_content</option>
                <option value="auto">auto — server decides based on model</option>
              </select>
            </Field>
            <Field label="Enable thinking in responses" description="Toggle chain-of-thought per conversation. Ignored on models without thinking support.">
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.enable_thinking}
                  onChange={(e) => update("enable_thinking", e.target.checked)}
                />
                <span className="text-sm">Enable thinking</span>
              </label>
            </Field>
            <Field
              label={`Thinking budget: ${settings.thinking_budget_tokens === 0 ? "Unlimited ⚠️" : `${settings.thinking_budget_tokens.toLocaleString()} tokens`}`}
              description={
                settings.thinking_budget_tokens === 0
                  ? "⚠️ Unlimited — the model can consume its entire output window thinking, leaving nothing for the response. Set a limit."
                  : `Thinking is capped at ${settings.thinking_budget_tokens.toLocaleString()} tokens. Response gets its own separate budget below.`
              }
            >
              <input
                type="range"
                min={0}
                max={8192}
                step={256}
                className="w-full accent-primary"
                value={settings.thinking_budget_tokens}
                onChange={(e) => update("thinking_budget_tokens", parseInt(e.target.value))}
              />
              <div className="flex justify-between text-[10px] text-muted-foreground mt-0.5">
                <span>Unlimited (0)</span>
                <span>1K</span>
                <span>4K</span>
                <span>8K</span>
              </div>
            </Field>
            <Field
              label={`Response budget: ${(settings.max_response_tokens ?? 2048).toLocaleString()} tokens`}
              description="Max tokens for the visible reply (not counting thinking). Total server limit = thinking budget + response budget."
            >
              <input
                type="range"
                min={256}
                max={8192}
                step={256}
                className="w-full accent-primary"
                value={settings.max_response_tokens ?? 2048}
                onChange={(e) => update("max_response_tokens", parseInt(e.target.value))}
              />
              <div className="flex justify-between text-[10px] text-muted-foreground mt-0.5">
                <span>256</span>
                <span>2K</span>
                <span>4K</span>
                <span>8K</span>
              </div>
            </Field>
          </Section>

          {/* Remote server */}
          <Section title="Remote LLM Server (optional)">
            <Field label="Server URL" description="OpenAI-compatible endpoint (e.g., http://localhost:8080)">
              <Input
                placeholder="http://localhost:8080"
                value={settings.remote_server_url || ""}
                onChange={(e) => update("remote_server_url", e.target.value || null)}
              />
            </Field>
            <Field label="API Key">
              <Input
                type="password"
                placeholder="Optional API key"
                value={settings.remote_api_key || ""}
                onChange={(e) => update("remote_api_key", e.target.value || null)}
              />
            </Field>
          </Section>

          {/* Tools & Code Execution */}
          <Section title="Tools &amp; Code Execution">
            <Field
              label="Code Execution"
              description="Allow the AI to run code (Python, JavaScript, Shell) in a sandboxed subprocess and return real output — like Claude's code execution."
            >
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.enable_code_execution ?? false}
                  onChange={(e) => update("enable_code_execution", e.target.checked)}
                />
                <span className="text-sm">Enable code execution</span>
              </label>
              {settings.enable_code_execution && (
                <p className="text-xs text-amber-400 mt-2">
                  Requires Python 3 and/or Node.js to be installed and available on your PATH.
                  Code runs with the same permissions as this application.
                </p>
              )}
            </Field>
          </Section>

          {/* Voice Input (Whisper) */}
          <Section title="Voice Input (Whisper)">
            <Field
              label="Enable voice input"
              description="Show a microphone button in the chat input bar. Audio is transcribed locally using whisper-server."
            >
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.whisper_enabled ?? false}
                  onChange={(e) => update("whisper_enabled", e.target.checked)}
                />
                <span className="text-sm flex items-center gap-1.5">
                  <Mic className="w-3.5 h-3.5" /> Enable microphone / voice input
                </span>
              </label>
            </Field>

            {(settings.whisper_enabled) && (
              <>
                {/* Binary download — same pattern as llama-server */}
                <Field
                  label="whisper-server binary"
                  description="The whisper.cpp server binary. Downloaded from ggml-org/whisper.cpp (latest release)."
                >
                  <WhisperBinaryRow
                    variant={settings.whisper_variant ?? "cpu"}
                    onVariantChange={(v) => update("whisper_variant", v)}
                  />
                </Field>

                {/* Model */}
                <Field
                  label="Whisper model"
                  description="GGML model file for transcription. 'base' is a good balance of speed and accuracy."
                >
                  <WhisperModelRow
                    currentPath={settings.whisper_model_path}
                    onPathChange={(p) => update("whisper_model_path", p)}
                  />
                </Field>

                {/* Language */}
                <Field
                  label="Language"
                  description="BCP-47 language code (e.g. 'en', 'pt', 'es'). Use 'auto' for automatic detection."
                >
                  <Input
                    value={settings.whisper_language ?? "auto"}
                    onChange={(e) => update("whisper_language", e.target.value)}
                    placeholder="auto"
                    className="w-32"
                  />
                </Field>

                {/* Port */}
                <Field label="Server port" description="Port the whisper-server sidecar listens on (default 8765).">
                  <Input
                    type="number"
                    value={settings.whisper_port ?? 8765}
                    onChange={(e) => update("whisper_port", Number(e.target.value))}
                    className="w-28"
                  />
                </Field>

                {/* Server control */}
                <Field label="Server" description="Start or stop the whisper-server sidecar manually.">
                  <WhisperServerControl />
                </Field>
              </>
            )}
          </Section>

          {/* Image Generation */}
          <Section title="Image Generation (ComfyUI)">
            <Field
              label="ComfyUI URL"
              description="Base URL of a running ComfyUI instance. Leave empty to disable image generation."
            >
              <Input
                placeholder="http://localhost:8188"
                value={settings.comfyui_url || ""}
                onChange={(e) => update("comfyui_url", e.target.value || null)}
              />
            </Field>
            {settings.comfyui_url && (
              <>
                {/* Model type selector */}
                <Field
                  label="Model Type"
                  description="Which loader ComfyUI uses for your model. Auto-detect checks checkpoints/ first, then diffusion_models/."
                >
                  <select
                    className="w-full bg-secondary border border-border rounded px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
                    value={settings.comfyui_model_type || "auto"}
                    onChange={(e) =>
                      update("comfyui_model_type", e.target.value === "auto" ? null : e.target.value)
                    }
                  >
                    <option value="auto">Auto-detect</option>
                    <option value="checkpoint">Checkpoint (models/checkpoints/)</option>
                    <option value="unet">Diffusion Model (models/diffusion_models/)</option>
                  </select>
                </Field>

                {/* Model name */}
                <Field
                  label="Model filename"
                  description={
                    settings.comfyui_model_type === "unet"
                      ? "Filename in models/diffusion_models/ (e.g. z_image_turbo_bf16.safetensors). Leave blank to auto-pick the first available."
                      : settings.comfyui_model_type === "checkpoint"
                      ? "Filename in models/checkpoints/ (e.g. v1-5-pruned-emaonly.safetensors). Leave blank to auto-pick the first available."
                      : "Model filename — auto-detected from checkpoints/ then diffusion_models/ when left blank."
                  }
                >
                  <Input
                    placeholder="Leave blank to auto-detect"
                    value={settings.comfyui_model || ""}
                    onChange={(e) => update("comfyui_model", e.target.value || null)}
                  />
                </Field>

                {/* CLIP + VAE fields — only shown for UNETLoader models */}
                {settings.comfyui_model_type === "unet" && (
                  <>
                    <Field
                      label="CLIP model"
                      description="Filename in models/clip/ (e.g. clip_l.safetensors). Leave blank to auto-pick the first available."
                    >
                      <Input
                        placeholder="Leave blank to auto-detect"
                        value={settings.comfyui_clip_name || ""}
                        onChange={(e) => update("comfyui_clip_name", e.target.value || null)}
                      />
                    </Field>
                    <Field
                      label="VAE model"
                      description="Filename in models/vae/ (e.g. ae.safetensors). Leave blank to auto-pick the first available."
                    >
                      <Input
                        placeholder="Leave blank to auto-detect"
                        value={settings.comfyui_vae_name || ""}
                        onChange={(e) => update("comfyui_vae_name", e.target.value || null)}
                      />
                    </Field>
                  </>
                )}

                <p className="text-xs text-emerald-400">
                  Image generation enabled — the AI can now call{" "}
                  <code className="font-mono">generate_image</code> when you ask for visuals.
                </p>
                <ComfyWorkflowManager />
              </>
            )}
          </Section>

          {/* VLM / Multimodal */}
          <Section title="Vision / Multimodal (VLM)">
            <Field
              label="Multimodal Projection (mmproj)"
              description="Path to the mmproj-*.gguf file used by VLM models. This is set automatically when you start a model that has a companion mmproj file in the same folder. Clear it before loading a non-VLM model."
            >
              {settings.mmproj_path ? (
                <div className="flex items-center gap-2">
                  <code className="flex-1 text-xs font-mono bg-secondary border border-border rounded px-2 py-1.5 text-foreground truncate">
                    {settings.mmproj_path}
                  </code>
                  <button
                    className="shrink-0 text-xs px-2 py-1.5 rounded border border-destructive/40 text-destructive hover:bg-destructive/10 transition-colors"
                    onClick={() => update("mmproj_path", null)}
                    title="Clear mmproj — the next model start will not use a projection file"
                  >
                    Clear
                  </button>
                </div>
              ) : (
                <p className="text-xs text-muted-foreground italic">
                  Not set — start a VLM model from the Model Manager to configure automatically.
                </p>
              )}
            </Field>
          </Section>

          {/* Agent settings */}
          <Section title="Agent Settings">
            <Field label="Max iterations" description="Maximum ReAct loop iterations per task">
              <Input
                type="number"
                min={1}
                max={50}
                value={settings.max_agent_iterations}
                onChange={(e) => update("max_agent_iterations", parseInt(e.target.value) || 10)}
              />
            </Field>
            <Field label="Timeout (seconds)" description="Maximum time per agent task">
              <Input
                type="number"
                min={30}
                max={3600}
                value={settings.agent_timeout_seconds}
                onChange={(e) => update("agent_timeout_seconds", parseInt(e.target.value) || 300)}
              />
            </Field>
          </Section>

          {/* Memory */}
          <Section title="Memory">
            <Field
              label="Enable conversation memory"
              description="Automatically extract and recall key facts from your conversations."
            >
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.memory_enabled ?? true}
                  onChange={(e) => update("memory_enabled", e.target.checked)}
                />
                <span className="text-sm">Enable conversation memory</span>
              </label>
            </Field>
          </Section>

          {/* Knowledge Base */}
          <Section title="Knowledge Base">
            <Field
              label="Embedding model"
              description="Model name sent to the running llama-server (or Ollama) /v1/embeddings endpoint. llama-server ignores this field and uses whatever model is currently loaded; Ollama uses it to route to the right model."
            >
              <select
                className="w-full bg-background border border-input rounded-md px-3 py-2 text-sm"
                value={settings.embedding_model ?? "nomic-embed-text-v1.5"}
                onChange={(e) => update("embedding_model", e.target.value)}
              >
                <option value="nomic-embed-text-v1.5">nomic-embed-text-v1.5 (768d, recommended)</option>
                <option value="nomic-embed-text-v1.5-quantized">nomic-embed-text-v1.5 quantized (768d)</option>
                <option value="all-MiniLM-L6-v2">all-MiniLM-L6-v2 (384d, lightweight)</option>
                <option value="bge-base-en-v1.5">BGE-Base-EN v1.5 (768d)</option>
                <option value="bge-large-en-v1.5">BGE-Large-EN v1.5 (1024d)</option>
                <option value="bge-small-en-v1.5">BGE-Small-EN v1.5 (384d)</option>
                <option value="multilingual-e5-large">Multilingual E5 Large (1024d, multilingual)</option>
                <option value="multilingual-e5-base">Multilingual E5 Base (384d, multilingual)</option>
                <option value="mxbai-embed-large-v1">MxBai Embed Large v1 (1024d)</option>
              </select>
            </Field>
            <Field
              label={`Hybrid cosine weight: ${(settings.hybrid_cosine_weight ?? 0.6).toFixed(2)}`}
              description="Fraction of the final relevance score coming from semantic (cosine) similarity. The remainder comes from BM25 keyword search. 0 = BM25 only, 1 = cosine only."
            >
              <input
                type="range"
                min={0}
                max={1}
                step={0.05}
                value={settings.hybrid_cosine_weight ?? 0.6}
                onChange={(e) => update("hybrid_cosine_weight", parseFloat(e.target.value))}
                className="w-full accent-primary"
              />
            </Field>

            {/* GraphRAG sub-section */}
            <div className="mt-2 border border-border rounded-lg overflow-hidden">
              <div className="flex items-center gap-3 px-4 py-3 bg-secondary/30">
                <label className="flex items-center gap-2 cursor-pointer flex-1">
                  <input
                    type="checkbox"
                    className="w-4 h-4 accent-primary"
                    checked={settings.graph_rag_enabled ?? false}
                    onChange={(e) => update("graph_rag_enabled", e.target.checked)}
                  />
                  <span className="text-sm font-medium">Enable GraphRAG sidecar</span>
                </label>
                <span className="text-[10px] text-muted-foreground/60">
                  Advanced knowledge graph retrieval for interconnected documents
                </span>
              </div>
              {settings.graph_rag_enabled && (
                <div className="p-4 space-y-3 border-t border-border">
                  <Field
                    label="Port"
                    description="Port the graphrag-server sidecar listens on (default 3848)."
                  >
                    <Input
                      type="number"
                      min={1024}
                      max={65535}
                      value={settings.graph_rag_port ?? 3848}
                      onChange={(e) => update("graph_rag_port", parseInt(e.target.value) || 3848)}
                    />
                  </Field>
                  <Field
                    label="Auto-start"
                    description="Start the graphrag-server sidecar automatically when the app launches."
                  >
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        className="w-4 h-4 accent-primary"
                        checked={settings.graph_rag_auto_start ?? false}
                        onChange={(e) => update("graph_rag_auto_start", e.target.checked)}
                      />
                      <span className="text-sm">Auto-start with app</span>
                    </label>
                  </Field>
                  <Field
                    label="Vector database"
                    description="Backend vector store used by graphrag-server. lancedb is embedded (no extra process)."
                  >
                    <select
                      className="w-full bg-background border border-input rounded-md px-3 py-2 text-sm"
                      value={settings.graph_rag_vector_db ?? "lancedb"}
                      onChange={(e) => update("graph_rag_vector_db", e.target.value)}
                    >
                      <option value="lancedb">LanceDB (embedded, recommended)</option>
                      <option value="qdrant">Qdrant (requires separate Qdrant instance)</option>
                    </select>
                  </Field>
                  <Field
                    label="Server binary path (optional)"
                    description="Override path to the graphrag-server binary. Leave empty to use the default location in your app data directory."
                  >
                    <Input
                      placeholder="e.g. C:\tools\graphrag-server.exe"
                      value={settings.graph_rag_server_path || ""}
                      onChange={(e) => update("graph_rag_server_path", e.target.value || null)}
                    />
                  </Field>
                </div>
              )}
            </div>
          </Section>

          {/* Mobile API */}
          <Section title="Mobile API Bridge">
            <Field
              label="Enable mobile API server"
              description="Starts an embedded HTTP/SSE server so the XandSuite mobile app can connect to this desktop instance over your local network."
            >
              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  type="checkbox"
                  className="w-4 h-4 accent-primary"
                  checked={settings.mobile_api_enabled ?? false}
                  onChange={(e) => update("mobile_api_enabled", e.target.checked)}
                />
                <span className="text-sm">Enable mobile API server</span>
              </label>
            </Field>
            {settings.mobile_api_enabled && (
              <>
                <Field
                  label="Port"
                  description="Port the mobile API server listens on (default 3847). Restart the app to apply changes."
                >
                  <Input
                    type="number"
                    min={1024}
                    max={65535}
                    value={settings.mobile_api_port ?? 3847}
                    onChange={(e) => update("mobile_api_port", parseInt(e.target.value) || 3847)}
                  />
                </Field>
                <Field
                  label="API token (optional)"
                  description="If set, mobile clients must send this token as Bearer authorization. Leave empty to allow unauthenticated connections on your local network."
                >
                  <Input
                    type="password"
                    placeholder="Leave empty for no authentication"
                    value={settings.mobile_api_token || ""}
                    onChange={(e) => update("mobile_api_token", e.target.value || null)}
                  />
                </Field>
                <p className="text-xs text-emerald-400">
                  Mobile API active on port {settings.mobile_api_port ?? 3847}. Connect the mobile app to{" "}
                  <code className="font-mono">http://&lt;this-device-IP&gt;:{settings.mobile_api_port ?? 3847}</code>
                </p>
              </>
            )}
          </Section>

          {/* User profile */}
          <Section title="Profile">
            <Field label="Your name" description="How the AI addresses you.">
              <Input
                placeholder="e.g. Alex"
                value={settings.user_name ?? ""}
                onChange={(e) => update("user_name", e.target.value || null)}
              />
            </Field>
            <Field label="Your role / profession" description="Gives the AI context about your background.">
              <Input
                placeholder="e.g. Developer, Designer, Researcher…"
                value={settings.user_profession ?? ""}
                onChange={(e) => update("user_profession", e.target.value || null)}
              />
            </Field>
            <Field label="About you" description="Extra context injected into every conversation system prompt.">
              <Textarea
                placeholder="e.g. I prefer concise answers and code examples over prose…"
                value={settings.user_about ?? ""}
                onChange={(e) => update("user_about", e.target.value || null)}
                className="min-h-[80px] resize-none"
              />
            </Field>
          </Section>

          {/* App info */}
          <Section title="About">
            <div className="text-sm text-muted-foreground space-y-1">
              <p>XandSuite v0.1.0</p>
              <p>Data directory: <code className="font-mono text-xs">{dataDir}</code></p>
              <p>Built with Tauri v2 + Rust + React</p>
            </div>
          </Section>
        </div>
      </ScrollArea>
    </div>
  );
}

// ── ComfyUI Workflow Manager ──────────────────────────────────────────────────

const PLACEHOLDER_HELP = `Export your workflow from ComfyUI using "Save (API format)".
Then replace values in the JSON with these placeholders:

  __POSITIVE_PROMPT__  → the AI's image prompt
  __NEGATIVE_PROMPT__  → negative prompt
  __SEED__             → random seed
  __STEPS__            → number of steps
  __WIDTH__            → image width
  __HEIGHT__           → image height

Example: find the text node and change its "text" value to __POSITIVE_PROMPT__`;

interface WorkflowForm {
  id: string | null;
  name: string;
  description: string;
  workflow_json: string;
}

const emptyForm = (): WorkflowForm => ({ id: null, name: "", description: "", workflow_json: "" });

function ComfyWorkflowManager() {
  const [workflows, setWorkflows] = useState<ComfyWorkflow[]>([]);
  const [loading, setLoading] = useState(true);
  const [form, setForm] = useState<WorkflowForm | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);

  const load = () => {
    setLoading(true);
    invoke<ComfyWorkflow[]>("list_comfyui_workflows")
      .then(setWorkflows)
      .catch(console.error)
      .finally(() => setLoading(false));
  };

  useEffect(() => { load(); }, []);

  const handleSave = async () => {
    if (!form) return;
    setError(null);
    if (!form.name.trim()) { setError("Name is required."); return; }
    if (!form.workflow_json.trim()) { setError("Workflow JSON is required."); return; }
    try {
      JSON.parse(form.workflow_json);
    } catch {
      setError("Invalid JSON — please paste valid ComfyUI API-format workflow JSON.");
      return;
    }
    setSaving(true);
    try {
      await invoke("save_comfyui_workflow", {
        payload: {
          id: form.id ?? undefined,
          name: form.name.trim(),
          description: form.description.trim() || null,
          workflow_json: form.workflow_json.trim(),
        },
      });
      setForm(null);
      load();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: string) => {
    if (!confirm("Delete this workflow?")) return;
    await invoke("delete_comfyui_workflow", { id }).catch(console.error);
    load();
  };

  return (
    <div className="space-y-3 pt-1">
      {/* Header */}
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium">Custom Workflows</span>
        {!form && (
          <Button size="sm" variant="outline" onClick={() => setForm(emptyForm())}>
            <Plus className="w-3 h-3 mr-1" /> Add Workflow
          </Button>
        )}
      </div>

      {/* Help block */}
      <div className="rounded-md border border-border bg-secondary/20 text-xs">
        <button
          className="flex w-full items-center gap-1.5 px-3 py-2 text-muted-foreground hover:text-foreground transition-colors"
          onClick={() => setHelpOpen((v) => !v)}
        >
          {helpOpen ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
          How to use placeholder tokens
        </button>
        {helpOpen && (
          <pre className="px-3 pb-3 text-[10px] font-mono whitespace-pre-wrap text-muted-foreground leading-relaxed">
            {PLACEHOLDER_HELP}
          </pre>
        )}
      </div>

      {/* Workflow list */}
      {loading ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground py-2">
          <Loader2 className="w-3 h-3 animate-spin" /> Loading…
        </div>
      ) : workflows.length === 0 && !form ? (
        <p className="text-xs text-muted-foreground italic">
          No custom workflows yet. Add one to let the AI pick it by name.
        </p>
      ) : (
        <div className="space-y-2">
          {workflows.map((wf) => (
            <div
              key={wf.id}
              className="flex items-start justify-between gap-2 rounded-md border border-border bg-secondary/20 px-3 py-2"
            >
              <div className="min-w-0">
                <p className="text-sm font-medium truncate">{wf.name}</p>
                {wf.description && (
                  <p className="text-xs text-muted-foreground truncate">{wf.description}</p>
                )}
              </div>
              <div className="flex shrink-0 gap-1">
                <button
                  className="p-1 rounded hover:bg-white/10 text-muted-foreground hover:text-foreground transition-colors"
                  title="Edit"
                  onClick={() =>
                    setForm({
                      id: wf.id,
                      name: wf.name,
                      description: wf.description ?? "",
                      workflow_json: wf.workflow_json,
                    })
                  }
                >
                  <Pencil className="w-3.5 h-3.5" />
                </button>
                <button
                  className="p-1 rounded hover:bg-red-500/20 text-muted-foreground hover:text-red-400 transition-colors"
                  title="Delete"
                  onClick={() => handleDelete(wf.id)}
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Add / Edit form */}
      {form && (
        <div className="rounded-md border border-primary/30 bg-primary/5 p-3 space-y-3">
          <p className="text-xs font-semibold text-primary">
            {form.id ? "Edit Workflow" : "New Workflow"}
          </p>
          <div className="space-y-2">
            <Input
              placeholder="Name (e.g. Portrait)"
              value={form.name}
              onChange={(e) => setForm((f) => f && { ...f, name: e.target.value })}
            />
            <Input
              placeholder="Description (optional)"
              value={form.description}
              onChange={(e) => setForm((f) => f && { ...f, description: e.target.value })}
            />
            <textarea
              className="w-full rounded-md border border-input bg-background text-foreground text-[11px] font-mono px-3 py-2 min-h-[160px] resize-y placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
              placeholder={'Paste ComfyUI API-format workflow JSON here…\nUse __POSITIVE_PROMPT__ etc. as placeholders.'}
              value={form.workflow_json}
              onChange={(e) => setForm((f) => f && { ...f, workflow_json: e.target.value })}
            />
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
          <div className="flex gap-2">
            <Button size="sm" onClick={handleSave} disabled={saving}>
              {saving && <Loader2 className="w-3 h-3 mr-1 animate-spin" />}
              {form.id ? "Update" : "Save"}
            </Button>
            <Button size="sm" variant="outline" onClick={() => { setForm(null); setError(null); }}>
              Cancel
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h2 className="text-sm font-semibold text-foreground mb-4 pb-2 border-b border-border">{title}</h2>
      <div className="space-y-4">{children}</div>
    </div>
  );
}

function Field({ label, description, children }: { label: string; description?: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="text-sm font-medium mb-1 block">{label}</label>
      {description && <p className="text-xs text-muted-foreground mb-1.5">{description}</p>}
      {children}
    </div>
  );
}

// ── Whisper sub-components ─────────────────────────────────────────────────────

const WHISPER_SIZES = [
  { id: "tiny",     label: "Tiny",     size: "~75 MB" },
  { id: "base",     label: "Base",     size: "~142 MB" },
  { id: "small",    label: "Small",    size: "~466 MB" },
  { id: "medium",   label: "Medium",   size: "~1.5 GB" },
  { id: "large-v3", label: "Large v3", size: "~3 GB" },
];

interface WhisperStatus {
  binary_exists: boolean;
  running: boolean;
  port: number;
  model_path?: string;
  enabled: boolean;
}

const WHISPER_VARIANTS = [
  { id: "cpu",    label: "CPU only",  subtitle: "Any hardware" },
  { id: "cuda11", label: "CUDA 11",   subtitle: "GTX 10/16, RTX 20" },
  { id: "cuda12", label: "CUDA 12",   subtitle: "RTX 30/40/50" },
] as const;

function WhisperBinaryRow({
  variant,
  onVariantChange,
}: {
  variant: string;
  onVariantChange: (v: string) => void;
}) {
  const [status, setStatus] = useState<WhisperStatus | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const { saveSettings } = useSettingsStore();

  const refresh = () =>
    invoke<WhisperStatus>("get_whisper_status").then(setStatus).catch(console.error);

  useEffect(() => { refresh(); }, []);

  const handleDownload = async (v: string) => {
    setError(null);
    setDownloading(true);
    setDownloadedBytes(0);
    setTotalBytes(0);

    // Persist the chosen variant to the backend before the download command
    // reads settings — this ensures the correct build is downloaded.
    onVariantChange(v);
    await saveSettings({ whisper_variant: v });

    const unlisten = await listen<{
      model_id: string;
      downloaded_bytes: number;
      total_bytes?: number;
    }>("server_binary_progress", (event) => {
      const p = event.payload;
      if (p.model_id !== "whisper-server") return;
      if (p.total_bytes) setTotalBytes(p.total_bytes);
      setDownloadedBytes(p.downloaded_bytes);
    });

    try {
      await invoke("download_whisper_binary");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      unlisten();
      setDownloading(false);
    }
  };

  return (
    <div className="space-y-3">
      {/* Installed status */}
      <div className="flex items-center gap-2">
        {status?.binary_exists ? (
          <span className="flex items-center gap-1.5 text-xs text-emerald-400">
            <CheckCircle2 className="w-3.5 h-3.5" />
            Binary installed
          </span>
        ) : (
          <span className="text-xs text-muted-foreground">Not installed</span>
        )}
      </div>

      {/* Variant buttons */}
      <div className="flex flex-wrap gap-2">
        {WHISPER_VARIANTS.map(({ id, label, subtitle }) => {
          const isSelected = variant === id;
          return (
            <Button
              key={id}
              size="sm"
              variant={isSelected ? "default" : "outline"}
              disabled={downloading}
              onClick={() => handleDownload(id)}
              className={cn("flex-col h-auto py-1.5 px-3 gap-0", isSelected && "ring-1 ring-primary")}
            >
              <span className="flex items-center gap-1">
                {downloading && isSelected ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <Download className="w-3 h-3" />
                )}
                {label}
              </span>
              <span className="text-[9px] opacity-60">{subtitle}</span>
            </Button>
          );
        })}
      </div>

      {/* Download progress */}
      {downloading && (
        <div className="space-y-0.5">
          <Progress
            className="h-1.5"
            value={totalBytes > 0 ? (downloadedBytes / totalBytes) * 100 : undefined}
          />
          <p className="text-[10px] text-muted-foreground">
            {downloadedBytes > 0
              ? `${(downloadedBytes / 1024 / 1024).toFixed(1)} MB${totalBytes ? ` / ${(totalBytes / 1024 / 1024).toFixed(0)} MB` : ""}`
              : "Contacting GitHub…"}
          </p>
        </div>
      )}

      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  );
}

function WhisperModelRow({
  currentPath,
  onPathChange,
}: {
  currentPath?: string;
  onPathChange: (path: string) => void;
}) {
  const [selectedSize, setSelectedSize] = useState("base");
  const [downloading, setDownloading] = useState(false);
  const [downloadedBytes, setDownloadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Persist the model path immediately so the backend knows about it
  // without requiring the user to hit the global Save button first.
  const { saveSettings } = useSettingsStore();

  const currentFile = currentPath ? currentPath.replace(/\\/g, "/").split("/").pop() : null;

  const handleDownload = async () => {
    setError(null);
    setDownloading(true);
    setDownloadedBytes(0);
    setTotalBytes(0);

    // Listen for progress events emitted by the Rust download command.
    // The model_id is "whisper-{size}" (set by commands/whisper.rs).
    const targetModelId = `whisper-${selectedSize}`;
    const unlisten = await listen<{
      model_id: string;
      filename: string;
      downloaded_bytes: number;
      total_bytes?: number;
      status: string;
    }>("download_progress", (event) => {
      const p = event.payload;
      if (p.model_id !== targetModelId) return;
      if (p.total_bytes) setTotalBytes(p.total_bytes);
      setDownloadedBytes(p.downloaded_bytes);
    });

    try {
      const path = await invoke<string>("download_whisper_model", { size: selectedSize });
      // Update local UI state
      onPathChange(path);
      // Immediately persist to backend so start_whisper_server can find it
      await saveSettings({ whisper_model_path: path });
    } catch (e) {
      setError(String(e));
    } finally {
      unlisten();
      setDownloading(false);
    }
  };

  const pct = totalBytes > 0 ? Math.round((downloadedBytes / totalBytes) * 100) : 0;

  return (
    <div className="flex flex-col gap-2">
      {currentFile && (
        <p className="text-xs text-muted-foreground">
          Active: <span className="font-mono text-foreground">{currentFile}</span>
        </p>
      )}
      <div className="flex items-center gap-2 flex-wrap">
        <select
          value={selectedSize}
          onChange={(e) => setSelectedSize(e.target.value)}
          className="text-sm bg-background border border-border rounded px-2 py-1 h-8"
        >
          {WHISPER_SIZES.map((s) => (
            <option key={s.id} value={s.id}>
              {s.label} — {s.size}
            </option>
          ))}
        </select>
        {downloading ? (
          <div className="flex items-center gap-2">
            <Progress value={pct} className="w-32 h-1.5" />
            <span className="text-xs text-muted-foreground">{pct}%</span>
          </div>
        ) : (
          <Button size="sm" variant="outline" onClick={handleDownload} className="h-7 gap-1.5">
            <Download className="w-3 h-3" /> Download
          </Button>
        )}
      </div>
      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  );
}

function WhisperServerControl() {
  const [status, setStatus] = useState<WhisperStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = () =>
    invoke<WhisperStatus>("get_whisper_status").then(setStatus).catch(console.error);

  useEffect(() => { refresh(); }, []);

  const handleStart = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("start_whisper_server");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("stop_whisper_server");
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex items-center gap-3 flex-wrap">
      <span className={cn(
        "text-xs px-2 py-0.5 rounded-full font-medium",
        status?.running
          ? "bg-green-500/15 text-green-400"
          : "bg-muted text-muted-foreground"
      )}>
        {status?.running ? `Running (port ${status.port})` : "Stopped"}
      </span>
      {status?.running ? (
        <Button size="sm" variant="outline" onClick={handleStop} disabled={busy} className="h-7 gap-1.5">
          {busy ? <Loader2 className="w-3 h-3 animate-spin" /> : <Square className="w-3 h-3" />}
          Stop
        </Button>
      ) : (
        <Button size="sm" variant="outline" onClick={handleStart} disabled={busy} className="h-7 gap-1.5">
          {busy ? <Loader2 className="w-3 h-3 animate-spin" /> : <Play className="w-3 h-3" />}
          Start
        </Button>
      )}
      {error && <p className="text-xs text-destructive w-full">{error}</p>}
    </div>
  );
}
