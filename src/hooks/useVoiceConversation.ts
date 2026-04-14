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

/**
 * Wrap raw 16-bit signed LE PCM bytes into a valid WAV container.
 * Used to convert streaming PCM chunks from Kokoro into something
 * `AudioContext.decodeAudioData` can handle.
 */
function rawPcmToWav(pcmBytes: Uint8Array, sampleRate = 24000): Uint8Array {
  const dataSize = pcmBytes.length;
  const header = new ArrayBuffer(44);
  const v = new DataView(header);
  const ws = (o: number, s: string) => { for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i)); };
  ws(0, "RIFF"); v.setUint32(4, 36 + dataSize, true); ws(8, "WAVE");
  ws(12, "fmt "); v.setUint32(16, 16, true); v.setUint16(20, 1, true);
  v.setUint16(22, 1, true); v.setUint32(24, sampleRate, true);
  v.setUint32(28, sampleRate * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
  ws(36, "data"); v.setUint32(40, dataSize, true);
  const wav = new Uint8Array(44 + dataSize);
  wav.set(new Uint8Array(header), 0);
  wav.set(pcmBytes, 44);
  return wav;
}

// ── Sentence buffer for streaming TTS ─────────────────────────────────────────

/**
 * Accumulates LLM tokens and emits complete sentences as soon as they are
 * detected.  Strips `<think>` blocks and XML tags so TTS only receives
 * clean, speakable text.
 *
 * Sentence boundary: `.` `!` `?` followed by whitespace or end-of-string,
 * with guards against decimals (3.14) and ellipses (...).
 */
class SentenceBuffer {
  private buf = "";
  private inThink = false;

  /** Minimum characters before a sentence boundary triggers a flush. */
  private static MIN_CHUNK = 40;

  /**
   * Push a new token.  Returns a flushed sentence string when a boundary is
   * detected, or `null` if more tokens are needed.
   */
  push(token: string): string | null {
    for (const ch of token) {
      if (this.processChar(ch)) continue;
    }

    // Check for a sentence boundary in the cleaned buffer.
    return this.tryExtract();
  }

  /** Drain whatever remains in the buffer (call on `done:true`). */
  flush(): string {
    const text = this.buf
      .replace(/<[^>]+>/g, "")
      .trim();
    this.buf = "";
    return text;
  }

  // -- internals --

  private processChar(ch: string): boolean {
    this.buf += ch;

    // Track <think>…</think> blocks — discard their content.
    if (!this.inThink && this.buf.endsWith("<think>")) {
      this.buf = this.buf.slice(0, -7);
      this.inThink = true;
      return true;
    }
    if (this.inThink) {
      if (this.buf.endsWith("</think>")) {
        this.buf = this.buf.slice(0, -8);
        this.inThink = false;
      }
      return true;
    }
    return false;
  }

