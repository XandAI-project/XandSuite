/**
 * Remote LLM server URL helpers — the TypeScript mirror of
 * `src-tauri/src/engine/remote.rs::normalize_server_url`. Kept in sync so the
 * UI can show the user the exact address the backend will dial.
 */

const WILDCARD_HOSTS = new Set(["0.0.0.0", "[::]", "[::0]", "[0:0:0:0:0:0:0:0]"]);

function withHttpScheme(raw: string): string {
  const trimmed = raw.trim();
  const withScheme =
    trimmed.startsWith("http://") || trimmed.startsWith("https://")
      ? trimmed
      : `http://${trimmed}`;
  return withScheme.replace(/\/+$/, "");
}

/** Split `scheme://userinfo@host:port/path` into its parts. */
function splitUrl(url: string) {
  const sep = url.indexOf("://");
  if (sep === -1) return null;

  const scheme = url.slice(0, sep);
  const rest = url.slice(sep + 3);
  const pathStart = rest.search(/[/?#]/);
  const authority = pathStart === -1 ? rest : rest.slice(0, pathStart);
  const path = pathStart === -1 ? "" : rest.slice(pathStart);

  const at = authority.lastIndexOf("@");
  const userinfo = at === -1 ? "" : `${authority.slice(0, at)}@`;
  const hostport = at === -1 ? authority : authority.slice(at + 1);

  let host = hostport;
  let port = "";
  if (hostport.startsWith("[")) {
    const close = hostport.indexOf("]");
    if (close !== -1) {
      host = hostport.slice(0, close + 1);
      port = hostport.slice(close + 1);
    }
  } else {
    const colon = hostport.indexOf(":");
    if (colon !== -1) {
      host = hostport.slice(0, colon);
      port = hostport.slice(colon);
    }
  }

  return { scheme, userinfo, host, port, path };
}

/**
 * Add a scheme when missing, drop trailing slashes, and rewrite wildcard bind
 * addresses (`0.0.0.0`, `::`) to loopback — those can never reach a server on
 * another machine.
 */
export function normalizeServerUrl(raw: string): string {
  const url = withHttpScheme(raw);
  const parts = splitUrl(url);
  if (!parts) return url;

  const loopback = WILDCARD_HOSTS.has(parts.host)
    ? parts.host.startsWith("[")
      ? "[::1]"
      : "127.0.0.1"
    : null;
  if (!loopback) return url;

  return `${parts.scheme}://${parts.userinfo}${loopback}${parts.port}${parts.path}`;
}

/** True when the URL names a wildcard bind address instead of a real host. */
export function isWildcardHost(raw: string): boolean {
  const parts = splitUrl(withHttpScheme(raw));
  return !!parts && WILDCARD_HOSTS.has(parts.host);
}
