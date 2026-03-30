import { useEffect, useRef } from "react";
import { Terminal } from "lucide-react";
import { useCodingStore } from "@/stores/codingStore";

export function TerminalOutput() {
  const { terminalOutput, liveEvents } = useCodingStore();
  const endRef = useRef<HTMLDivElement>(null);

  // Auto-scroll when new output arrives
  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [terminalOutput.length, liveEvents.length]);

  // Collect live shell_exec observations for the current run
  const liveOutput = liveEvents
    .filter((e) => e.event_type === "observation" && e.payload.tool === "shell_exec")
    .map((e) => String(e.payload.observation ?? ""));

  const allOutput = [...terminalOutput, ...liveOutput];

  return (
    <div className="flex flex-col h-full min-h-0 font-mono">
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-b border-border shrink-0">
        <Terminal className="w-3.5 h-3.5 text-muted-foreground/60" />
        <span className="text-[11px] text-muted-foreground/60 font-sans">Terminal</span>
        {allOutput.length === 0 && (
          <span className="text-[10px] text-muted-foreground/30 font-sans ml-1">
            (shell_exec output appears here)
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          <span className="text-[10px] text-muted-foreground/30 font-sans">
            {allOutput.length} run{allOutput.length !== 1 ? "s" : ""}
          </span>
        </div>
      </div>

      {/* Output */}
      <div className="flex-1 overflow-y-auto min-h-0 bg-black/30 px-3 py-2 space-y-3">
        {allOutput.length === 0 ? (
          <p className="text-[11px] text-muted-foreground/20 py-2">
            $ <span className="animate-pulse">_</span>
          </p>
        ) : (
          allOutput.map((output, i) => (
            <div key={i} className="space-y-0.5">
              <div className="flex items-center gap-1.5 text-[10px] text-muted-foreground/30">
                <span>$</span>
                {/* Try to extract the command from the JSON output */}
                <span>{extractCommand(output)}</span>
              </div>
              <pre className="text-[11px] text-emerald-300/80 whitespace-pre-wrap break-words leading-relaxed">
                {extractStdout(output)}
              </pre>
              {extractStderr(output) && (
                <pre className="text-[11px] text-red-400/70 whitespace-pre-wrap break-words">
                  {extractStderr(output)}
                </pre>
              )}
            </div>
          ))
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}

function extractCommand(output: string): string {
  try {
    const obj = JSON.parse(output);
    return obj.command ?? "";
  } catch {
    return "";
  }
}

function extractStdout(output: string): string {
  try {
    const obj = JSON.parse(output);
    return obj.stdout ?? output;
  } catch {
    return output;
  }
}

function extractStderr(output: string): string {
  try {
    const obj = JSON.parse(output);
    return obj.stderr ?? "";
  } catch {
    return "";
  }
}
