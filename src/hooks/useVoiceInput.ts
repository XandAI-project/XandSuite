import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface VoiceInputOptions {
  onTranscript: (text: string) => void;
  onTranscribing: (active: boolean) => void;
  onError: (msg: string) => void;
  language: string;
  /** Milliseconds of silence before a segment is finalised. Default 2000. */
  silenceMs?: number;
  /**
   * RMS amplitude below which audio is considered silence.
   * Range 0–1. Default 0.01 (~-40 dBFS).
   */
  silenceThreshold?: number;
  /** Minimum segment length in ms before triggering transcription. Default 300. */
  minSegmentMs?: number;
}

interface VoiceInputHandle {
  active: boolean;
  transcribing: boolean;
  start: () => Promise<void>;
  stop: () => void;
}

// ── WAV encoding ──────────────────────────────────────────────────────────────

/**
 * Encode a mono Float32 PCM buffer into a standard WAV Uint8Array.
 * whisper-server accepts 16-bit PCM WAV without requiring ffmpeg.
 */
function pcmToWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const numSamples = samples.length;
  const bytesPerSample = 2; // 16-bit
  const dataSize = numSamples * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);

  const writeStr = (offset: number, str: string) => {
    for (let i = 0; i < str.length; i++) view.setUint8(offset + i, str.charCodeAt(i));
  };
  const writeU16 = (o: number, v: number) => view.setUint16(o, v, true);
  const writeU32 = (o: number, v: number) => view.setUint32(o, v, true);

  // RIFF header
  writeStr(0, "RIFF");
  writeU32(4, 36 + dataSize);
  writeStr(8, "WAVE");
  // fmt chunk
  writeStr(12, "fmt ");
  writeU32(16, 16);       // chunk size
  writeU16(20, 1);        // PCM
  writeU16(22, 1);        // mono
  writeU32(24, sampleRate);
  writeU32(28, sampleRate * bytesPerSample); // byte rate
  writeU16(32, bytesPerSample);              // block align
  writeU16(34, 16);                          // bits per sample
  // data chunk
  writeStr(36, "data");
  writeU32(40, dataSize);

  // Convert float32 → int16
  for (let i = 0; i < numSamples; i++) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true);
  }

  return new Uint8Array(buffer);
}

// ── Hook ──────────────────────────────────────────────────────────────────────

