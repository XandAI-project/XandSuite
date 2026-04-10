import { useCallback, useRef, useState } from "react";
import { invoke } from "@/lib/tauri";

interface UseTTSOptions {
  onError?: (msg: string) => void;
  onSpeakingChange?: (speaking: boolean) => void;
}

interface UseTTSHandle {
  speak: (text: string, voice: string, speed: number) => Promise<void>;
  stop: () => void;
  speaking: boolean;
}

/**
 * Calls the KokoroTTS backend, receives raw WAV bytes, and plays them via
 * Web Audio API.  Exposes `speaking` state for UI feedback.
 */
export function useTTS({ onError, onSpeakingChange }: UseTTSOptions = {}): UseTTSHandle {
  const [speaking, setSpeaking] = useState(false);
  const sourceRef = useRef<AudioBufferSourceNode | null>(null);
  const audioCtxRef = useRef<AudioContext | null>(null);

  const setSpeakingState = useCallback(
    (v: boolean) => {
      setSpeaking(v);
      onSpeakingChange?.(v);
    },
    [onSpeakingChange]
  );

  const stop = useCallback(() => {
    try {
      sourceRef.current?.stop();
    } catch {
      // already stopped
    }
    sourceRef.current = null;
    setSpeakingState(false);
  }, [setSpeakingState]);

  const speak = useCallback(
    async (text: string, voice: string, speed: number) => {
      if (!text.trim()) return;

      // Stop any current playback
      stop();

      let wavBytes: number[];
      try {
        wavBytes = await invoke<number[]>("synthesize_speech", {
          text: text.trim(),
          voice,
          speed,
        });
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        onError?.(msg);
        return;
      }

      if (!wavBytes || wavBytes.length === 0) return;

      try {
        // Lazily create or reuse AudioContext
        if (!audioCtxRef.current || audioCtxRef.current.state === "closed") {
          audioCtxRef.current = new AudioContext();
        }
        const ctx = audioCtxRef.current;

        // Decode the WAV bytes
        const uint8 = new Uint8Array(wavBytes);
        const audioBuffer = await ctx.decodeAudioData(uint8.buffer.slice(0));

        const source = ctx.createBufferSource();
        source.buffer = audioBuffer;
        source.connect(ctx.destination);

        sourceRef.current = source;
        setSpeakingState(true);

        source.onended = () => {
          sourceRef.current = null;
          setSpeakingState(false);
        };

        source.start();
      } catch (e) {
        setSpeakingState(false);
        const msg = e instanceof Error ? e.message : String(e);
        onError?.(msg);
      }
    },
    [stop, onError, setSpeakingState]
  );

  return { speak, stop, speaking };
}