  private tryExtract(): string | null {
    // Strip XML tags for boundary detection.
    const clean = this.buf.replace(/<[^>]+>/g, "");
    if (clean.length < SentenceBuffer.MIN_CHUNK) return null;

    // Find sentence-ending punctuation (.!?) followed by a space, quote+space,
    // or end-of-string with trailing content still in the buffer.
    // The key: we need SOMETHING after the punctuation to confirm the sentence
    // is complete (a space, a new word starting, etc.).
    const match = clean.match(/^([\s\S]*?[.!?]["\')]*)\s([\s\S]*)$/);
    if (!match) return null;

    const candidate = match[1].trim();

    // Reject false positives: decimal numbers like "3.14"
    if (/\d\.\d/.test(candidate.slice(-4))) return null;
    // Reject ellipsis: "..."
    if (candidate.endsWith("...")) return null;

    if (candidate.length < SentenceBuffer.MIN_CHUNK) return null;

    // Advance the raw buffer past the extracted portion + the space.
    // Map back from clean-text offset to raw-buffer offset by skipping tags.
    let rawIdx = 0;
    let cleanConsumed = 0;
    const targetClean = candidate.length + 1; // +1 for the space delimiter
    let inTag = false;
    while (rawIdx < this.buf.length && cleanConsumed < targetClean) {
      if (this.buf[rawIdx] === "<") inTag = true;
      if (!inTag) cleanConsumed++;
      if (this.buf[rawIdx] === ">") inTag = false;
      rawIdx++;
    }

    this.buf = this.buf.slice(rawIdx);
    return candidate;
  }
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

    unlistenRef.current?.();
    unlistenRef.current = null;

    // ── Streaming sentence-boundary TTS ──────────────────────────────────
    // Instead of waiting for the full LLM response, we detect sentence
    // boundaries in real-time and enqueue each sentence for TTS immediately.
    // A concurrent playback loop starts as soon as the first sentence is
    // ready, so the user hears audio while the LLM is still generating.

    const sentenceBuf = new SentenceBuffer();
    const ttsQueue: string[] = [];
    let queueDone = false;          // true after done:true + final flush
    let queueResolve: (() => void) | null = null;
    let ttsLoopStarted = false;
    let fullAccum = "";              // raw accumulator for setResponse

    /** Signal the playback loop that a new sentence is available. */
    const notifyQueue = () => {
      if (queueResolve) { queueResolve(); queueResolve = null; }
    };

    /** Wait until the queue has an item or is done. */
    const waitForQueue = (): Promise<void> =>
      new Promise((resolve) => {
        if (ttsQueue.length > 0 || queueDone) { resolve(); return; }
        queueResolve = resolve;
      });

    const unlisten = await listen<{ conversation_id: string; token: string; done: boolean }>(
      "chat_token",
      (ev) => {
        if (!activeRef.current) return;
        if (ev.payload.conversation_id !== convId) return;

        if (!ev.payload.done) {
          fullAccum += ev.payload.token;
          const sentence = sentenceBuf.push(ev.payload.token);
          if (sentence) {
            ttsQueue.push(sentence);
            // Start the TTS playback loop on the first sentence.
            if (!ttsLoopStarted) {
              ttsLoopStarted = true;
              goPhase("speaking");
              runTTSQueue(); // eslint-disable-line @typescript-eslint/no-use-before-define
            }
            notifyQueue();
          }
        } else {
          unlisten();
          unlistenRef.current = null;
          if (!activeRef.current) { queueDone = true; notifyQueue(); return; }

          // Flush remaining text from the sentence buffer.
          const tail = sentenceBuf.flush();
          if (tail) ttsQueue.push(tail);

          // If nothing was ever enqueued (very short or empty response),
          // update the UI and go back to listening.
          const cleaned = fullAccum
            .replace(/<think>[\s\S]*?<\/think>/g, "")
            .replace(/<[^>]+>/g, "")
            .trim();

          log(`LLM done (${cleaned.length} chars)`);
          setResponse(cleaned);

          if (!ttsLoopStarted) {
            if (ttsQueue.length > 0) {
              // Sentence(s) were flushed on done but the loop never started.
              queueDone = true;
              ttsLoopStarted = true;
              goPhase("speaking");
              runTTSQueue(); // eslint-disable-line @typescript-eslint/no-use-before-define
            } else if (cleaned) {
              // The full response didn't produce any sentence boundaries at all
              // (e.g. very short answer). Play the whole cleaned text as one shot.
              ttsQueue.push(cleaned);
              queueDone = true;
              ttsLoopStarted = true;
              goPhase("speaking");
              runTTSQueue(); // eslint-disable-line @typescript-eslint/no-use-before-define
            } else {
              // Empty response — go back to listening.
              queueDone = true;
              if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
            }
          } else {
            queueDone = true;
            notifyQueue();
          }
        }
      }
    );

    unlistenRef.current = unlisten;

    /**
     * Synthesize one sentence via the streaming PCM endpoint.
     * Collects all PCM chunks, wraps them into a WAV, and plays.
     * Returns a promise that resolves when playback finishes.
     */
    const synthesizeAndPlayStream = async (
      sentence: string, voice: string, speed: number,
    ): Promise<void> => {
      const requestId = `tts-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      const pcmChunks: Uint8Array[] = [];
      let totalBytes = 0;
      let streamDone = false;
      let streamResolve: (() => void) | null = null;

      const waitStreamDone = (): Promise<void> =>
        new Promise((resolve) => {
          if (streamDone) { resolve(); return; }
          streamResolve = resolve;
        });

      const unlistenChunk = await listen<{ request_id: string; data: number[]; done: boolean }>(
        "tts_audio_chunk",
        (ev) => {
          if (ev.payload.request_id !== requestId) return;
          if (!ev.payload.done) {
            const chunk = new Uint8Array(ev.payload.data);
            pcmChunks.push(chunk);
            totalBytes += chunk.length;
          } else {
            streamDone = true;
            if (streamResolve) { streamResolve(); streamResolve = null; }
          }
        },
      );

      try {
        await invoke("synthesize_speech_stream", {
          text: sentence, voice, speed, requestId,
        });
      } catch (e) {
        unlistenChunk();
        throw e;
      }

      await waitStreamDone();
      unlistenChunk();

      if (totalBytes === 0) return;

      // Merge PCM chunks and wrap in WAV header.
      const merged = new Uint8Array(totalBytes);
      let off = 0;
      for (const c of pcmChunks) { merged.set(c, off); off += c.length; }
      const wavData = rawPcmToWav(merged);
      await playWavBuffer(Array.from(wavData));
    };

    /** Consume the TTS queue, playing sentences as they arrive. */
    const runTTSQueue = async () => {
      const s = settingsRef.current;
      const voice = s?.tts_voice ?? "af_heart";
      const speed = s?.tts_speed ?? 1.0;

      ttsCancelledRef.current = false;
      let chunkIdx = 0;

      while (true) {
        if (ttsCancelledRef.current || !activeRef.current) break;

        await waitForQueue();
        if (ttsQueue.length === 0 && queueDone) break;
        if (ttsQueue.length === 0) continue;

        const sentence = ttsQueue.shift()!;
        chunkIdx++;
        log(`TTS queue #${chunkIdx}: "${sentence.slice(0, 50)}${sentence.length > 50 ? "…" : ""}"`);

        try {
          await synthesizeAndPlayStream(sentence, voice, speed);
        } catch (e) {
          if (ttsCancelledRef.current) break;
          // Fallback: try the non-streaming endpoint.
          log(`Stream TTS failed, falling back to batch: ${e}`);
          try {
            const wavBytes = await invoke<number[]>("synthesize_speech", { text: sentence, voice, speed });
            if (ttsCancelledRef.current || !activeRef.current) break;
            await playWavBuffer(wavBytes);
          } catch (e2) {
            if (ttsCancelledRef.current) break;
            const msg = e2 instanceof Error ? e2.message : String(e2);
            log(`TTS fallback error on chunk ${chunkIdx}: ${msg}`);
            setError(msg);
            break;
          }
        }
      }

      if (activeRef.current && phaseRef.current === "speaking") {
        log("TTS queue drained — back to listening");
        goPhase("listening");
        startMic(); // eslint-disable-line @typescript-eslint/no-use-before-define
      }
    };

    try {
      await store.sendMessage(text.trim());
    } catch (e) {
      unlisten();
      unlistenRef.current = null;
      queueDone = true;
      notifyQueue();
      const msg = e instanceof Error ? e.message : String(e);
      log(`LLM send error: ${msg}`);
      setError(msg);
      if (activeRef.current) { goPhase("listening"); startMic(); } // eslint-disable-line @typescript-eslint/no-use-before-define
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [goPhase, log]);

  // ── TTS playback ─────────────────────────────────────────────────────────

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
