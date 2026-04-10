import { useEffect, useRef, useState } from "react";
import { X, ChevronDown, Terminal } from "lucide-react";
import { cn } from "@/lib/utils";
import { useVoiceConversation, VoiceConvPhase } from "@/hooks/useVoiceConversation";
import { useSettingsStore } from "@/stores/settingsStore";
import { CloudAnimation } from "./CloudAnimation";

// ── Voice catalogue ───────────────────────────────────────────────────────────

export const KOKORO_VOICES: {
  id: string;
  label: string;
  lang: string;
  langCode: string;
}[] = [
  { id: "af_heart",    label: "Heart (F)",    lang: "English (US)", langCode: "en-us" },
  { id: "af_bella",    label: "Bella (F)",    lang: "English (US)", langCode: "en-us" },
  { id: "af_sarah",    label: "Sarah (F)",    lang: "English (US)", langCode: "en-us" },
  { id: "af_sky",      label: "Sky (F)",      lang: "English (US)", langCode: "en-us" },
  { id: "am_adam",     label: "Adam (M)",     lang: "English (US)", langCode: "en-us" },
  { id: "am_michael",  label: "Michael (M)",  lang: "English (US)", langCode: "en-us" },
  { id: "bf_emma",     label: "Emma (F)",     lang: "English (UK)", langCode: "en-gb" },
  { id: "bf_isabella", label: "Isabella (F)", lang: "English (UK)", langCode: "en-gb" },
  { id: "bm_george",   label: "George (M)",   lang: "English (UK)", langCode: "en-gb" },
  { id: "bm_lewis",    label: "Lewis (M)",    lang: "English (UK)", langCode: "en-gb" },
  { id: "pf_dora",     label: "Dora (F)",     lang: "Portuguese (BR)", langCode: "pt-br" },
  { id: "pm_alex",     label: "Alex (M)",     lang: "Portuguese (BR)", langCode: "pt-br" },
  { id: "pm_santa",    label: "Santa (M)",    lang: "Portuguese (BR)", langCode: "pt-br" },
  { id: "ef_dora",     label: "Dora (F)",     lang: "Spanish",     langCode: "es" },
  { id: "em_alex",     label: "Alex (M)",     lang: "Spanish",     langCode: "es" },
  { id: "em_santa",    label: "Santa (M)",    lang: "Spanish",     langCode: "es" },
  { id: "ff_siwis",    label: "Siwis (F)",    lang: "French",      langCode: "fr-fr" },
  { id: "if_sara",     label: "Sara (F)",     lang: "Italian",     langCode: "it" },
  { id: "hf_alpha",    label: "Alpha (F)",    lang: "Hindi",       langCode: "hi" },
  { id: "hm_omega",    label: "Omega (M)",    lang: "Hindi",       langCode: "hi" },
  { id: "jf_alpha",    label: "Alpha (F)",    lang: "Japanese",    langCode: "ja" },
  { id: "kf_alpha",    label: "Alpha (F)",    lang: "Korean",      langCode: "ko" },
  { id: "zf_xiaobei",  label: "Xiaobei (F)",  lang: "Mandarin",    langCode: "cmn" },
  { id: "zm_yunxi",    label: "Yunxi (M)",    lang: "Mandarin",    langCode: "cmn" },
];

// ── Phase labels ──────────────────────────────────────────────────────────────

const PHASE_LABEL: Record<VoiceConvPhase, string> = {
  idle:         "Press Enter or tap the orb to start",
  listening:    "Listening…",
  transcribing: "Processing speech…",
  thinking:     "Thinking…",
  speaking:     "Speaking… (Enter to interrupt)",
  error:        "Error",
};

// ── Dot animation for thinking ────────────────────────────────────────────────

function ThinkingDots() {
  const [dots, setDots] = useState(1);
  useEffect(() => {
    const t = setInterval(() => setDots((d) => (d % 3) + 1), 500);
    return () => clearInterval(t);
  }, []);
  return <span className="font-mono tracking-widest">{"•".repeat(dots)}</span>;
}

// ── Component ─────────────────────────────────────────────────────────────────

interface Props {
  onClose: () => void;
}

