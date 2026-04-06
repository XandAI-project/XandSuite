import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  ScrollView,
  Switch,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { useConnectionStore } from "../../stores/connectionStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { AppSettings } from "../../lib/types";

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <View className="mb-5">
      <Text className="text-muted text-xs font-semibold uppercase tracking-widest mb-2 px-1">
        {title}
      </Text>
      <View className="bg-surface border border-border rounded-2xl overflow-hidden">
        {children}
      </View>
    </View>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <View className="flex-row items-center justify-between px-4 py-3.5 border-b border-border/50 last:border-0">
      <Text className="text-foreground text-sm flex-1 mr-4">{label}</Text>
      {children}
    </View>
  );
}

function SwitchRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <Row label={label}>
      <Switch
        value={value}
        onValueChange={onChange}
        trackColor={{ false: "#313244", true: "#cba6f7" }}
        thumbColor="#1e1e2e"
      />
    </Row>
  );
}

function TextRow({
  label,
  value,
  onChange,
  placeholder,
  keyboardType,
  secureTextEntry,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  keyboardType?: "default" | "url" | "numeric" | "decimal-pad";
  secureTextEntry?: boolean;
}) {
  return (
    <Row label={label}>
      <TextInput
        className="text-foreground text-sm text-right bg-transparent"
        style={{ maxWidth: 200 }}
        value={value}
        onChangeText={onChange}
        placeholder={placeholder}
        placeholderTextColor="#6c7086"
        keyboardType={keyboardType}
        secureTextEntry={secureTextEntry}
        autoCapitalize="none"
        autoCorrect={false}
      />
    </Row>
  );
}