export function useVoiceInput({
  onTranscript,
  onTranscribing,
  onError,
  language,
  silenceMs = 2000,
  silenceThreshold = 0.01,
  minSegmentMs = 300,
}: VoiceInputOptions): VoiceInputHandle {
  const [active, setActive] = useState(false);
  const [transcribing, setTranscribing] = useState(false);

  // Mutable refs — avoids stale closure issues in audio callbacks
  const activeRef = useRef(false);
  const audioCtxRef = useRef<AudioContext | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const sourceRef = useRef<MediaStreamAudioSourceNode | null>(null);

  // PCM accumulation
  const samplesRef = useRef<Float32Array[]>([]);
  const totalSamplesRef = useRef(0);

  // VAD state
  const silenceStartRef = useRef<number | null>(null);
  const vadIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Prevent overlapping transcription calls
  const transcribingRef = useRef(false);

  const setTranscribingState = useCallback((v: boolean) => {
    transcribingRef.current = v;
    setTranscribing(v);
    onTranscribing(v);
  }, [onTranscribing]);

  // ── Segment finalization ────────────────────────────────────────────────────

  const finalizeSegment = useCallback(async (sampleRate: number) => {
    if (transcribingRef.current) return;

    const chunks = samplesRef.current;
    const totalLen = totalSamplesRef.current;

    // Reset buffer immediately so new audio accumulates during transcription
    samplesRef.current = [];
    totalSamplesRef.current = 0;
    silenceStartRef.current = null;

    if (totalLen === 0) return;

    // Merge all chunk arrays into one contiguous Float32Array
    const merged = new Float32Array(totalLen);
    let offset = 0;
    for (const chunk of chunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }

    const wavBytes = pcmToWav(merged, sampleRate);

    setTranscribingState(true);
    try {
      const text = await invoke<string>("transcribe_audio", {
        audioData: Array.from(wavBytes),
        ext: "wav",
      });
      if (text && text.trim()) {
        onTranscript(text.trim());
      }
    } catch (e) {
      // Only surface errors when voice mode is still active to avoid noise
      // from the last segment after the user has stopped
      if (activeRef.current) {
        onError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      setTranscribingState(false);
    }
  }, [onTranscript, onError, setTranscribingState]);

  // ── Teardown ─────────────────────────────────────────────────────────────────

  const teardown = useCallback(() => {
    if (vadIntervalRef.current !== null) {
      clearInterval(vadIntervalRef.current);
      vadIntervalRef.current = null;
    }
    processorRef.current?.disconnect();
    analyserRef.current?.disconnect();
    sourceRef.current?.disconnect();
    streamRef.current?.getTracks().forEach((t) => t.stop());
    audioCtxRef.current?.close().catch(() => undefined);

    processorRef.current = null;
    analyserRef.current = null;
    sourceRef.current = null;
    streamRef.current = null;
    audioCtxRef.current = null;

    samplesRef.current = [];
    totalSamplesRef.current = 0;
    silenceStartRef.current = null;
  }, []);

  // ── Start ─────────────────────────────────────────────────────────────────────

  const start = useCallback(async () => {
    if (activeRef.current) return;

    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Microphone access denied";
      onError(msg);
      throw new Error(msg);
    }

    streamRef.current = stream;

    // AudioContext — prefer 16 kHz for whisper efficiency
    const ctx = new AudioContext({ sampleRate: 16000 });
    audioCtxRef.current = ctx;

    const source = ctx.createMediaStreamSource(stream);
    sourceRef.current = source;

    // Analyser for RMS measurement
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 2048;
    analyserRef.current = analyser;

    // ScriptProcessorNode to capture raw PCM
    // bufferSize 4096 ≈ 256ms at 16 kHz — small enough for responsive VAD
    const processor = ctx.createScriptProcessor(4096, 1, 1);
    processorRef.current = processor;

    source.connect(analyser);
    analyser.connect(processor);
    // Connect to destination is required for onaudioprocess to fire in Chromium
    processor.connect(ctx.destination);

    const freqData = new Uint8Array(analyser.frequencyBinCount);

    processor.onaudioprocess = (ev) => {
      if (!activeRef.current) return;
      const channelData = ev.inputBuffer.getChannelData(0);
      // Clone — the buffer is recycled after the callback
      samplesRef.current.push(new Float32Array(channelData));
      totalSamplesRef.current += channelData.length;
    };

    // VAD interval — runs every 100ms to check silence duration
    vadIntervalRef.current = setInterval(() => {
      if (!activeRef.current || transcribingRef.current) return;

      analyser.getByteFrequencyData(freqData);

      // Compute normalised RMS from frequency data (0–255 range)
      let sum = 0;
      for (let i = 0; i < freqData.length; i++) sum += (freqData[i] / 255) ** 2;
      const rms = Math.sqrt(sum / freqData.length);

      const isSilent = rms < silenceThreshold;
      const segmentMs = (totalSamplesRef.current / ctx.sampleRate) * 1000;

      if (isSilent) {
        if (silenceStartRef.current === null) {
          silenceStartRef.current = Date.now();
        } else if (
          Date.now() - silenceStartRef.current >= silenceMs &&
          segmentMs >= minSegmentMs
        ) {
          // Silence threshold exceeded — finalise this segment
          finalizeSegment(ctx.sampleRate);
        }
      } else {
        // Sound detected — reset silence timer
        silenceStartRef.current = null;
      }
    }, 100);

    activeRef.current = true;
    setActive(true);
  }, [onError, silenceMs, silenceThreshold, minSegmentMs, finalizeSegment]);

  // ── Stop ──────────────────────────────────────────────────────────────────────

  const stop = useCallback(() => {
    if (!activeRef.current) return;
    activeRef.current = false;
    setActive(false);

    // Transcribe whatever was buffered when the user clicked stop
    const sampleRate = audioCtxRef.current?.sampleRate ?? 16000;
    const segmentMs = (totalSamplesRef.current / sampleRate) * 1000;
    if (segmentMs >= minSegmentMs && !transcribingRef.current) {
      finalizeSegment(sampleRate);
    }

    teardown();
  }, [minSegmentMs, finalizeSegment, teardown]);

  return { active, transcribing, start, stop };
}
