import { useEffect, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  FlatList,
  ScrollView,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { ChevronLeft, Cpu, Download, Play, RefreshCw, Server, Trash2, Wifi } from "lucide-react-native";
import { useModelStore } from "../../../stores/modelStore";
import { useServerStore } from "../../../stores/serverStore";
import { formatBytes } from "../../../lib/utils";

export default function ModelsScreen() {
  const router = useRouter();
  const {
    hfModels, downloadedModels, downloads, engineLoaded,
    fetchHfModels, fetchDownloaded, checkEngine, loadModel, connectRemote, deleteModel,
  } = useModelStore();
  const { running, model: activeModel, fetchStatus, startServer, stopServer } = useServerStore();

  const [searchQuery, setSearchQuery] = useState("");
  const [remoteUrl, setRemoteUrl] = useState("");
  const [tab, setTab] = useState<"downloaded" | "browse" | "remote">("downloaded");

  useEffect(() => {
    fetchDownloaded();
    checkEngine();
    fetchStatus();
  }, []);

  const handleBrowse = () => {
    fetchHfModels(searchQuery || undefined);
    setTab("browse");
  };

  const handleLoad = async (id: string) => {
    await loadModel(id);
    await checkEngine();
  };

  const handleConnectRemote = async () => {
    if (!remoteUrl.trim()) return;
    await connectRemote(remoteUrl.trim());
    await checkEngine();
  };

  const handleDelete = (id: string) => {
    Alert.alert("Delete model?", "This will delete the downloaded file.", [
      { text: "Cancel", style: "cancel" },
      { text: "Delete", style: "destructive", onPress: () => deleteModel(id) },
    ]);
  };

  return (
    <View className="flex-1 bg-background">
      {/* Header */}
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Models</Text>
      </View>

      <ScrollView className="flex-1" contentContainerClassName="p-4 gap-4">
        {/* Server Status */}
        <View className="bg-surface border border-border rounded-2xl p-4 gap-3">
          <View className="flex-row items-center justify-between">
            <View className="flex-row items-center gap-2">
              <Server size={16} color="#cba6f7" />
              <Text className="text-foreground font-semibold">Local Server</Text>
            </View>
            <View className={`px-2 py-1 rounded-full ${running ? "bg-green/20" : "bg-surface"}`}>
              <Text className={`text-xs font-medium ${running ? "text-green" : "text-muted"}`}>
                {running ? "Running" : "Stopped"}
              </Text>
            </View>
          </View>
          {running && activeModel && (
            <Text className="text-muted text-sm" numberOfLines={1}>Model: {activeModel}</Text>
          )}
          {engineLoaded && (
            <View className="flex-row items-center gap-2">
              <Cpu size={14} color="#a6e3a1" />
              <Text className="text-green text-sm">Engine loaded</Text>
            </View>
          )}
          <View className="flex-row gap-2">
            {running ? (
              <TouchableOpacity
                onPress={stopServer}
                className="flex-1 bg-destructive/20 border border-destructive/30 rounded-xl py-2.5 items-center"
              >
                <Text className="text-destructive font-medium text-sm">Stop Server</Text>
              </TouchableOpacity>
            ) : (
              <TouchableOpacity
                onPress={startServer}
                className="flex-1 bg-primary/20 border border-primary/30 rounded-xl py-2.5 items-center"
              >
                <Text className="text-primary font-medium text-sm">Start Server</Text>
              </TouchableOpacity>
            )}
            <TouchableOpacity
              onPress={fetchStatus}
              className="w-10 h-10 bg-surface border border-border rounded-xl items-center justify-center"
            >
              <RefreshCw size={16} color="#6c7086" />
            </TouchableOpacity>
          </View>
        </View>

        {/* Tab selector */}
        <View className="flex-row bg-surface border border-border rounded-2xl overflow-hidden">
          {(["downloaded", "browse", "remote"] as const).map((t) => (
            <TouchableOpacity
              key={t}
              onPress={() => setTab(t)}
              className={`flex-1 py-3 items-center ${tab === t ? "bg-primary" : ""}`}
            >
              <Text className={`text-sm font-medium ${tab === t ? "text-background" : "text-muted"}`}>
                {t === "downloaded" ? "Downloaded" : t === "browse" ? "Browse HF" : "Remote"}
              </Text>
            </TouchableOpacity>
          ))}
        </View>

        {/* Downloaded models */}
        {tab === "downloaded" && (
          <View className="gap-2">
            {downloadedModels.length === 0 ? (
              <Text className="text-muted text-center py-8">No downloaded models</Text>
            ) : (
              downloadedModels.map((m) => (
                <View key={m.id} className="bg-surface border border-border rounded-2xl p-4 gap-2">
                  <Text className="text-foreground font-medium" numberOfLines={1}>{m.name || m.id}</Text>
                  <Text className="text-muted text-xs">{formatBytes(m.size_bytes || 0)}</Text>
                  <View className="flex-row gap-2 mt-1">
                    <TouchableOpacity
                      onPress={() => handleLoad(m.id)}
                      className="flex-1 bg-primary/20 border border-primary/30 rounded-xl py-2 items-center flex-row justify-center gap-1"
                    >
                      <Play size={12} color="#cba6f7" />
                      <Text className="text-primary text-sm font-medium">Load</Text>
                    </TouchableOpacity>
                    <TouchableOpacity
                      onPress={() => handleDelete(m.id)}
                      className="w-10 h-9 bg-destructive/10 border border-destructive/30 rounded-xl items-center justify-center"
                    >
                      <Trash2 size={14} color="#f38ba8" />
                    </TouchableOpacity>
                  </View>
                  {downloads[m.id] && (
                    <View className="gap-1">
                      <View className="h-1.5 bg-border rounded-full overflow-hidden">
                        <View
                          className="h-full bg-primary rounded-full"
                          style={{ width: `${Math.round((downloads[m.id].downloaded_bytes / (downloads[m.id].total_bytes || 1)) * 100)}%` }}
                        />
                      </View>
                      <Text className="text-muted text-xs">
                        {formatBytes(downloads[m.id].downloaded_bytes)} / {formatBytes(downloads[m.id].total_bytes || 0)} · {downloads[m.id].status}
                      </Text>
                    </View>
                  )}
                </View>
              ))
            )}
          </View>
        )}

        {/* Browse HF */}
        {tab === "browse" && (
          <View className="gap-3">
            <View className="flex-row gap-2">
              <TextInput
                className="flex-1 bg-surface border border-border rounded-xl px-4 py-3 text-foreground text-sm"
                value={searchQuery}
                onChangeText={setSearchQuery}
                placeholder="Search Hugging Face…"
                placeholderTextColor="#6c7086"
                onSubmitEditing={handleBrowse}
                returnKeyType="search"
              />
              <TouchableOpacity
                onPress={handleBrowse}
                className="bg-primary rounded-xl px-4 items-center justify-center"
              >
                <Text className="text-background font-medium text-sm">Go</Text>
              </TouchableOpacity>
            </View>
            {hfModels.map((m) => (
              <View key={m.id} className="bg-surface border border-border rounded-2xl p-4 gap-2">
                <Text className="text-foreground font-medium" numberOfLines={1}>{m.name || m.id}</Text>
                {m.description && (
                  <Text className="text-muted text-xs" numberOfLines={2}>{m.description}</Text>
                )}
                <TouchableOpacity
                  onPress={() => handleLoad(m.id)}
                  className="bg-primary/20 border border-primary/30 rounded-xl py-2.5 items-center flex-row justify-center gap-2 mt-1"
                >
                  <Download size={14} color="#cba6f7" />
                  <Text className="text-primary text-sm font-medium">Download & Load</Text>
                </TouchableOpacity>
              </View>
            ))}
          </View>
        )}

        {/* Remote server */}
        {tab === "remote" && (
          <View className="gap-3">
            <Text className="text-muted text-sm">
              Connect to an OpenAI-compatible API endpoint (e.g. Ollama, LM Studio, OpenAI).
            </Text>
            <TextInput
              className="bg-surface border border-border rounded-xl px-4 py-3 text-foreground text-sm"
              value={remoteUrl}
              onChangeText={setRemoteUrl}
              placeholder="http://localhost:11434"
              placeholderTextColor="#6c7086"
              autoCapitalize="none"
              keyboardType="url"
            />
            <TouchableOpacity
              onPress={handleConnectRemote}
              className="bg-primary rounded-xl py-3.5 items-center flex-row justify-center gap-2"
            >
              <Wifi size={16} color="#1e1e2e" />
              <Text className="text-background font-semibold">Connect Remote</Text>
            </TouchableOpacity>
          </View>
        )}
      </ScrollView>
    </View>
  );
}