export default function SettingsScreen() {
  const { settings, fetchSettings, saveSettings, isSaving } = useSettingsStore();
  const { host, token, setHost, disconnect } = useConnectionStore();
  const [draft, setDraft] = useState<AppSettings | null>(null);
  const [connHost, setConnHost] = useState(host || "");
  const [connToken, setConnToken] = useState(token || "");

  useEffect(() => {
    fetchSettings();
  }, []);

  useEffect(() => {
    if (settings) setDraft({ ...settings });
  }, [settings]);

  if (!draft) {
    return (
      <View className="flex-1 bg-background items-center justify-center">
        <ActivityIndicator color="#cba6f7" />
      </View>
    );
  }

  const update = (key: keyof AppSettings, value: unknown) =>
    setDraft((d) => (d ? { ...d, [key]: value } : d));

  const handleSave = async () => {
    if (!draft) return;
    await saveSettings(draft);
    Alert.alert("Saved", "Settings saved successfully.");
  };

  const handleUpdateConnection = async () => {
    await setHost(connHost.trim(), connToken.trim() || null);
  };

  const handleDisconnect = () => {
    Alert.alert("Disconnect?", "You will be taken to the connect screen.", [
      { text: "Cancel", style: "cancel" },
      { text: "Disconnect", style: "destructive", onPress: disconnect },
    ]);
  };

  return (
    <View className="flex-1 bg-background">
      <View className="px-4 pt-14 pb-4 border-b border-border">
        <Text className="text-foreground text-xl font-semibold">Settings</Text>
      </View>

      <ScrollView className="flex-1" contentContainerClassName="px-4 py-4">
        {/* Connection section */}
        <Section title="Connection">
          <TextRow
            label="Host"
            value={connHost}
            onChange={setConnHost}
            placeholder="http://192.168.1.100:3847"
            keyboardType="url"
          />
          <TextRow
            label="API Token"
            value={connToken}
            onChange={setConnToken}
            placeholder="Optional"
            secureTextEntry
          />
          <Row label="">
            <View className="flex-row gap-3 flex-1 justify-end">
              <TouchableOpacity
                onPress={handleUpdateConnection}
                className="bg-primary/20 border border-primary/30 rounded-xl px-4 py-2"
              >
                <Text className="text-primary text-sm font-medium">Update</Text>
              </TouchableOpacity>
              <TouchableOpacity
                onPress={handleDisconnect}
                className="bg-destructive/20 border border-destructive/30 rounded-xl px-4 py-2"
              >
                <Text className="text-destructive text-sm font-medium">Disconnect</Text>
              </TouchableOpacity>
            </View>
          </Row>
        </Section>

        {/* Engine section */}
        <Section title="Engine">
          <TextRow
            label="Remote server URL"
            value={draft.remote_server_url || ""}
            onChange={(v) => update("remote_server_url", v)}
            placeholder="http://localhost:11434"
            keyboardType="url"
          />
          <TextRow
            label="Remote API key"
            value={draft.remote_api_key || ""}
            onChange={(v) => update("remote_api_key", v)}
            placeholder="sk-…"
            secureTextEntry
          />
          <TextRow
            label="Max response tokens"
            value={String(draft.max_response_tokens ?? "")}
            onChange={(v) => update("max_response_tokens", Number(v) || 0)}
            keyboardType="numeric"
          />
          <TextRow
            label="Default engine mode"
            value={draft.default_engine_mode || ""}
            onChange={(v) => update("default_engine_mode", v)}
            placeholder="local / remote"
          />
        </Section>

        {/* Features */}
        <Section title="Features">
          <SwitchRow
            label="Memory enabled"
            value={!!draft.memory_enabled}
            onChange={(v) => update("memory_enabled", v)}
          />
          <SwitchRow
            label="Code execution"
            value={!!draft.enable_code_execution}
            onChange={(v) => update("enable_code_execution", v)}
          />
        </Section>

        {/* Reasoning */}
        <Section title="Reasoning">
          <SwitchRow
            label="Enable thinking"
            value={!!draft.enable_thinking}
            onChange={(v) => update("enable_thinking", v)}
          />
          <TextRow
            label="Thinking budget (tokens)"
            value={String(draft.thinking_budget_tokens ?? "")}
            onChange={(v) => update("thinking_budget_tokens", Number(v) || 0)}
            keyboardType="numeric"
          />
          <TextRow
            label="Reasoning format"
            value={draft.reasoning_format || ""}
            onChange={(v) => update("reasoning_format", v)}
            placeholder="auto / chain-of-thought"
          />
        </Section>

        {/* ComfyUI */}
        <Section title="ComfyUI">
          <TextRow
            label="URL"
            value={draft.comfyui_url || ""}
            onChange={(v) => update("comfyui_url", v)}
            placeholder="http://127.0.0.1:8188"
            keyboardType="url"
          />
          <TextRow
            label="Model"
            value={draft.comfyui_model || ""}
            onChange={(v) => update("comfyui_model", v)}
            placeholder="v1-5-pruned.safetensors"
          />
          <TextRow
            label="VAE"
            value={draft.comfyui_vae_name || ""}
            onChange={(v) => update("comfyui_vae_name", v)}
            placeholder="vae-ft-mse-840000.safetensors"
          />
        </Section>

        {/* Local server */}
        <Section title="Local Server">
          <TextRow
            label="Context size"
            value={String(draft.server_context_size ?? "")}
            onChange={(v) => update("server_context_size", Number(v) || 0)}
            keyboardType="numeric"
          />
          <TextRow
            label="GPU layers"
            value={String(draft.n_gpu_layers ?? "")}
            onChange={(v) => update("n_gpu_layers", Number(v) || 0)}
            keyboardType="numeric"
          />
          <TextRow
            label="Server threads"
            value={String(draft.server_threads ?? "")}
            onChange={(v) => update("server_threads", Number(v) || 0)}
            keyboardType="numeric"
          />
          <SwitchRow
            label="Flash attention"
            value={!!draft.flash_attention}
            onChange={(v) => update("flash_attention", v)}
          />
          <SwitchRow
            label="Memory map (mmap)"
            value={!!draft.use_mmap}
            onChange={(v) => update("use_mmap", v)}
          />
        </Section>

        {/* Save button */}
        <TouchableOpacity
          onPress={handleSave}
          disabled={isSaving}
          className={`rounded-2xl py-4 items-center mb-8 ${isSaving ? "bg-primary/60" : "bg-primary"}`}
        >
          {isSaving ? (
            <ActivityIndicator color="#1e1e2e" />
          ) : (
            <Text className="text-background font-semibold text-base">Save Settings</Text>
          )}
        </TouchableOpacity>
      </ScrollView>
    </View>
  );
}
