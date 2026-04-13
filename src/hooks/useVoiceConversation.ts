/**
 * useVoiceConversation — single self-contained voice state machine.
 *
 * All audio I/O runs imperatively inside refs.  State updates are batched at
 * transition points so React never triggers a re-render that cascades into
 * hook recreation / effect cleanup.
 *
 * Flow:
 *   idle ──[Enter/click]──► listening
 *   listening ──[2s silence]──► transcribing ──► thinking ──► speaking
 *   speaking ──[TTS ends OR Enter]──► listening
 *   any ──[stop()]──► idle (inactive)
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@/lib/tauri";
import { listen } from "@/lib/tauri";
import { useChatStore } from "@/stores/chatStore";
import { useSettingsStore } from "@/stores/settingsStore";

/** Map a TTS langCode (e.g. "pt-br", "fr-fr") to a Whisper ISO-639-1 code. */
function _ttsLangToWhisper(langCode: string): string {
  const map: Record<string, string> = {
    "en-us": "en", "en-gb": "en",
    "pt-br": "pt", "pt":    "pt",
    "fr-fr": "fr", "fr":    "fr",
    "es":    "es",
    "it":    "it",
    "hi":    "hi",
    "ja":    "ja",
    "ko":    "ko",
    "cmn":   "zh", "zh":    "zh",
    "de":    "de",
    "ru":    "ru",
    "ar":    "ar",
  };
  return map[langCode.toLowerCase()] ?? "auto";
}

export type VoiceConvPhase =
  | "idle"
  | "listening"
  | "transcribing"
  | "thinking"
  | "speaking"
  | "error";

interface UseVoiceConversationHandle {
  phase: VoiceConvPhase;
  transcript: string;
  response: string;
  micLevel: number;
  logs: string[];
  error: string | null;
  /** Start listening (from idle) or interrupt TTS (from speaking). */
  trigger: () => void;
  /** End the entire conversation. */
  stop: () => void;
  active: boolean;
}

// ── WAV encoder ───────────────────────────────────────────────────────────────

function pcmToWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const dataSize = samples.length * 2;
  const buf = new ArrayBuffer(44 + dataSize);
  const v = new DataView(buf);
  const ws = (o: number, s: string) => { for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i)); };
  ws(0, "RIFF"); v.setUint32(4, 36 + dataSize, true); ws(8, "WAVE");
  ws(12, "fmt "); v.setUint32(16, 16, true); v.setUint16(20, 1, true);
  v.setUint16(22, 1, true); v.setUint32(24, sampleRate, true);
  v.setUint32(28, sampleRate * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  ws(36, "data"); v.setUint32(40, dataSize, true);
  for (let i = 0; i < samples.length; i++) {
    const c = Math.max(-1, Math.min(1, samples[i]));
    v.setInt16(44 + i * 2, c < 0 ? c * 0x8000 : c * 0x7fff, true);
  }
  return new Uint8Array(buf);
}

// ── Hook ──────────────────────────────────────────────────────────────────────