export function VoiceModal({ onClose }: Props) {
  const { settings, saveSettings } = useSettingsStore();
  const [voiceDropdownOpen, setVoiceDropdownOpen] = useState(false);
  const [showLogs, setShowLogs] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const logBoxRef = useRef<HTMLDivElement>(null);

  const selectedVoice = settings?.tts_voice ?? "af_heart";
  const selectedSpeed = settings?.tts_speed ?? 1.0;
  const selectedVoiceInfo = KOKORO_VOICES.find((v) => v.id === selectedVoice);

  const { phase, transcript, response, micLevel, logs, error, trigger, stop, active } =
    useVoiceConversation();

  // ── Enter key: trigger or close ───────────────────────────────────────────
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        // Escape / Enter when idle+inactive → close
        trigger();
      }
      if (e.key === "Escape") {
        stop();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [trigger, stop, onClose]);

  // ── Cleanup on unmount ────────────────────────────────────────────────────
  useEffect(() => {
    return () => stop();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── Auto-scroll log ───────────────────────────────────────────────────────
  useEffect(() => {
    if (logBoxRef.current) logBoxRef.current.scrollTop = logBoxRef.current.scrollHeight;
  }, [logs]);

  // ── Dropdown outside click ────────────────────────────────────────────────
  useEffect(() => {
    const h = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node))
        setVoiceDropdownOpen(false);
    };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, []);

  const handleVoiceSelect = async (voiceId: string, langCode: string) => {
    setVoiceDropdownOpen(false);
    // Derive the Whisper language from the TTS langCode so STT matches voice language
    const whisperLangMap: Record<string, string> = {
      "en-us": "en", "en-gb": "en",
      "pt-br": "pt", "fr-fr": "fr",
      "es": "es", "it": "it", "hi": "hi",
      "ja": "ja", "ko": "ko", "cmn": "zh",
    };
    const whisperLang = whisperLangMap[langCode] ?? "auto";
    await saveSettings({ tts_voice: voiceId, tts_language: langCode, whisper_language: whisperLang });
  };

  const handleClose = () => { stop(); onClose(); };

  const voiceGroups = KOKORO_VOICES.reduce<Record<string, typeof KOKORO_VOICES>>(
    (acc, v) => { if (!acc[v.lang]) acc[v.lang] = []; acc[v.lang].push(v); return acc; },
    {}
  );

  // Orb click: trigger or stop depending on phase
  const handleOrbClick = () => {
    if (!active) { trigger(); return; }
    if (phase === "speaking") { trigger(); return; } // interrupt
    // listening / thinking — show hint but don't interrupt
  };

  return (
    <div
      className="fixed inset-0 z-[200] flex items-center justify-center"
      style={{ backdropFilter: "blur(24px)", background: "rgba(0,0,0,0.72)" }}
    >
      <div
        className="relative flex flex-col items-center w-full max-w-sm mx-4 rounded-3xl overflow-hidden"
        style={{
          background: "rgba(255,255,255,0.05)",
          border: "1px solid rgba(255,255,255,0.10)",
          boxShadow: "0 32px 80px rgba(0,0,0,0.6), inset 0 1px 0 rgba(255,255,255,0.08)",
          backdropFilter: "blur(32px)",
        }}
      >
        {/* ── Top bar ──────────────────────────────────────────────────────── */}
        <div className="flex items-center justify-between w-full px-5 pt-5 pb-3">
          {/* Voice selector */}
          <div className="relative" ref={dropdownRef}>
            <button
              onClick={() => setVoiceDropdownOpen((v) => !v)}
              className={cn(
                "flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-sm font-medium transition-all",
                "text-white/70 hover:text-white border border-white/10 hover:border-white/20 bg-white/5 hover:bg-white/10"
              )}
            >
              <span className="max-w-[120px] truncate">
                {selectedVoiceInfo ? selectedVoiceInfo.label : selectedVoice}
              </span>
              <span className="text-white/40 text-xs truncate max-w-[80px]">
                {selectedVoiceInfo?.lang}
              </span>
              <ChevronDown className="w-3 h-3 ml-0.5 shrink-0" />
            </button>

            {voiceDropdownOpen && (
              <div
                className="absolute top-full left-0 mt-1.5 w-56 max-h-64 overflow-y-auto rounded-xl z-10"
                style={{
                  background: "rgba(20,20,30,0.95)",
                  border: "1px solid rgba(255,255,255,0.12)",
                  boxShadow: "0 12px 40px rgba(0,0,0,0.5)",
                  backdropFilter: "blur(20px)",
                }}
              >
                {Object.entries(voiceGroups).map(([lang, voices]) => (
                  <div key={lang}>
                    <div className="px-3 pt-2 pb-1 text-xs font-semibold text-white/40 uppercase tracking-wider">
                      {lang}
                    </div>
                    {voices.map((v) => (
                      <button
                        key={v.id}
                        onClick={() => handleVoiceSelect(v.id, v.langCode)}
                        className={cn(
                          "w-full text-left px-3 py-1.5 text-sm transition-colors",
                          v.id === selectedVoice
                            ? "text-white bg-white/10"
                            : "text-white/65 hover:text-white hover:bg-white/8"
                        )}
                      >
                        {v.label}
                      </button>
                    ))}
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="flex items-center gap-1">
            <button
              onClick={() => setShowLogs((v) => !v)}
              title="Debug log"
              className={cn(
                "flex items-center justify-center w-8 h-8 rounded-xl transition-all",
                showLogs ? "text-white/80 bg-white/10" : "text-white/30 hover:text-white/60 hover:bg-white/8"
              )}
            >
              <Terminal className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={handleClose}
              className="flex items-center justify-center w-8 h-8 rounded-xl text-white/50 hover:text-white hover:bg-white/10 transition-all"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* ── Phase label ───────────────────────────────────────────────────── */}
        <div className="text-sm font-medium text-white/55 h-5 flex items-center px-4 text-center">
          {error ? (
            <span className="text-red-400/80 text-xs">{error}</span>
          ) : phase === "thinking" ? (
            <span className="flex items-center gap-1.5 text-white/55">
              Thinking <ThinkingDots />
            </span>
          ) : (
            PHASE_LABEL[phase]
          )}
        </div>

        {/* ── Central orb animation ─────────────────────────────────────────── */}
        <div
          className="relative flex items-center justify-center my-4 cursor-pointer select-none"
          onClick={handleOrbClick}
          title={
            !active ? "Click or press Enter to start" :
            phase === "speaking" ? "Click or press Enter to interrupt" :
            ""
          }
        >
          <CloudAnimation phase={phase} micLevel={micLevel} />

          {/* Idle hint overlay */}
          {!active && (
            <div
              className="absolute inset-0 flex items-center justify-center rounded-full"
              style={{ background: "rgba(0,0,0,0.0)" }}
            >
              <span className="text-white/30 text-xs font-medium pointer-events-none">
                tap to start
              </span>
            </div>
          )}
        </div>

        {/* ── Transcript / response chips ───────────────────────────────────── */}
        <div className="w-full px-5 pb-3 space-y-2 min-h-[68px]">
          {transcript && (
            <div
              className="rounded-2xl px-4 py-2.5 text-sm text-white/80 leading-relaxed"
              style={{ background: "rgba(255,255,255,0.07)", border: "1px solid rgba(255,255,255,0.08)" }}
            >
              <span className="text-xs text-white/35 mr-2 font-medium uppercase tracking-wide">You</span>
              {transcript}
            </div>
          )}
          {response && (phase === "speaking" || phase === "listening" || phase === "idle") && (
            <div
              className="rounded-2xl px-4 py-2.5 text-sm text-white/80 leading-relaxed"
              style={{ background: "rgba(120,180,255,0.08)", border: "1px solid rgba(120,180,255,0.12)" }}
            >
              <span className="text-xs text-blue-300/50 mr-2 font-medium uppercase tracking-wide">AI</span>
              <span className="line-clamp-3">{response}</span>
            </div>
          )}
        </div>

        {/* ── Debug log ─────────────────────────────────────────────────────── */}
        {showLogs && (
          <div className="w-full px-3 pb-2">
            <div
              ref={logBoxRef}
              className="rounded-xl text-[10px] font-mono leading-relaxed max-h-36 overflow-y-auto p-2"
              style={{
                background: "rgba(0,0,0,0.45)",
                border: "1px solid rgba(255,255,255,0.07)",
                color: "rgba(255,255,255,0.50)",
              }}
            >
              {logs.length === 0 ? (
                <span className="opacity-40">— no events yet —</span>
              ) : (
                logs.map((line, i) => (
                  <div key={i} className={cn(
                    "whitespace-pre-wrap break-all",
                    line.includes("error") || line.includes("Error") ? "text-red-400/80" :
                    line.includes("Phase:") ? "text-blue-300/70" :
                    line.includes("Transcript:") ? "text-green-300/70" :
                    line.includes("TTS") || line.includes("Speaking") ? "text-purple-300/70" : ""
                  )}>
                    {line}
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {/* ── Speed + stop button ───────────────────────────────────────────── */}
        <div className="flex items-center justify-between w-full px-5 pb-6 gap-4">
          <div className="flex items-center gap-2 flex-1">
            <span className="text-xs text-white/35 shrink-0">Speed</span>
            <input
              type="range" min={0.5} max={2.0} step={0.1}
              value={selectedSpeed}
              onChange={(e) => saveSettings({ tts_speed: parseFloat(e.target.value) })}
              className="flex-1 accent-white/60 h-1"
            />
            <span className="text-xs text-white/40 w-6 text-right">
              {selectedSpeed.toFixed(1)}×
            </span>
          </div>

          {/* Red stop button — ends conversation */}
          <button
            onClick={active ? stop : handleClose}
            className={cn(
              "flex items-center justify-center w-12 h-12 rounded-full shrink-0 transition-all duration-200",
              active
                ? "bg-red-500/80 hover:bg-red-500 border border-red-400/30 shadow-lg shadow-red-500/20"
                : "bg-white/10 hover:bg-white/20 border border-white/15"
            )}
            title={active ? "Stop conversation" : "Close"}
          >
            {active ? (
              <span className="w-4 h-4 rounded-sm bg-white" />
            ) : (
              <X className="w-4 h-4 text-white/60" />
            )}
          </button>
        </div>
      </div>
    </div>
  );
}
