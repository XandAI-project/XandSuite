import { useEffect, useRef, useState } from "react";
import {
  FlatList,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { ChevronLeft, RefreshCw, Trash2 } from "lucide-react-native";
import { useLogStore } from "../../../stores/logStore";
import { LogEntry } from "../../../lib/types";

const LEVEL_COLORS: Record<string, string> = {
  error: "#f38ba8",
  warn: "#fab387",
  warning: "#fab387",
  info: "#89b4fa",
  debug: "#6c7086",
};

const LEVEL_BG: Record<string, string> = {
  error: "bg-red/10",
  warn: "bg-peach/10",
  warning: "bg-peach/10",
  info: "bg-blue/10",
  debug: "bg-surface",
};

export default function LogsScreen() {
  const router = useRouter();
  const { logs, fetchLogs, clear } = useLogStore();
  const listRef = useRef<FlatList>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => {
    fetchLogs();
  }, []);

  useEffect(() => {
    if (autoScroll && logs.length > 0) {
      listRef.current?.scrollToIndex({ index: 0, animated: false });
    }
  }, [logs]);

  return (
    <View className="flex-1 bg-background">
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Logs</Text>
        <TouchableOpacity
          onPress={fetchLogs}
          className="w-9 h-9 bg-surface border border-border rounded-xl items-center justify-center mr-2"
        >
          <RefreshCw size={16} color="#6c7086" />
        </TouchableOpacity>
        <TouchableOpacity
          onPress={clear}
          className="w-9 h-9 bg-surface border border-border rounded-xl items-center justify-center"
        >
          <Trash2 size={16} color="#6c7086" />
        </TouchableOpacity>
      </View>

      {/* Live indicator */}
      <View className="flex-row items-center justify-between px-4 py-2 bg-surface border-b border-border">
        <View className="flex-row items-center gap-2">
          <View className="w-2 h-2 bg-green rounded-full" />
          <Text className="text-muted text-xs">Live (SSE)</Text>
        </View>
        <Text className="text-muted text-xs">{logs.length} entries</Text>
      </View>

      <FlatList
        ref={listRef}
        data={logs}
        keyExtractor={(_, i) => String(i)}
        contentContainerClassName="py-2 px-3"
        inverted={false}
        onScrollBeginDrag={() => setAutoScroll(false)}
        ListEmptyComponent={
          <Text className="text-muted text-sm text-center py-12">No logs yet</Text>
        }
        renderItem={({ item }: { item: LogEntry }) => (
          <View
            className={`px-3 py-2 rounded-lg mb-0.5 flex-row gap-2 ${
              LEVEL_BG[item.level] || "bg-surface"
            }`}
          >
            <Text
              className="text-xs font-mono font-bold w-12"
              style={{ color: LEVEL_COLORS[item.level] || "#6c7086" }}
            >
              {item.level.toUpperCase().slice(0, 5)}
            </Text>
            <Text className="flex-1 text-foreground text-xs font-mono leading-4">
              {item.message}
            </Text>
            <Text className="text-muted text-xs font-mono">{item.ts?.slice(11, 19)}</Text>
          </View>
        )}
      />
    </View>
  );
}
