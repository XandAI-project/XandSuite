import { useEffect, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  RotateCw,
  Pause,
  Play,
  MousePointer2,
  Hand,
  Power,
  PlayCircle,
  Cookie,
  Bookmark,
  Eye,
  EyeOff,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useBrowserAgentStore } from "@/stores/browserAgentStore";
import { invoke } from "@/lib/tauri";

interface CookieSessionDigest {
  id: string;
  name: string;
  cookie_count: number;
  domains: string[];
}

interface BrowserToolbarProps {
  conversationId: string | null;
}

export function BrowserToolbar({ conversationId }: BrowserToolbarProps) {
  const sessionId = useBrowserAgentStore((s) => s.sessionId);
  const url = useBrowserAgentStore((s) => s.url);
  const paused = useBrowserAgentStore((s) => s.paused);
  const takeover = useBrowserAgentStore((s) => s.takeover);
  const streaming = useBrowserAgentStore((s) => s.streaming);
  const busy = useBrowserAgentStore((s) => s.busy);
  const cookieSessionId = useBrowserAgentStore((s) => s.cookieSessionId);
  const setCookieSessionId = useBrowserAgentStore((s) => s.setCookieSessionId);
  const stealth = useBrowserAgentStore((s) => s.stealth);
  const setStealth = useBrowserAgentStore((s) => s.setStealth);
  const startSession = useBrowserAgentStore((s) => s.startSession);
  const stopSession = useBrowserAgentStore((s) => s.stopSession);
  const startScreencast = useBrowserAgentStore((s) => s.startScreencast);
  const stopScreencast = useBrowserAgentStore((s) => s.stopScreencast);
  const togglePause = useBrowserAgentStore((s) => s.togglePause);
  const setTakeover = useBrowserAgentStore((s) => s.setTakeover);
  const navigate = useBrowserAgentStore((s) => s.navigate);

  const [urlInput, setUrlInput] = useState(url);
  useEffect(() => setUrlInput(url), [url]);

  // Named profiles give Chromium a persistent user-data-dir — localStorage,
  // IndexedDB, and cookies survive across restarts. That's how sites like
  // WhatsApp Web stay logged in after the user scans the QR code once. Empty
  // string = disposable profile (wiped on session end, same as before).
  const [profileName, setProfileName] = useState<string>(() => {
    try {
      return localStorage.getItem("browserProfileName") ?? "";
    } catch {
      return "";
    }
  });
  const saveProfileName = (v: string) => {
    setProfileName(v);
    try {
      if (v.trim()) localStorage.setItem("browserProfileName", v.trim());
      else localStorage.removeItem("browserProfileName");
    } catch {
      /* quota / sandbox — silently fall back to in-memory state */
    }
  };

  // Refresh the saved cookie sessions every time the toolbar mounts (and
  // whenever the session is torn down) so newly-added entries from Settings
  // show up without requiring a full app reload.
  const [cookieSessions, setCookieSessions] = useState<CookieSessionDigest[]>(
    []
  );
  useEffect(() => {
    if (sessionId) return; // only need the list when picking pre-launch
    let cancelled = false;
    void (async () => {
      try {
        const list = await invoke<CookieSessionDigest[]>(
          "list_browser_cookie_sessions"
        );
        if (!cancelled) setCookieSessions(list);
      } catch {
        /* ignore — surfaced in Settings */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const sessionActive = !!sessionId;
  const disabled = !sessionActive || busy;

  const submitUrl = (e: React.FormEvent) => {
    e.preventDefault();
    if (!urlInput.trim()) return;
    let target = urlInput.trim();
    if (!/^https?:\/\//i.test(target) && target !== "about:blank") {
      target = `https://${target}`;
    }
    void navigate(target);
  };

  const doGoBack = async () => {
    if (!conversationId) return;
    try {
      await invoke("browser_toolbar_back", { conversationId });
    } catch {
      /* ignore */
    }
  };

  const doReload = async () => {
    if (!conversationId) return;
    try {
      await invoke("browser_toolbar_reload", { conversationId });
    } catch {
      /* ignore */
    }
  };

  const doGoForward = async () => {
    if (!conversationId) return;
    try {
      await invoke("browser_toolbar_forward", { conversationId });
    } catch {
      /* ignore */
    }
  };

  // Stealth toggle — rendered in both pre-session and during-session states
  // at the same anchor position so muscle memory carries across. Works even
  // before a session exists (just persists the preference for next launch).
  const stealthToggle = (
    <Button
      size="icon"
      variant={stealth ? "default" : "ghost"}
      onClick={() => void setStealth(!stealth)}
      className="h-8 w-8 shrink-0"
      title={
        stealth
          ? "Stealth mode is ON — browser view is hidden and the screencast is paused. The agent is still running. Click to show the viewport."
          : "Hide the browser viewport (stealth mode). The agent keeps executing, but no frames are streamed — saves CPU and bandwidth."
      }
      aria-label={stealth ? "Show browser viewport" : "Hide browser viewport (stealth mode)"}
    >
      {stealth ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
    </Button>
  );

  return (
    <div className="flex items-center gap-1.5 px-2 py-1.5 border-b border-border bg-card">
      {!sessionActive ? (
        <>
          <div
            className="flex items-center gap-1.5 h-8 px-2 rounded-md border border-border bg-background text-xs"
            title="Named sessions persist login state (cookies, localStorage, IndexedDB) across restarts. Use e.g. 'whatsapp' to scan the QR code once and stay logged in."
          >
            <Bookmark className="w-3.5 h-3.5 text-muted-foreground" />
            <input
              value={profileName}
              onChange={(e) => saveProfileName(e.target.value)}
              placeholder="Session name (optional)"
              className="bg-transparent outline-none text-xs w-32 placeholder:text-muted-foreground"
            />
          </div>
          {cookieSessions.length > 0 && (
            <div
              className="flex items-center gap-1.5 h-8 px-2 rounded-md border border-border bg-background text-xs"
              title="Replay saved cookies on launch"
            >
              <Cookie className="w-3.5 h-3.5 text-muted-foreground" />
              <select
                value={cookieSessionId ?? ""}
                onChange={(e) =>
                  setCookieSessionId(e.target.value ? e.target.value : null)
                }
                className="bg-transparent text-xs outline-none cursor-pointer max-w-[160px] truncate"
              >
                <option value="">No cookies</option>
                {cookieSessions.map((cs) => (
                  <option key={cs.id} value={cs.id}>
                    {cs.name} ({cs.cookie_count})
                  </option>
                ))}
              </select>
            </div>
          )}
          <Button
            size="sm"
            variant="default"
            disabled={busy || !conversationId}
            onClick={() =>
              conversationId &&
              startSession(conversationId, {
                profileName: profileName.trim() || undefined,
              })
            }
            className="h-8"
          >
            <PlayCircle className="w-4 h-4 mr-1.5" />
            Start browser
          </Button>
          <div className="ml-auto">{stealthToggle}</div>
        </>
      ) : (
        <>
          <Button
            size="icon"
            variant="ghost"
            disabled={disabled}
            onClick={() => void doGoBack()}
            className="h-8 w-8"
            title="Back"
          >
            <ArrowLeft className="w-4 h-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            disabled={disabled}
            onClick={() => void doGoForward()}
            className="h-8 w-8"
            title="Forward"
          >
            <ArrowRight className="w-4 h-4" />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            disabled={disabled}
            onClick={() => void doReload()}
            className="h-8 w-8"
            title="Reload"
          >
            <RotateCw className="w-4 h-4" />
          </Button>

          <form onSubmit={submitUrl} className="flex-1 min-w-0">
            <Input
              value={urlInput}
              onChange={(e) => setUrlInput(e.target.value)}
              placeholder="https://example.com"
              className="h-8 text-xs"
              disabled={disabled}
            />
          </form>

          <Button
            size="sm"
            variant={paused ? "default" : "ghost"}
            disabled={disabled}
            onClick={() => void togglePause()}
            className="h-8 gap-1.5"
            title={paused ? "Resume agent" : "Pause agent"}
          >
            {paused ? <Play className="w-4 h-4" /> : <Pause className="w-4 h-4" />}
            <span className="text-xs hidden sm:inline">
              {paused ? "Resume" : "Pause"}
            </span>
          </Button>

          <Button
            size="sm"
            variant={takeover ? "default" : "ghost"}
            disabled={disabled}
            onClick={() => void setTakeover(!takeover)}
            className="h-8 gap-1.5"
            title={takeover ? "Return control to agent" : "Take control"}
          >
            {takeover ? (
              <Hand className="w-4 h-4" />
            ) : (
              <MousePointer2 className="w-4 h-4" />
            )}
            <span className="text-xs hidden sm:inline">
              {takeover ? "Takeover" : "Agent"}
            </span>
          </Button>

          <Button
            size="icon"
            variant="ghost"
            disabled={disabled || stealth}
            onClick={() => void (streaming ? stopScreencast() : startScreencast())}
            className="h-8 w-8"
            title={
              stealth
                ? "Streaming is paused while stealth mode is on"
                : streaming
                  ? "Stop streaming"
                  : "Start streaming"
            }
          >
            <div
              className={`w-2 h-2 rounded-full ${
                streaming ? "bg-emerald-400 animate-pulse" : "bg-muted-foreground/40"
              }`}
            />
          </Button>

          {stealthToggle}

          <Button
            size="icon"
            variant="ghost"
            disabled={busy}
            onClick={() => void stopSession()}
            className="h-8 w-8 text-destructive hover:text-destructive"
            title="Stop session"
          >
            <Power className="w-4 h-4" />
          </Button>
        </>
      )}
    </div>
  );
}
