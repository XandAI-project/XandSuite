import { useCallback, useRef, useState } from "react";

export interface AudioRecorderState {
  recording: boolean;
  error: string | null;
  start: () => Promise<void>;
  stop: () => Promise<Uint8Array>;
}

/**
 * Hook that wraps the browser MediaRecorder API for voice capture.
 *
 * - `start()` requests microphone access and begins recording.
 * - `stop()` finalises the recording and returns the raw audio bytes.
 *
 * Audio is captured as `audio/webm` (Chromium default in Tauri's WebView).
 * The bytes are forwarded to the Rust `transcribe_audio` command unchanged.
 */
export function useAudioRecorder(): AudioRecorderState {
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const chunksRef = useRef<Blob[]>([]);
  const resolveRef = useRef<((bytes: Uint8Array) => void) | null>(null);
  const rejectRef = useRef<((err: Error) => void) | null>(null);

  const start = useCallback(async () => {
    setError(null);
    chunksRef.current = [];

    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "Microphone access denied";
      setError(msg);
      throw new Error(msg);
    }

    streamRef.current = stream;

    // Pick the best MIME type available in this WebView
    const mimeType = ["audio/webm;codecs=opus", "audio/webm", "audio/ogg"].find(
      (m) => MediaRecorder.isTypeSupported(m)
    ) ?? "";

    const recorder = new MediaRecorder(stream, mimeType ? { mimeType } : undefined);
    mediaRecorderRef.current = recorder;

    recorder.ondataavailable = (e) => {
      if (e.data.size > 0) chunksRef.current.push(e.data);
    };

    recorder.onstop = async () => {
      // Stop all microphone tracks to release the indicator light
      stream.getTracks().forEach((t) => t.stop());

      const blob = new Blob(chunksRef.current, {
        type: recorder.mimeType || "audio/webm",
      });

      const arrayBuffer = await blob.arrayBuffer();
      const bytes = new Uint8Array(arrayBuffer);

      resolveRef.current?.(bytes);
      resolveRef.current = null;
      rejectRef.current = null;
    };

    recorder.onerror = () => {
      const msg = "MediaRecorder error";
      setError(msg);
      rejectRef.current?.(new Error(msg));
      resolveRef.current = null;
      rejectRef.current = null;
    };

    recorder.start(100); // emit data every 100ms
    setRecording(true);
  }, []);

  const stop = useCallback((): Promise<Uint8Array> => {
    return new Promise((resolve, reject) => {
      const recorder = mediaRecorderRef.current;
      if (!recorder || recorder.state === "inactive") {
        reject(new Error("No active recording"));
        return;
      }
      resolveRef.current = (bytes) => {
        setRecording(false);
        resolve(bytes);
      };
      rejectRef.current = (err) => {
        setRecording(false);
        reject(err);
      };
      recorder.stop();
    });
  }, []);

  return { recording, error, start, stop };
}
