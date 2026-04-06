import AsyncStorage from "@react-native-async-storage/async-storage";
import { STORAGE_KEYS } from "./client";
import { ApiEvent } from "../lib/types";

type EventHandler = (event: ApiEvent) => void;
type RawHandler = (data: string) => void;

interface HandlerMap {
  [eventType: string]: Set<EventHandler>;
}

class SseManager {
  private handlers: HandlerMap = {};
  private rawHandlers: Set<RawHandler> = new Set();
  private eventSource: EventSource | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectDelay = 1000;
  private maxReconnectDelay = 30000;
  private conversationId: string | null = null;
  private connected = false;
  private shouldConnect = false;

  on(eventType: ApiEvent["type"], handler: EventHandler): () => void {
    if (!this.handlers[eventType]) {
      this.handlers[eventType] = new Set();
    }
    this.handlers[eventType].add(handler);
    return () => this.off(eventType, handler);
  }

  onRaw(handler: RawHandler): () => void {
    this.rawHandlers.add(handler);
    return () => this.rawHandlers.delete(handler);
  }

  off(eventType: string, handler: EventHandler) {
    this.handlers[eventType]?.delete(handler);
  }

  setConversationId(id: string | null) {
    if (this.conversationId !== id) {
      this.conversationId = id;
      if (this.shouldConnect) {
        this.disconnect();
        this.connect();
      }
    }
  }

  async connect(convId?: string) {
    if (convId !== undefined) this.conversationId = convId;
    this.shouldConnect = true;
    await this._openConnection();
  }

  private async _openConnection() {
    this.disconnect();

    const host = (await AsyncStorage.getItem(STORAGE_KEYS.HOST)) ||
      process.env.EXPO_PUBLIC_DEFAULT_HOST ||
      "http://localhost:3847";
    const token = await AsyncStorage.getItem(STORAGE_KEYS.TOKEN);

    let url = `${host}/api/events`;
    const params: string[] = [];
    if (this.conversationId) params.push(`conversation_id=${encodeURIComponent(this.conversationId)}`);
    if (token) params.push(`token=${encodeURIComponent(token)}`);
    if (params.length) url += "?" + params.join("&");

    try {
      // React Native doesn't have native EventSource, but expo ships a polyfill.
      // The polyfill is available globally via expo.
      const ES = (globalThis as Record<string, unknown>).EventSource as typeof EventSource;
      if (!ES) {
        console.warn("[SSE] EventSource not available");
        return;
      }

      this.eventSource = new ES(url);

      this.eventSource.onmessage = (e) => {
        this.reconnectDelay = 1000; // reset backoff on message
        const rawData = e.data as string;
        this.rawHandlers.forEach((h) => h(rawData));
        try {
          const event = JSON.parse(rawData) as ApiEvent;
          const typeHandlers = this.handlers[event.type];
          typeHandlers?.forEach((h) => h(event));
          // Also dispatch to wildcard handlers
          this.handlers["*"]?.forEach((h) => h(event));
        } catch {
          // ignore parse errors
        }
      };

      this.eventSource.onopen = () => {
        this.connected = true;
        this.reconnectDelay = 1000;
        console.log("[SSE] Connected");
      };

      this.eventSource.onerror = () => {
        this.connected = false;
        this.disconnect();
        if (this.shouldConnect) {
          console.log(`[SSE] Reconnecting in ${this.reconnectDelay}ms...`);
          this.reconnectTimer = setTimeout(() => {
            this.reconnectDelay = Math.min(this.reconnectDelay * 2, this.maxReconnectDelay);
            this._openConnection();
          }, this.reconnectDelay);
        }
      };
    } catch (err) {
      console.error("[SSE] Failed to open:", err);
    }
  }

  disconnect() {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.eventSource) {
      this.eventSource.close();
      this.eventSource = null;
    }
    this.connected = false;
  }

  stop() {
    this.shouldConnect = false;
    this.disconnect();
  }

  isConnected() {
    return this.connected;
  }
}

export const sseManager = new SseManager();
