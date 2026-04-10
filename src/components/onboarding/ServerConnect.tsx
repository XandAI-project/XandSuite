import { useEffect, useState } from "react";
import {
  Server,
  Wifi,
  WifiOff,
  KeyRound,
  Globe,
  Download,
  Monitor,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  setServerConfig,
  testServerConnection,
  DEFAULT_PORT,
} from "@/lib/serverConfig";

interface Props {
  onConnected: () => void;
}

interface InstallerInfo {
  filename: string;
  platform: string;
  size_bytes: number;
  download_url: string;
}

interface AvailableInstallers {
  version: string;
  installers: InstallerInfo[];
}

function detectPlatform(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("win")) return "windows";
  if (ua.includes("mac") || ua.includes("darwin")) return "macos";
  if (ua.includes("linux")) return "linux";
  return "windows";
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
}

const platformLabels: Record<string, string> = {
  windows: "Windows",
  macos: "macOS",
  linux: "Linux",
};

const platformIcons: Record<string, string> = {
  windows: "🪟",
  macos: "🍎",
  linux: "🐧",
};

export function ServerConnect({ onConnected }: Props) {
  const [url, setUrl] = useState(`http://localhost:${DEFAULT_PORT}`);
  const [token, setToken] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Installer download state
  const [installers, setInstallers] = useState<AvailableInstallers | null>(null);
  const [showConnect, setShowConnect] = useState(false);

  useEffect(() => {
    // Try to fetch available installers from the current origin
    // (works when the user visits the server URL directly)
    fetch("/api/download")
      .then((r) => (r.ok ? r.json() : null))
      .then((data: AvailableInstallers | null) => {
        if (data && data.installers.length > 0) {
          setInstallers(data);
        }
      })
      .catch(() => {
        // Not served from a XandSuite backend — that's fine
      });
  }, []);

  async function handleConnect() {
    setError(null);
    setLoading(true);
    try {
      await testServerConnection(url, token || null);
      setServerConfig(url, token || null);
      onConnected();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : "Connection failed.");
    } finally {
      setLoading(false);
    }
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter") handleConnect();
  }

  const userPlatform = detectPlatform();
  const primaryInstaller = installers?.installers.find(
    (i) => i.platform === userPlatform
  );
  const otherInstallers = installers?.installers.filter(
    (i) => i.platform !== userPlatform
  );

  // If we have installers and the user hasn't clicked "Connect to server",
  // show the download-first landing page.
  if (installers && !showConnect) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-md">
        <div className="glass w-full max-w-lg rounded-2xl p-8 shadow-2xl flex flex-col gap-6">
          {/* Header */}
          <div className="flex flex-col items-center gap-3 text-center">
            <div className="flex h-14 w-14 items-center justify-center rounded-2xl glass-primary">
              <Monitor className="h-7 w-7 text-white" />
            </div>
            <div>
              <h1 className="text-xl font-semibold">XandSuite</h1>
              <p className="text-sm text-muted-foreground mt-1">
                Local AI assistant — download the desktop app or connect via
                browser.
              </p>
              {installers.version && (
                <p className="text-xs text-muted-foreground mt-0.5">
                  v{installers.version}
                </p>
              )}
            </div>
          </div>

          {/* Primary download */}
          {primaryInstaller && (
            <a
              href={primaryInstaller.download_url}
              className="block"
              download
            >
              <Button className="w-full h-12 text-base gap-2">
                <Download className="h-5 w-5" />
                Download for {platformLabels[userPlatform] ?? userPlatform}
                <span className="text-xs opacity-70 ml-1">
                  ({formatBytes(primaryInstaller.size_bytes)})
                </span>
              </Button>
            </a>
          )}

          {/* Other platforms */}
          {otherInstallers && otherInstallers.length > 0 && (
            <div className="flex items-center justify-center gap-3">
              <span className="text-xs text-muted-foreground">
                Also available:
              </span>
              {otherInstallers.map((inst) => (
                <a
                  key={inst.filename}
                  href={inst.download_url}
                  download
                  className="text-xs text-blue-400 hover:text-blue-300 underline underline-offset-2 flex items-center gap-1"
                >
                  <span>{platformIcons[inst.platform] ?? ""}</span>
                  {platformLabels[inst.platform] ?? inst.platform}
                  <span className="opacity-60">
                    ({formatBytes(inst.size_bytes)})
                  </span>
                </a>
              ))}
            </div>
          )}

          {/* Divider */}
          <div className="flex items-center gap-3">
            <div className="flex-1 h-px bg-white/10" />
            <span className="text-xs text-muted-foreground">or</span>
            <div className="flex-1 h-px bg-white/10" />
          </div>

          {/* Use in browser */}
          <Button
            variant="outline"
            className="w-full gap-2"
            onClick={() => setShowConnect(true)}
          >
            <Globe className="h-4 w-4" />
            Use in browser — connect to a remote server
          </Button>
        </div>
      </div>
    );
  }

  // Connect-to-server form
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-md">
      <div className="glass w-full max-w-md rounded-2xl p-8 shadow-2xl flex flex-col gap-6">
        {/* Header */}
        <div className="flex flex-col items-center gap-3 text-center">
          <div className="flex h-14 w-14 items-center justify-center rounded-2xl glass-primary">
            <Server className="h-7 w-7 text-white" />
          </div>
          <div>
            <h1 className="text-xl font-semibold">Connect to XandSuite</h1>
            <p className="text-sm text-muted-foreground mt-1">
              Enter the address of your XandSuite backend server.
            </p>
          </div>
        </div>

        {/* Fields */}
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="sc-url"
              className="text-sm font-medium flex items-center gap-1.5"
            >
              <Globe className="h-3.5 w-3.5" /> Backend URL
            </label>
            <Input
              id="sc-url"
              type="url"
              placeholder={`http://192.168.1.50:${DEFAULT_PORT}`}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={handleKeyDown}
              autoFocus
            />
            <p className="text-xs text-muted-foreground">
              The IP/hostname and port of the server running{" "}
              <code className="bg-white/10 rounded px-1">
                XANDSUITE_HEADLESS=1
              </code>
              .
            </p>
          </div>

          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="sc-token"
              className="text-sm font-medium flex items-center gap-1.5"
            >
              <KeyRound className="h-3.5 w-3.5" /> API Token{" "}
              <span className="text-muted-foreground font-normal">
                (optional)
              </span>
            </label>
            <Input
              id="sc-token"
              type="password"
              placeholder="Leave blank if no token is set"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              onKeyDown={handleKeyDown}
            />
            <p className="text-xs text-muted-foreground">
              Set in Settings → API Token on the server.
            </p>
          </div>
        </div>

        {/* Error */}
        {error && (
          <div className="flex items-start gap-2 rounded-lg bg-destructive/15 border border-destructive/30 p-3 text-sm text-destructive">
            <WifiOff className="h-4 w-4 mt-0.5 shrink-0" />
            <span>{error}</span>
          </div>
        )}

        {/* Action */}
        <Button
          className="w-full"
          onClick={handleConnect}
          disabled={loading || !url}
        >
          {loading ? (
            <span className="flex items-center gap-2">
              <span className="h-4 w-4 animate-spin rounded-full border-2 border-white/30 border-t-white" />
              Connecting…
            </span>
          ) : (
            <span className="flex items-center gap-2">
              <Wifi className="h-4 w-4" /> Connect
            </span>
          )}
        </Button>

        {/* Back to download page */}
        {installers && (
          <button
            className="text-xs text-muted-foreground hover:text-foreground transition-colors text-center"
            onClick={() => setShowConnect(false)}
          >
            ← Back to download page
          </button>
        )}
      </div>
    </div>
  );
}
