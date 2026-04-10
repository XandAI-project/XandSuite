import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface VoiceInputOptions {
  onTranscript: (text: string) => void;
  onTranscribing: (active: boolean) => void;
  onError: (msg: string) => void;
  onLog?: (msg: string) => void;
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
  micLevel: number;
  start: () => Promise<void>;
  stop: () => void;
}

// ── WAV encoding ──────────────────────────────────────────────────────────────

function pcmToWav(samples: Float32Array, sampleRate: number): Uint8Array {
  const numSamples = samples.length;
  const bytesPerSample = 2;
  const dataSize = numSamples * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);

  const writeStr = (offset: number, str: string) => {
    for (let i = 0; i < str.length; i++) view.setUint8(offset + i, str.charCodeAt(i));
  };
  const writeU16 = (o: number, v: number) => view.setUint16(o, v, true);
  const writeU32 = (o: number, v: number) => view.setUint32(o, v, true);

  writeStr(0, "RIFF");
  writeU32(4, 36 + dataSize);
  writeStr(8, "WAVE");
  writeStr(12, "fmt ");
  writeU32(16, 16);
  writeU16(20, 1);
  writeU16(22, 1);
  writeU32(24, sampleRate);
  writeU32(28, sampleRate * bytesPerSample);
  writeU16(32, bytesPerSample);
  writeU16(34, 16);
  writeStr(36, "data");
  writeU32(40, dataSize);

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
  onLog,
  language: _language,
  silenceMs = 2000,
  silenceThreshold = 0.01,
  minSegmentMs = 300,
}: VoiceInputOptions): VoiceInputHandle {
  const [active, setActive] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [micLevel, setMicLevel] = useState(0);

  // ── Callback refs — prevents stale-closure re-renders from recreating
  //    finalizeSegment / stop and triggering useEffect cleanups. ─────────────
  const cbTranscript = useRef(onTranscript);
  const cbTranscribing = useRef(onTranscribing);
  const cbError = useRef(onError);
  const cbLog = useRef(onLog);
  useEffect(() => { cbTranscript.current = onTranscript; });
  useEffect(() => { cbTranscribing.current = onTranscribing; });
  useEffect(() => { cbError.current = onError; });
  useEffect(() => { cbLog.current = onLog; });

  const log = useCallback((msg: string) => {
    console.debug("[VoiceInput]", msg);
    cbLog.current?.(msg);
  }, []); // stable — uses ref

  // Audio pipeline refs
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

  // Silence / VAD config via refs so finalizeSegment never needs to change
  const silenceMsRef = useRef(silenceMs);
  const silenceThresholdRef = useRef(silenceThreshold);
  const minSegmentMsRef = useRef(minSegmentMs);
  useEffect(() => { silenceMsRef.current = silenceMs; });
  useEffect(() => { silenceThresholdRef.current = silenceThreshold; });
  useEffect(() => { minSegmentMsRef.current = minSegmentMs; });

  const setTranscribingState = useCallback((v: boolean) => {
    transcribingRef.current = v;
    setTranscribing(v);
    cbTranscribing.current(v);
  }, []); // stable — uses refs

  // ── Segment finalization ────────────────────────────────────────────────────

  const finalizeSegment = useCallback(async (sampleRate: number) => {
    if (transcribingRef.current) return;

    const chunks = samplesRef.current;
    const totalLen = totalSamplesRef.current;

    samplesRef.current = [];
    totalSamplesRef.current = 0;
    silenceStartRef.current = null;

    if (totalLen === 0) {
      log("finalizeSegment: empty buffer, skipping");
      return;
    }

    const durationMs = Math.round((totalLen / sampleRate) * 1000);
    log(`finalizeSegment: ${durationMs}ms audio → sending to Whisper`);

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
        log(`Whisper result: "${text.trim()}"`);
        cbTranscript.current(text.trim());
      } else {
        log("Whisper returned empty — resuming listening");
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      log(`Whisper error: ${msg}`);
      if (activeRef.current) {
        cbError.current(msg);
      }
    } finally {
      setTranscribingState(false);
    }
  }, [log, setTranscribingState]); // stable — all callbacks via refs

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
    setMicLevel(0);
  }, []); // stable

  // ── Start ─────────────────────────────────────────────────────────────────────

  const start = useCallback(async () => {
    if (activeRef.current) return;

    log("Requesting microphone access …");
    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
      log("Microphone acquired");
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Microphone access denied";
      log(`Microphone error: ${msg}`);
      cbError.current(msg);
      throw new Error(msg);
    }

    streamRef.current = stream;

    const ctx = new AudioContext({ sampleRate: 16000 });
    audioCtxRef.current = ctx;

    const source = ctx.createMediaStreamSource(stream);
    sourceRef.current = source;

    const analyser = ctx.createAnalyser();
    analyser.fftSize = 2048;
    analyserRef.current = analyser;

    const processor = ctx.createScriptProcessor(4096, 1, 1);
    processorRef.current = processor;

    source.connect(analyser);
    analyser.connect(processor);
    processor.connect(ctx.destination);

    const freqData = new Uint8Array(analyser.frequencyBinCount);

    processor.onaudioprocess = (ev) => {
      if (!activeRef.current) return;
      const channelData = ev.inputBuffer.getChannelData(0);
      samplesRef.current.push(new Float32Array(channelData));
      totalSamplesRef.current += channelData.length;
    };

    activeRef.current = true;
    setActive(true);
    log(`VAD started (silenceMs=${silenceMsRef.current}, threshold=${silenceThresholdRef.current})`);

    vadIntervalRef.current = setInterval(() => {
      if (!activeRef.current || transcribingRef.current) return;

      analyser.getByteFrequencyData(freqData);

      let sum = 0;
      for (let i = 0; i < freqData.length; i++) sum += (freqData[i] / 255) ** 2;
      const rms = Math.sqrt(sum / freqData.length);

      setMicLevel((prev) => prev * 0.7 + rms * 0.3);

      const isSilent = rms < silenceThresholdRef.current;
      const segmentMs = (totalSamplesRef.current / ctx.sampleRate) * 1000;

      if (isSilent) {
        if (silenceStartRef.current === null) {
          silenceStartRef.current = Date.now();
        } else if (
          Date.now() - silenceStartRef.current >= silenceMsRef.current &&
          segmentMs >= minSegmentMsRef.current
        ) {
          log(`Silence ${Date.now() - silenceStartRef.current}ms, segment ${Math.round(segmentMs)}ms → finalizing`);
          finalizeSegment(ctx.sampleRate);
        }
      } else {
        silenceStartRef.current = null;
      }
    }, 100);
  }, [log, finalizeSegment]); // stable

  // ── Stop ──────────────────────────────────────────────────────────────────────

  const stop = useCallback(() => {
    if (!activeRef.current) return;
    activeRef.current = false;
    setActive(false);
    log("Microphone stopped");

    const sampleRate = audioCtxRef.current?.sampleRate ?? 16000;
    const segmentMs = (totalSamplesRef.current / sampleRate) * 1000;
    if (segmentMs >= minSegmentMsRef.current && !transcribingRef.current) {
      finalizeSegment(sampleRate);
    }

    teardown();
  }, [log, finalizeSegment, teardown]); // stable

  return { active, transcribing, micLevel, start, stop };
}
