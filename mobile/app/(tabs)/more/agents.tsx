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
import { Brain, ChevronLeft, ChevronRight, Plus, RefreshCw, Trash2, X } from "lucide-react-native";
import { useAgentStore } from "../../../stores/agentStore";
import { AgentTask } from "../../../lib/types";
import { formatDate } from "../../../lib/utils";

const STATUS_COLOR: Record<string, string> = {
  running: "#a6e3a1",
  completed: "#89b4fa",
  failed: "#f38ba8",
  pending: "#fab387",
  cancelled: "#6c7086",
};

export default function AgentsScreen() {
  const router = useRouter();
  const { tasks, activeEvents, fetchTasks, runTask, cancelTask, deleteTask } = useAgentStore();
  const [showNew, setShowNew] = useState(false);
  const [description, setDescription] = useState("");
  const [expandedTask, setExpandedTask] = useState<string | null>(null);

  useEffect(() => {
    fetchTasks();
  }, []);

  const handleRun = async () => {
    if (!description.trim()) return;
    const id = await runTask(description.trim());
    setDescription("");
    setShowNew(false);
    setExpandedTask(id);
  };

  const handleCancel = (id: string) => {
    Alert.alert("Cancel task?", undefined, [
      { text: "No", style: "cancel" },
      { text: "Yes", onPress: () => cancelTask(id) },
    ]);
  };

  const handleDelete = (id: string) => {
    Alert.alert("Delete task?", undefined, [
      { text: "Cancel", style: "cancel" },
      { text: "Delete", style: "destructive", onPress: () => deleteTask(id) },
    ]);
  };

  return (
    <View className="flex-1 bg-background">
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Agents</Text>
        <TouchableOpacity
          onPress={fetchTasks}
          className="w-9 h-9 bg-surface border border-border rounded-xl items-center justify-center mr-2"
        >
          <RefreshCw size={16} color="#6c7086" />
        </TouchableOpacity>
        <TouchableOpacity
          onPress={() => setShowNew(true)}
          className="w-9 h-9 bg-primary rounded-xl items-center justify-center"
        >
          <Plus size={18} color="#1e1e2e" />
        </TouchableOpacity>
      </View>

      <FlatList
        data={tasks}
        keyExtractor={(item) => item.id}
        contentContainerClassName="py-2 px-4"
        ListEmptyComponent={
          <View className="items-center justify-center py-20 gap-3">
            <Brain size={48} color="#313244" />
            <Text className="text-muted text-base">No agent tasks</Text>
            <TouchableOpacity
              onPress={() => setShowNew(true)}
              className="bg-primary rounded-xl px-5 py-2.5"
            >
              <Text className="text-background font-medium">New Task</Text>
            </TouchableOpacity>
          </View>
        }
        renderItem={({ item }: { item: AgentTask }) => {
          const evs = activeEvents[item.id] || [];
          const expanded = expandedTask === item.id;
          return (
            <View className="bg-surface border border-border rounded-2xl mb-3 overflow-hidden">
              <TouchableOpacity
                onPress={() => setExpandedTask(expanded ? null : item.id)}
                className="flex-row items-center px-4 py-3 gap-3"
                activeOpacity={0.75}
              >
                <View
                  className="w-2 h-2 rounded-full"
                  style={{ backgroundColor: STATUS_COLOR[item.status] || "#6c7086" }}
                />
                <View className="flex-1">
                  <Text className="text-foreground font-medium text-sm" numberOfLines={1}>
                    {item.title || item.description || item.id}
                  </Text>
                  <Text className="text-muted text-xs mt-0.5">
                    {item.status} · {formatDate(item.created_at)}
                  </Text>
                </View>
                <View className="flex-row gap-2 items-center">
                  {item.status === "running" && (
                    <TouchableOpacity
                      onPress={() => handleCancel(item.id)}
                      hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}
                    >
                      <X size={16} color="#fab387" />
                    </TouchableOpacity>
                  )}
                  <TouchableOpacity
                    onPress={() => handleDelete(item.id)}
                    hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}
                  >
                    <Trash2 size={16} color="#6c7086" />
                  </TouchableOpacity>
                  <ChevronRight
                    size={14}
                    color="#6c7086"
                    style={{ transform: [{ rotate: expanded ? "90deg" : "0deg" }] }}
                  />
                </View>
              </TouchableOpacity>

              {expanded && (
                <View className="border-t border-border px-4 py-3 gap-2">
                  {item.description && (
                    <Text className="text-muted text-xs mb-2">{item.description}</Text>
                  )}
                  {evs.length === 0 ? (
                    <Text className="text-muted text-xs">No events yet</Text>
                  ) : (
                    evs.slice(-20).map((ev, i) => (
                      <View key={i} className="flex-row gap-2 items-start">
                        <Text className="text-primary text-xs font-mono w-24" numberOfLines={1}>
                          {ev.event_type}
                        </Text>
                        <Text className="flex-1 text-foreground text-xs font-mono leading-4">
                          {typeof ev.payload === "string"
                            ? ev.payload
                            : JSON.stringify(ev.payload)}
                        </Text>
                      </View>
                    ))
                  )}
                </View>
              )}
            </View>
          );
        }}
      />

      {/* New task modal */}
      <Modal visible={showNew} transparent animationType="slide" onRequestClose={() => setShowNew(false)}>
        <View className="flex-1 bg-black/70 justify-end">
          <View className="bg-base border-t border-border rounded-t-3xl p-6 gap-4">
            <View className="flex-row items-center justify-between">
              <Text className="text-foreground text-lg font-semibold">New Agent Task</Text>
              <TouchableOpacity onPress={() => setShowNew(false)}>
                <X size={20} color="#6c7086" />
              </TouchableOpacity>
            </View>
            <TextInput
              className="bg-surface border border-border rounded-2xl px-4 py-3 text-foreground text-sm"
              style={{ minHeight: 100 }}
              value={description}
              onChangeText={setDescription}
              placeholder="Describe the task for the agent to complete…"
              placeholderTextColor="#6c7086"
              multiline
              autoFocus
            />
            <TouchableOpacity
              onPress={handleRun}
              disabled={!description.trim()}
              className={`rounded-2xl py-4 items-center ${description.trim() ? "bg-primary" : "bg-primary/40"}`}
            >
              <Text className="text-background font-semibold">Run Task</Text>
            </TouchableOpacity>
          </View>
        </View>
      </Modal>
    </View>
  );
}
