import AsyncStorage from "@react-native-async-storage/async-storage";

export const STORAGE_KEYS = {
  HOST: "xand_host",
  TOKEN: "xand_token",
} as const;

async function getConnectionInfo(): Promise<{ host: string; token: string | null }> {
  const [host, token] = await Promise.all([
    AsyncStorage.getItem(STORAGE_KEYS.HOST),
    AsyncStorage.getItem(STORAGE_KEYS.TOKEN),
  ]);
  return {
    host: host || process.env.EXPO_PUBLIC_DEFAULT_HOST || "http://localhost:3847",
    token: token || process.env.EXPO_PUBLIC_DEFAULT_TOKEN || null,
  };
}

export async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  customHeaders?: Record<string, string>
): Promise<T> {
  const { host, token } = await getConnectionInfo();
  const url = `${host}/api${path}`;

  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...customHeaders,
  };

  if (token) {
    headers["Authorization"] = `Bearer ${token}`;
  }

  const response = await fetch(url, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (!response.ok) {
    const text = await response.text().catch(() => response.statusText);
    throw new Error(`HTTP ${response.status}: ${text}`);
  }

  const text = await response.text();
  if (!text) return undefined as unknown as T;
  return JSON.parse(text) as T;
}

export const api = {
  get: <T>(path: string) => request<T>("GET", path),
  post: <T>(path: string, body?: unknown) => request<T>("POST", path, body),
  put: <T>(path: string, body?: unknown) => request<T>("PUT", path, body),
  delete: <T>(path: string) => request<T>("DELETE", path),
};

export async function buildSseUrl(path: string, params?: Record<string, string>): Promise<string> {
  const { host, token } = await getConnectionInfo();
  const qs = params ? "?" + new URLSearchParams(params).toString() : "";
  const url = `${host}/api${path}${qs}`;
  return url;
}

export async function getAuthToken(): Promise<string | null> {
  return AsyncStorage.getItem(STORAGE_KEYS.TOKEN);
}

/** Upload a file via multipart/form-data */
export async function uploadFile(
  path: string,
  fileUri: string,
  filename: string,
  mimeType: string,
  extraFields?: Record<string, string>
): Promise<unknown> {
  const { host, token } = await getConnectionInfo();
  const url = `${host}/api${path}`;

  const form = new FormData();
  // @ts-expect-error React Native accepts this object in FormData
  form.append("file", { uri: fileUri, name: filename, type: mimeType });
  if (extraFields) {
    Object.entries(extraFields).forEach(([k, v]) => form.append(k, v));
  }

  const headers: Record<string, string> = {};
  if (token) headers["Authorization"] = `Bearer ${token}`;

  const response = await fetch(url, {
    method: "POST",
    headers,
    body: form,
  });
  if (!response.ok) throw new Error(`Upload failed: ${response.status}`);
  return response.json();
}
