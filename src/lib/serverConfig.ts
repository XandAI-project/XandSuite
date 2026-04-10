/**
 * Server configuration for web (non-Tauri) mode.
 * Stores the backend URL and optional API token in localStorage so the
 * user only has to enter them once per browser session.
 */

const KEY_URL   = "xand_api_url";
const KEY_TOKEN = "xand_api_token";

/** Default API port matching the backend default (mobile_api_port). */
export const DEFAULT_PORT = 3847;

export function getServerUrl(): string {
  // Allow a compile-time default via VITE_API_URL
  return (
    localStorage.getItem(KEY_URL) ||
    (import.meta.env.VITE_API_URL as string | undefined) ||
    `http://localhost:${DEFAULT_PORT}`
  );
}

export function getServerToken(): string | null {
  return localStorage.getItem(KEY_TOKEN) || null;
}

export function setServerConfig(url: string, token: string | null): void {
  const normalised = url.replace(/\/$/, ""); // strip trailing slash
  localStorage.setItem(KEY_URL, normalised);
  if (token) {
    localStorage.setItem(KEY_TOKEN, token);
  } else {
    localStorage.removeItem(KEY_TOKEN);
  }
}

export function clearServerConfig(): void {
  localStorage.removeItem(KEY_URL);
  localStorage.removeItem(KEY_TOKEN);
}

/** Returns true if a backend URL has been explicitly stored. */
export function hasServerConfig(): boolean {
  return !!(
    localStorage.getItem(KEY_URL) ||
    (import.meta.env.VITE_API_URL as string | undefined)
  );
}

/** Test connectivity: GET /api/settings with the given credentials. */
export async function testServerConnection(
  url: string,
  token: string | null
): Promise<void> {
  const base = url.replace(/\/$/, "");
  const headers: Record<string, string> = {};
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const res = await fetch(`${base}/api/settings`, { headers });
  if (!res.ok) {
    if (res.status === 401) {
      throw new Error("Invalid API token — check your token and try again.");
    }
    throw new Error(`Server returned HTTP ${res.status}. Is the URL correct?`);
  }
}
