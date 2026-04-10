import { useEffect, useRef } from "react";
import type { VoiceConvPhase } from "@/hooks/useVoiceConversation";

interface Props {
  phase: VoiceConvPhase;
  /** 0–1 mic RMS level for listening animation */
  micLevel?: number;
}

/**
 * Central animated element for the voice modal.
 *
 * idle      → static frosted sphere
 * listening → morphing cloud blobs that react to mic level
 * transcribing → pulsing ring
 * thinking  → four bouncing white pills ("...")
 * speaking  → concentric ripple rings
 * error     → static red-tinted sphere
 */
export function CloudAnimation({ phase, micLevel = 0 }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef<number>(0);
  const timeRef = useRef(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const W = canvas.width;
    const H = canvas.height;
    const cx = W / 2;
    const cy = H / 2;

    const drawIdle = (t: number) => {
      ctx.clearRect(0, 0, W, H);
      const radius = 80 + Math.sin(t * 0.8) * 4;
      const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, radius);
      grad.addColorStop(0, "rgba(255,255,255,0.22)");
      grad.addColorStop(0.6, "rgba(200,220,255,0.12)");
      grad.addColorStop(1, "rgba(255,255,255,0.04)");
      ctx.beginPath();
      ctx.arc(cx, cy, radius, 0, Math.PI * 2);
      ctx.fillStyle = grad;
      ctx.fill();
      // border glow
      ctx.beginPath();
      ctx.arc(cx, cy, radius, 0, Math.PI * 2);
      ctx.strokeStyle = "rgba(255,255,255,0.18)";
      ctx.lineWidth = 1.5;
      ctx.stroke();
    };

    const drawBlob = (
      t: number,
      offsetX: number,
      offsetY: number,
      baseR: number,
      pulseAmp: number,
      alpha: number,
      color: string
    ) => {
      const r = baseR + Math.sin(t + offsetX) * pulseAmp * (1 + micLevel * 2);
      const x = cx + offsetX * (1 + micLevel * 0.5) * Math.sin(t * 0.4);
      const y = cy + offsetY * (1 + micLevel * 0.5) * Math.cos(t * 0.5);
      const grad = ctx.createRadialGradient(x, y, 0, x, y, r);
      grad.addColorStop(0, `rgba(${color},${alpha})`);
      grad.addColorStop(1, `rgba(${color},0)`);
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fillStyle = grad;
      ctx.fill();
    };

    const drawListening = (t: number) => {
      ctx.clearRect(0, 0, W, H);
      const boost = 1 + micLevel * 1.5;
      // Three overlapping morphing blobs
      drawBlob(t, 0, 0, 72 * boost, 18 * boost, 0.18, "220,235,255");
      drawBlob(t * 1.1, -28, 18, 55 * boost, 14 * boost, 0.22, "255,255,255");
      drawBlob(t * 0.9, 22, -22, 60 * boost, 16 * boost, 0.20, "180,210,255");
      drawBlob(t * 1.3, -18, -30, 48 * boost, 12 * boost, 0.26, "255,255,255");
      // Outer glow ring that grows with mic level
      const ringR = 88 + micLevel * 30;
      ctx.beginPath();
      ctx.arc(cx, cy, ringR, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(255,255,255,${0.08 + micLevel * 0.18})`;
      ctx.lineWidth = 2;
      ctx.stroke();
    };

    const drawTranscribing = (t: number) => {
      ctx.clearRect(0, 0, W, H);
      const r = 80 + Math.sin(t * 3) * 8;
      const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
      grad.addColorStop(0, "rgba(180,210,255,0.20)");
      grad.addColorStop(1, "rgba(255,255,255,0.04)");
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fillStyle = grad;
      ctx.fill();
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(255,255,255,${0.25 + Math.sin(t * 3) * 0.15})`;
      ctx.lineWidth = 2;
      ctx.stroke();
    };

    const drawThinking = (t: number) => {
      ctx.clearRect(0, 0, W, H);
      // 4 pills like the "..." loader in the reference images
      const count = 4;
      const gap = 22;
      const totalW = (count - 1) * gap;
      const pillW = 16;
      const pillH = 16;
      for (let i = 0; i < count; i++) {
        const x = cx - totalW / 2 + i * gap;
        const phase_offset = i * 0.4;
        const yOff = Math.sin(t * 3.5 + phase_offset) * 10;
        const alpha = 0.45 + Math.sin(t * 3.5 + phase_offset) * 0.35;
        ctx.beginPath();
        ctx.roundRect(x - pillW / 2, cy - pillH / 2 + yOff, pillW, pillH, pillH / 2);
        ctx.fillStyle = `rgba(255,255,255,${Math.max(0.12, alpha)})`;
        ctx.fill();
      }
    };

    const drawSpeaking = (t: number) => {
      ctx.clearRect(0, 0, W, H);
      const numRings = 4;
      for (let i = 0; i < numRings; i++) {
        const progress = ((t * 0.7 + i * (1 / numRings)) % 1);
        const r = 30 + progress * 100;
        const alpha = (1 - progress) * 0.25;
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(255,255,255,${alpha})`;
        ctx.lineWidth = 2.5 - progress * 1.5;
        ctx.stroke();
      }
      // Central blob
      drawBlob(t, 0, 0, 52, 10, 0.22, "220,235,255");
    };

    const drawError = () => {
      ctx.clearRect(0, 0, W, H);
      const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, 80);
      grad.addColorStop(0, "rgba(255,80,80,0.18)");
      grad.addColorStop(1, "rgba(255,80,80,0.04)");
      ctx.beginPath();
      ctx.arc(cx, cy, 80, 0, Math.PI * 2);
      ctx.fillStyle = grad;
      ctx.fill();
    };

    const animate = (timestamp: number) => {
      const t = timestamp * 0.001;
      timeRef.current = t;

      switch (phase) {
        case "idle":
          drawIdle(t);
          break;
        case "listening":
          drawListening(t);
          break;
        case "transcribing":
          drawTranscribing(t);
          break;
        case "thinking":
          drawThinking(t);
          break;
        case "speaking":
          drawSpeaking(t);
          break;
        case "error":
          drawError();
          break;
      }

      rafRef.current = requestAnimationFrame(animate);
    };

    rafRef.current = requestAnimationFrame(animate);

    return () => {
      cancelAnimationFrame(rafRef.current);
    };
  }, [phase, micLevel]);

  return (
    <canvas
      ref={canvasRef}
      width={260}
      height={260}
      className="select-none pointer-events-none"
      style={{ imageRendering: "pixelated" }}
    />
  );
}
