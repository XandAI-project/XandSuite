import { useEffect, useState } from "react";
import {
  Alert,
  FlatList,
  Modal,
  ScrollView,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { ChevronLeft, ChevronRight, Plus, RefreshCw, Trash2, Wrench } from "lucide-react-native";
import { useSkillsStore } from "../../../stores/skillsStore";

export default function SkillsScreen() {
  const router = useRouter();
  const { servers, tools, fetchServers, fetchTools } = useSkillsStore();
  const [showAdd, setShowAdd] = useState(false);
  const [serverUrl, setServerUrl] = useState("");
  const [serverName, setServerName] = useState("");
  const [expandedServer, setExpandedServer] = useState<string | null>(null);

  useEffect(() => {
    fetchServers();
    fetchTools();
  }, []);

  return (
    <View className="flex-1 bg-background">
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Skills / MCP</Text>
        <TouchableOpacity
          onPress={() => { fetchServers(); fetchTools(); }}
          className="w-9 h-9 bg-surface border border-border rounded-xl items-center justify-center mr-2"
        >
          <RefreshCw size={16} color="#6c7086" />
        </TouchableOpacity>
        <TouchableOpacity
          onPress={() => setShowAdd(true)}
          className="w-9 h-9 bg-primary rounded-xl items-center justify-center"
        >
          <Plus size={18} color="#1e1e2e" />
        </TouchableOpacity>
      </View>

      <ScrollView className="flex-1" contentContainerClassName="p-4 gap-4">
        {/* Servers */}
        <Text className="text-muted text-xs font-semibold uppercase tracking-widest mb-1">Servers</Text>
        {servers.length === 0 ? (
          <Text className="text-muted text-sm text-center py-4">No MCP servers configured</Text>
        ) : (
          servers.map((srv: { id?: string; name?: string; status?: string; url?: string }) => (
            <View key={srv.id || srv.name} className="bg-surface border border-border rounded-2xl overflow-hidden">
              <TouchableOpacity
                onPress={() => setExpandedServer(expandedServer === srv.id ? null : srv.id || null)}
                className="flex-row items-center px-4 py-3 gap-3"
                activeOpacity={0.75}
              >
                <View className={`w-2 h-2 rounded-full ${srv.status === "running" ? "bg-green" : "bg-muted"}`} />
                <Text className="flex-1 text-foreground font-medium">{srv.name || srv.id}</Text>
                <Text className="text-muted text-xs mr-2">{srv.status || "unknown"}</Text>
                <ChevronRight size={14} color="#6c7086" />
              </TouchableOpacity>
              {expandedServer === srv.id && (
                <View className="px-4 pb-3 gap-2 border-t border-border">
                  {srv.url && <Text className="text-muted text-xs mt-2">URL: {srv.url}</Text>}
                  {/* Tools for this server */}
                  {tools
                    .filter((t: { server_id?: string }) => t.server_id === srv.id)
                    .map((tool: { name: string; description?: string }) => (
                      <View key={tool.name} className="bg-crust rounded-xl px-3 py-2 flex-row items-start gap-2">
                        <Wrench size={12} color="#cba6f7" style={{ marginTop: 2 }} />
                        <View>
                          <Text className="text-foreground text-sm font-medium">{tool.name}</Text>
                          {tool.description && (
                            <Text className="text-muted text-xs" numberOfLines={2}>{tool.description}</Text>
                          )}
                        </View>
                      </View>
                    ))}
                </View>
              )}
            </View>
          ))
        )}

        {/* All tools */}
        <Text className="text-muted text-xs font-semibold uppercase tracking-widest mt-2 mb-1">
          All Tools ({tools.length})
        </Text>
        {tools.map((tool: { name: string; description?: string; server_id?: string }) => (
          <View key={tool.name} className="bg-surface border border-border rounded-xl px-4 py-3 gap-1">
            <Text className="text-foreground text-sm font-medium">{tool.name}</Text>
            {tool.description && (
              <Text className="text-muted text-xs" numberOfLines={2}>{tool.description}</Text>
            )}
            {tool.server_id && (
              <Text className="text-primary text-xs">{tool.server_id}</Text>
            )}
          </View>
        ))}
      </ScrollView>

      {/* Add MCP server modal */}
      <Modal visible={showAdd} transparent animationType="fade" onRequestClose={() => setShowAdd(false)}>
        <View className="flex-1 bg-black/70 items-center justify-center px-6">
          <View className="bg-surface border border-border rounded-2xl p-6 w-full gap-4">
            <Text className="text-foreground text-lg font-semibold">Add MCP Server</Text>
            <TextInput
              className="bg-crust border border-border rounded-xl px-4 py-3 text-foreground text-sm"
              value={serverName}
              onChangeText={setServerName}
              placeholder="Server name"
              placeholderTextColor="#6c7086"
            />
            <TextInput
              className="bg-crust border border-border rounded-xl px-4 py-3 text-foreground text-sm"
              value={serverUrl}
              onChangeText={setServerUrl}
              placeholder="ws://localhost:3000"
              placeholderTextColor="#6c7086"
              autoCapitalize="none"
              keyboardType="url"
            />
            <View className="flex-row gap-3">
              <TouchableOpacity
                onPress={() => setShowAdd(false)}
                className="flex-1 bg-crust border border-border rounded-xl py-3 items-center"
              >
                <Text className="text-muted font-medium">Cancel</Text>
              </TouchableOpacity>
              <TouchableOpacity
                onPress={() => {
                  // TODO: call skillsApi.addServer
                  setShowAdd(false);
                  fetchServers();
                }}
                className="flex-1 bg-primary rounded-xl py-3 items-center"
              >
                <Text className="text-background font-semibold">Add</Text>
              </TouchableOpacity>
            </View>
          </View>
        </View>
      </Modal>
    </View>
  );
}
