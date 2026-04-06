import { useEffect } from "react";
import {
  Alert,
  FlatList,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { ChevronLeft, MemoryStick, RefreshCw, Trash2 } from "lucide-react-native";
import { useMemoryStore } from "../../../stores/memoryStore";
import { formatDate } from "../../../lib/utils";

export default function MemoryScreen() {
  const router = useRouter();
  const { entries, fetchEntries, deleteEntry, clearAll } = useMemoryStore();

  useEffect(() => {
    fetchEntries();
  }, []);

  const handleClearAll = () => {
    Alert.alert("Clear all memory?", "All memory entries will be deleted.", [
      { text: "Cancel", style: "cancel" },
      { text: "Clear", style: "destructive", onPress: clearAll },
    ]);
  };

  return (
    <View className="flex-1 bg-background">
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Memory</Text>
        <TouchableOpacity
          onPress={fetchEntries}
          className="w-9 h-9 bg-surface border border-border rounded-xl items-center justify-center mr-2"
        >
          <RefreshCw size={16} color="#6c7086" />
        </TouchableOpacity>
        {entries.length > 0 && (
          <TouchableOpacity
            onPress={handleClearAll}
            className="w-9 h-9 bg-destructive/20 border border-destructive/30 rounded-xl items-center justify-center"
          >
            <Trash2 size={16} color="#f38ba8" />
          </TouchableOpacity>
        )}
      </View>

      <FlatList
        data={entries as { id: string; content?: string; created_at?: string; collection_id?: string }[]}
        keyExtractor={(item) => item.id}
        contentContainerClassName="py-2 px-4"
        ListEmptyComponent={
          <View className="items-center justify-center py-20 gap-3">
            <MemoryStick size={48} color="#313244" />
            <Text className="text-muted text-base">No memory entries</Text>
          </View>
        }
        renderItem={({ item }) => (
          <View className="bg-surface border border-border rounded-2xl px-4 py-3 mb-2 flex-row items-start gap-3">
            <View className="flex-1">
              <Text className="text-foreground text-sm leading-5" numberOfLines={4}>
                {item.content}
              </Text>
              {item.created_at && (
                <Text className="text-muted text-xs mt-1">{formatDate(item.created_at)}</Text>
              )}
            </View>
            <TouchableOpacity
              onPress={() => deleteEntry(item.id)}
              hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}
            >
              <Trash2 size={16} color="#6c7086" />
            </TouchableOpacity>
          </View>
        )}
      />
    </View>
  );
}
