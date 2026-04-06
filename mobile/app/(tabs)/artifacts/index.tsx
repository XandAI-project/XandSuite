import { useEffect, useState } from "react";
import {
  FlatList,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { Code2, Search } from "lucide-react-native";
import { useArtifactStore } from "../../../stores/artifactStore";
import { Artifact } from "../../../lib/types";
import { artifactIcon, formatDate, truncate } from "../../../lib/utils";

export default function ArtifactsTab() {
  const { allArtifacts, fetchAll } = useArtifactStore();
  const [query, setQuery] = useState("");
  const router = useRouter();

  useEffect(() => {
    fetchAll();
  }, []);

  const filtered = allArtifacts.filter(
    (a) =>
      !query ||
      a.title.toLowerCase().includes(query.toLowerCase()) ||
      a.artifact_type?.toLowerCase().includes(query.toLowerCase())
  );

  return (
    <View className="flex-1 bg-background">
      {/* Header */}
      <View className="px-4 pt-14 pb-3 border-b border-border">
        <Text className="text-foreground text-xl font-semibold mb-3">Artifacts</Text>
        <View className="flex-row items-center bg-surface border border-border rounded-xl px-3 gap-2">
          <Search size={16} color="#6c7086" />
          <TextInput
            className="flex-1 py-2.5 text-foreground text-sm"
            placeholder="Search artifacts…"
            placeholderTextColor="#6c7086"
            value={query}
            onChangeText={setQuery}
          />
        </View>
      </View>

      <FlatList
        data={filtered}
        keyExtractor={(item) => item.id}
        contentContainerClassName="py-2"
        ListEmptyComponent={
          <View className="items-center justify-center py-20 gap-3">
            <Code2 size={48} color="#313244" />
            <Text className="text-muted text-base">No artifacts found</Text>
          </View>
        }
        renderItem={({ item }) => (
          <TouchableOpacity
            onPress={() => router.push(`/artifacts/${item.id}`)}
            className="flex-row items-center px-4 py-3 border-b border-border/50"
            activeOpacity={0.7}
          >
            <View className="w-10 h-10 bg-surface border border-border rounded-xl items-center justify-center mr-3">
              <Text className="text-lg">{artifactIcon(item.artifact_type)}</Text>
            </View>
            <View className="flex-1">
              <Text className="text-foreground font-medium" numberOfLines={1}>
                {item.title}
              </Text>
              <Text className="text-muted text-xs mt-0.5">
                {item.artifact_type || "text"} · {formatDate(item.updated_at)}
              </Text>
            </View>
          </TouchableOpacity>
        )}
      />
    </View>
  );
}