export function useVoiceConversation(): UseVoiceConversationHandle {
  // ── React state (UI only) ─────────────────────────────────────────────────
  const [phase, setPhase] = useState<VoiceConvPhase>("idle");
  const [transcript, setTranscript] = useState("");
  const [response, setResponse] = useState("");
  const [micLevel, setMicLevel] = useState(0);
  const [logs, setLogs] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [active, setActive] = useState(false);

  // ── Imperative refs (no re-render on change) ──────────────────────────────
  const phaseRef = useRef<VoiceConvPhase>("idle");
  const activeRef = useRef(false);

  // Audio pipeline
  const audioCtxRef = useRef<AudioContext | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const samplesRef = useRef<Float32Array[]>([]);
  const totalSamplesRef = useRef(0);
  const silenceTimerRef = useRef<number | null>(null);
  const vadIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // TTS playback (pipelined: synthesize chunk N+1 while playing chunk N)
  const ttsCtxRef = useRef<AudioContext | null>(null);
  const ttsSourceRef = useRef<AudioBufferSourceNode | null>(null);
  const ttsCancelledRef = useRef(false);

  // LLM event listener cleanup
  const unlistenRef = useRef<(() => void) | null>(null);

  // Settings access (read at call time, not during render)
  const settingsRef = useRef(useSettingsStore.getState().settings);
  const chatStoreRef = useRef(useChatStore.getState());

  useEffect(() => {
    const unsubSettings = useSettingsStore.subscribe(
      (s) => { settingsRef.current = s.settings; }
    );
    const unsubChat = useChatStore.subscribe(
      (s) => { chatStoreRef.current = s; }
    );
    return () => { unsubSettings(); unsubChat(); };
  }, []);

  // ── Logging ───────────────────────────────────────────────────────────────

  const log = useCallback((msg: string) => {
    const ts = new Date().toLocaleTimeString("en-US", { hour12: false });
    const line = `[${ts}] ${msg}`;
    console.debug("[Voice]", msg);
    setLogs((prev) => [...prev.slice(-299), line]);
  }, []);

  // ── Phase transition ──────────────────────────────────────────────────────

  const goPhase = useCallback((p: VoiceConvPhase) => {
    log(`Phase: ${phaseRef.current} → ${p}`);
    phaseRef.current = p;
    setPhase(p);
    if (p === "error") setActive(false);
  }, [log]);

  // ── Mic teardown ──────────────────────────────────────────────────────────

  const teardownMic = useCallback(() => {
    if (vadIntervalRef.current !== null) {
      clearInterval(vadIntervalRef.current);
      vadIntervalRef.current = null;
    }
    processorRef.current?.disconnect();
    analyserRef.current?.disconnect();
    streamRef.current?.getTracks().forEach((t) => t.stop());
    audioCtxRef.current?.close().catch(() => {});
    processorRef.current = null;
    analyserRef.current = null;
    streamRef.current = null;
    audioCtxRef.current = null;
    samplesRef.current = [];
    totalSamplesRef.current = 0;
    silenceTimerRef.current = null;
    setMicLevel(0);
  }, []);

  // ── TTS stop ──────────────────────────────────────────────────────────────

  const stopTTS = useCallback(() => {
    ttsCancelledRef.current = true;
    try { ttsSourceRef.current?.stop(); } catch { /* already ended */ }
    ttsSourceRef.current = null;
  }, []);

  // ── Send segment to Whisper then LLM ─────────────────────────────────────

  const sendSegment = useCallback(async (samples: Float32Array[], totalLen: number, sampleRate: number) => {
    if (totalLen < sampleRate * 0.2) {
      log("Segment too short, ignoring");
      goPhase("listening");
      startMic(); // eslint-disable-line @typescript-eslint/no-use-before-define
      return;
    }

    goPhase("transcribing");

    const merged = new Float32Array(totalLen);
    let off = 0;
    for (const c of samples) { merged.set(c, off); off += c.length; }

    const wav = pcmToWav(merged, sampleRate);
    log(`Sending ${Math.round((totalLen / sampleRate) * 1000)}ms to Whisper…`);

    // Derive Whisper language from the TTS voice language.
    // Whisper uses ISO 639-1 codes; map from the TTS langCode.
    const ttsLang = settingsRef.current?.tts_language ?? "en-us";
    const whisperLang = settingsRef.current?.whisper_language ?? _ttsLangToWhisper(ttsLang);
    log(`Whisper lang: ${whisperLang}`);

    let text = "";
    try {
      text = await invoke<string>("transcribe_audio", {
        audioData: Array.from(wav),
        ext: "wav",
        language: whisperLang === "auto" ? undefined : whisperLang,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      log(`Whisper error: ${msg}`);
      setError(msg);
      if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
      return;
    }

    if (!text || !text.trim()) {
      log("Whisper returned empty — resuming listening");
      if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
      return;
    }

    log(`Transcript: "${text.trim()}"`);
    setTranscript(text.trim());
    setError(null);
    goPhase("thinking");

    // ── Send to LLM ────────────────────────────────────────────────────────
    let store = chatStoreRef.current;
    if (!store.activeConversation) {
      log("No active conversation — creating one");
      try {
        const conv = await useChatStore.getState().createConversation();
        await useChatStore.getState().openConversation(conv.id);
        // Re-read the store after creating the conversation
        store = useChatStore.getState();
      } catch (e) {
        log(`Failed to create conversation: ${e}`);
        if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
        return;
      }
    }

    if (!store.activeConversation) {
      log("Still no active conversation after creation — aborting");
      if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
      return;
    }

    const convId = store.activeConversation.id;
    log(`Sending to LLM (conv ${convId.slice(0, 8)}…)`);

    let accum = "";
    unlistenRef.current?.();
    unlistenRef.current = null;

    const unlisten = await listen<{ conversation_id: string; token: string; done: boolean }>(
      "chat_token",
      async (ev) => {
        if (!activeRef.current) return;
        if (ev.payload.conversation_id !== convId) return;

        if (!ev.payload.done) {
          accum += ev.payload.token;
        } else {
          unlisten();
          unlistenRef.current = null;
          if (!activeRef.current) return;

          const cleaned = accum
            .replace(/<think>[\s\S]*?<\/think>/g, "")
            .replace(/<[^>]+>/g, "")
            .trim();

          log(`LLM done (${cleaned.length} chars)`);

          if (!cleaned) {
            if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
            return;
          }

          setResponse(cleaned);
          goPhase("speaking");
          await playTTS(cleaned); // eslint-disable-line @typescript-eslint/no-use-before-define
        }
      }
    );

    unlistenRef.current = unlisten;

    try {
      await store.sendMessage(text.trim());
    } catch (e) {
      unlisten();
      unlistenRef.current = null;
      const msg = e instanceof Error ? e.message : String(e);
      log(`LLM send error: ${msg}`);
      setError(msg);
      if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [goPhase, log]);

  // ── TTS playback ─────────────────────────────────────────────────────────

  /**
   * Split text into sentence-level chunks for pipelined TTS.
   * Splits on sentence-ending punctuation followed by a space, keeping each
   * chunk a reasonable length for low-latency synthesis.
   */
  const splitTTSChunks = useCallback((text: string): string[] => {
    // Split on sentence boundaries (. ! ? followed by space or end of string).
    // Also split on semicolons and colons for long compound sentences.
    const raw = text.match(/[^.!?;:]+[.!?;:]*/g) ?? [text];
    const chunks: string[] = [];
    let buf = "";

    for (const seg of raw) {
      buf += seg;
      // Flush when we have a reasonable chunk (>40 chars) or it's the last segment.
      if (buf.trim().length >= 40) {
        chunks.push(buf.trim());
        buf = "";
      }
    }
    if (buf.trim()) chunks.push(buf.trim());
    return chunks.filter(Boolean);
  }, []);

  /**
   * Play a single WAV buffer and return a promise that resolves when it ends.
   * The promise rejects if TTS is cancelled (user interrupted).
   */
  const playWavBuffer = useCallback((wavBytes: number[]): Promise<void> => {
    return new Promise(async (resolve, reject) => {
      if (ttsCancelledRef.current || !activeRef.current) { reject(new Error("cancelled")); return; }

      try {
        if (!ttsCtxRef.current || ttsCtxRef.current.state === "closed") {
          ttsCtxRef.current = new AudioContext();
        }
        const ctx = ttsCtxRef.current;
        const uint8 = new Uint8Array(wavBytes);
        const audioBuf = await ctx.decodeAudioData(uint8.buffer.slice(0));

        if (ttsCancelledRef.current || !activeRef.current) { reject(new Error("cancelled")); return; }

        const src = ctx.createBufferSource();
        src.buffer = audioBuf;
        src.connect(ctx.destination);
        ttsSourceRef.current = src;

        src.onended = () => {
          ttsSourceRef.current = null;
          resolve();
        };

        src.start();
      } catch (e) {
        reject(e);
      }
    });
  }, []);

  /**
   * Pipelined TTS: split text into sentence chunks, synthesize chunk N+1
   * while playing chunk N, so the user hears audio almost immediately.
   */
  const playTTS = useCallback(async (text: string) => {
    const s = settingsRef.current;
    const voice = s?.tts_voice ?? "af_heart";
    const speed = s?.tts_speed ?? 1.0;

    const chunks = splitTTSChunks(text);
    log(`TTS voice=${voice} speed=${speed} chunks=${chunks.length}`);

    ttsCancelledRef.current = false;

    // Pre-fetch the first chunk immediately.
    let nextSynthesis: Promise<number[]> | null = null;

    for (let i = 0; i < chunks.length; i++) {
      if (ttsCancelledRef.current || !activeRef.current) break;

      const chunk = chunks[i];
      log(`TTS chunk ${i + 1}/${chunks.length}: "${chunk.slice(0, 50)}${chunk.length > 50 ? "…" : ""}"`);

      // Either use the pre-fetched synthesis or start a new one.
      let wavBytes: number[];
      try {
        if (nextSynthesis) {
          wavBytes = await nextSynthesis;
          nextSynthesis = null;
        } else {
          wavBytes = await invoke<number[]>("synthesize_speech", { text: chunk, voice, speed });
        }
      } catch (e) {
        if (ttsCancelledRef.current) break;
        const msg = e instanceof Error ? e.message : String(e);
        log(`TTS error on chunk ${i + 1}: ${msg}`);
        setError(msg);
        break;
      }

      if (ttsCancelledRef.current || !activeRef.current) break;

      // Start synthesizing the NEXT chunk in the background while we play this one.
      if (i + 1 < chunks.length) {
        const nextChunk = chunks[i + 1];
        nextSynthesis = invoke<number[]>("synthesize_speech", { text: nextChunk, voice, speed });
      }

      // Play the current chunk and wait for it to finish.
      try {
        await playWavBuffer(wavBytes);
      } catch {
        break; // cancelled or decode error
      }
    }

    // All chunks done (or cancelled) — transition back to listening.
    if (activeRef.current && phaseRef.current === "speaking") {
      log("TTS ended — back to listening");
      goPhase("listening");
      startMic(); // eslint-disable-line @typescript-eslint/no-use-before-define
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [goPhase, log, stopTTS, splitTTSChunks, playWavBuffer]);

  // ── Start microphone + VAD ────────────────────────────────────────────────

  const startMic = useCallback(async () => {
    if (!activeRef.current) return;

    teardownMic();
    samplesRef.current = [];
    totalSamplesRef.current = 0;
    silenceTimerRef.current = null;

    log("Requesting microphone…");
    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      log("Mic acquired");
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Mic denied";
      log(`Mic error: ${msg}`);
      setError(msg);
      goPhase("error");
      return;
    }

    if (!activeRef.current) { stream.getTracks().forEach((t) => t.stop()); return; }

    streamRef.current = stream;
    const ctx = new AudioContext({ sampleRate: 16000 });
    audioCtxRef.current = ctx;

    const source = ctx.createMediaStreamSource(stream);
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 2048;
    analyserRef.current = analyser;

    const processor = ctx.createScriptProcessor(4096, 1, 1);
    processorRef.current = processor;

    source.connect(analyser);
    analyser.connect(processor);
    processor.connect(ctx.destination);

    processor.onaudioprocess = (ev) => {
      if (phaseRef.current !== "listening") return;
      const ch = ev.inputBuffer.getChannelData(0);
      samplesRef.current.push(new Float32Array(ch));
      totalSamplesRef.current += ch.length;
    };

    const SILENCE_MS = 2000;
    const MIN_SEGMENT_MS = 300;
    // Adaptive noise floor: measured during the first ~600 ms then fixed.
    // We treat anything less than (noiseFloor * NOISE_MULTIPLIER) as silence.
    const NOISE_MULTIPLIER = 3.0;
    const CALIBRATION_MS = 600;
    let noiseFloor = 0.01;      // conservative starting value
    let noiseCalibrated = false;
    let noiseSamples: number[] = [];
    const calibrationStart = Date.now();

    goPhase("listening");
    log(`VAD running (silence=${SILENCE_MS}ms) — calibrating noise floor…`);

    vadIntervalRef.current = setInterval(() => {
      if (phaseRef.current !== "listening" || !activeRef.current) return;

      // Use time-domain RMS — more reliable than frequency-domain for VAD.
      const timeBuf = new Float32Array(analyser.fftSize);
      analyser.getFloatTimeDomainData(timeBuf);
      let sum = 0;
      for (let i = 0; i < timeBuf.length; i++) sum += timeBuf[i] * timeBuf[i];
      const rms = Math.sqrt(sum / timeBuf.length);

      // Calibration phase: collect ambient noise level for CALIBRATION_MS.
      if (!noiseCalibrated) {
        noiseSamples.push(rms);
        if (Date.now() - calibrationStart >= CALIBRATION_MS) {
          const avg = noiseSamples.reduce((a, b) => a + b, 0) / noiseSamples.length;
          noiseFloor = Math.max(avg, 0.005);  // never go below 0.005
          noiseCalibrated = true;
          log(`Noise floor calibrated: ${noiseFloor.toFixed(4)} (threshold: ${(noiseFloor * NOISE_MULTIPLIER).toFixed(4)})`);
          noiseSamples = [];
        }
        return; // don't trigger silence during calibration
      }

      setMicLevel((prev) => prev * 0.7 + rms * 0.3);

      const segMs = (totalSamplesRef.current / ctx.sampleRate) * 1000;
      const isSilent = rms < noiseFloor * NOISE_MULTIPLIER;

      if (!isSilent) {
        if (silenceTimerRef.current !== null) {
          silenceTimerRef.current = null;
        }
        return;
      }

      if (silenceTimerRef.current === null) {
        silenceTimerRef.current = Date.now();
      } else if (
        Date.now() - silenceTimerRef.current >= SILENCE_MS &&
        segMs >= MIN_SEGMENT_MS
      ) {
        const snapshotSamples = samplesRef.current;
        const snapshotLen = totalSamplesRef.current;
        samplesRef.current = [];
        totalSamplesRef.current = 0;
        silenceTimerRef.current = null;

        log(`Silence triggered (${Math.round(segMs)}ms segment)`);

        // Stop mic while processing
        if (vadIntervalRef.current !== null) {
          clearInterval(vadIntervalRef.current);
          vadIntervalRef.current = null;
        }
        teardownMic();

        sendSegment(snapshotSamples, snapshotLen, ctx.sampleRate);
      }
    }, 100);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [goPhase, log, teardownMic, sendSegment]);

  // ── Public API ────────────────────────────────────────────────────────────

  /**
   * Enter / click the orb:
   * - idle → start listening
   * - listening → nothing (already recording)
   * - thinking → nothing (wait for LLM)
   * - speaking → interrupt TTS, start listening again
   */
  const trigger = useCallback(() => {
    if (!activeRef.current) {
      // First trigger: start the conversation
      log("Conversation started");
      activeRef.current = true;
      setActive(true);
      setTranscript("");
      setResponse("");
      setError(null);
      startMic();
      return;
    }

    if (phaseRef.current === "speaking") {
      log("Interrupted TTS → listening");
      stopTTS();
      startMic();
    }
    // listening / thinking: do nothing (let the flow complete)
  }, [log, startMic, stopTTS]);

  const stop = useCallback(() => {
    log("Conversation stopped");
    activeRef.current = false;
    setActive(false);
    phaseRef.current = "idle";
    setPhase("idle");

    stopTTS();
    teardownMic();
    unlistenRef.current?.();
    unlistenRef.current = null;
  }, [log, stopTTS, teardownMic]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      activeRef.current = false;
      try { ttsSourceRef.current?.stop(); } catch { /* ignore */ }
      if (vadIntervalRef.current) clearInterval(vadIntervalRef.current);
      streamRef.current?.getTracks().forEach((t) => t.stop());
      audioCtxRef.current?.close().catch(() => {});
      ttsCtxRef.current?.close().catch(() => {});
      unlistenRef.current?.();
    };
  }, []); // intentionally empty — only runs on unmount

  return { phase, transcript, response, micLevel, logs, error, trigger, stop, active };
}
