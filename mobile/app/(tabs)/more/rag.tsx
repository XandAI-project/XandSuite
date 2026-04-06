import { useEffect, useState } from "react";
import {
  Alert,
  ScrollView,
  Text,
  TextInput,
  TouchableOpacity,
  View,
  ActivityIndicator,
} from "react-native";
import { useRouter } from "expo-router";
import { ChevronLeft, Database, Plus, Search, Trash2, BarChart2, GitBranch, CheckCircle } from "lucide-react-native";
import { useRagStore } from "../../../stores/ragStore";
import { ragApi } from "../../../api/endpoints";
import { RagCollection } from "../../../lib/types";

export default function KnowledgeBaseScreen() {
  const router = useRouter();
  const { collections, searchResults, fetchCollections, createCollection, deleteCollection, ingest, search } =
    useRagStore();

  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState("");
  const [selectedColl, setSelectedColl] = useState<RagCollection | null>(null);
  const [ingestText, setIngestText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [isSwitchingMode, setIsSwitchingMode] = useState(false);

  useEffect(() => {
    fetchCollections();
  }, []);

  const handleCreate = async () => {
    if (!newName.trim()) return;
    await createCollection(newName.trim());
    setNewName("");
    setShowCreate(false);
  };

  const handleDelete = (col: RagCollection) => {
    Alert.alert("Delete Knowledge Base?", `"${col.name}" will be permanently deleted.`, [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: () => deleteCollection(col.id),
      },
    ]);
  };

  const handleIngest = async () => {
    if (!selectedColl || !ingestText.trim()) return;
    await ingest(selectedColl.id, ingestText.trim());
    setIngestText("");
    Alert.alert("Ingested", "Document added to knowledge base.");
  };

  const handleSearch = async () => {
    await search(searchQuery, selectedColl?.id);
  };

  const handleSetMode = async (mode: "hybrid" | "graph") => {
    if (!selectedColl) return;
    setIsSwitchingMode(true);
    try {
      await ragApi.setRetrievalMode(selectedColl.id, mode);
      await fetchCollections();
      setSelectedColl((prev) =>
        prev ? { ...prev, retrieval_mode: mode, graph_indexed: false } : prev
      );
    } catch (e) {
      console.error(e);
    } finally {
      setIsSwitchingMode(false);
    }
  };

  return (
    <View className="flex-1 bg-background">
      {/* Header */}
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Knowledge Bases</Text>
        <TouchableOpacity
          onPress={() => setShowCreate(true)}
          className="w-9 h-9 bg-primary rounded-xl items-center justify-center"
        >
          <Plus size={18} color="#1e1e2e" />
        </TouchableOpacity>
      </View>

      <ScrollView className="flex-1" contentContainerClassName="p-4 gap-4">
        {/* Collections list */}
        <View className="gap-2">
          {collections.length === 0 ? (
            <View className="items-center py-8 gap-3">
              <Database size={40} color="#313244" />
              <Text className="text-muted text-sm">No knowledge bases yet</Text>
            </View>
          ) : (
            collections.map((col) => (
              <TouchableOpacity
                key={col.id}
                onPress={() => setSelectedColl(selectedColl?.id === col.id ? null : col)}
                className={`bg-surface border rounded-2xl p-4 gap-1 ${
                  selectedColl?.id === col.id ? "border-primary" : "border-border"
                }`}
                activeOpacity={0.75}
              >
                <View className="flex-row items-center justify-between">
                  <Text className="text-foreground font-medium flex-1">{col.name}</Text>
                  <View className="flex-row items-center gap-2">
                    {/* Retrieval mode badge */}
                    {col.retrieval_mode === "graph" ? (
                      <View className="flex-row items-center gap-1 bg-purple-500/20 rounded-lg px-2 py-0.5">
                        <GitBranch size={10} color="#cba6f7" />
                        <Text className="text-[10px] text-purple-300 font-medium">Graph</Text>
                        {!col.graph_indexed && (
                          <ActivityIndicator size={8} color="#f9e2af" />
                        )}
                        {col.graph_indexed && (
                          <CheckCircle size={10} color="#a6e3a1" />
                        )}
                      </View>
                    ) : (
                      <View className="flex-row items-center gap-1 bg-blue-500/10 rounded-lg px-2 py-0.5">
                        <BarChart2 size={10} color="#89b4fa" />
                        <Text className="text-[10px] text-blue-300 font-medium">Hybrid</Text>
                      </View>
                    )}
                    <TouchableOpacity
                      onPress={() => handleDelete(col)}
                      hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}
                    >
                      <Trash2 size={16} color="#f38ba8" />
                    </TouchableOpacity>
                  </View>
                </View>
                {col.description && (
                  <Text className="text-muted text-xs">{col.description}</Text>
                )}
                <Text className="text-muted text-xs">{col.document_count} chunks</Text>
              </TouchableOpacity>
            ))
          )}
        </View>

        {/* Ingest / Search / Mode control when a knowledge base is selected */}
        {selectedColl && (
          <View className="bg-surface border border-primary/30 rounded-2xl p-4 gap-4">
            <Text className="text-primary font-semibold">
              {selectedColl.name}
            </Text>

            {/* Retrieval mode selector */}
            <View className="gap-1">
              <Text className="text-muted text-xs font-semibold uppercase tracking-wide">
                Retrieval mode
              </Text>
              <View className="flex-row gap-2">
                <TouchableOpacity
                  onPress={() => handleSetMode("hybrid")}
                  disabled={isSwitchingMode}
                  className={`flex-1 flex-row items-center justify-center gap-1.5 rounded-xl py-2.5 ${
                    selectedColl.retrieval_mode !== "graph"
                      ? "bg-primary"
                      : "bg-crust border border-border"
                  }`}
                >
                  <BarChart2 size={13} color={selectedColl.retrieval_mode !== "graph" ? "#1e1e2e" : "#6c7086"} />
                  <Text className={`text-sm font-medium ${selectedColl.retrieval_mode !== "graph" ? "text-background" : "text-muted"}`}>
                    Hybrid
                  </Text>
                </TouchableOpacity>
                <TouchableOpacity
                  onPress={() => handleSetMode("graph")}
                  disabled={isSwitchingMode}
                  className={`flex-1 flex-row items-center justify-center gap-1.5 rounded-xl py-2.5 ${
                    selectedColl.retrieval_mode === "graph"
                      ? "bg-purple-600"
                      : "bg-crust border border-border"
                  }`}
                >
                  {isSwitchingMode ? (
                    <ActivityIndicator size={13} color="#cba6f7" />
                  ) : (
                    <GitBranch size={13} color={selectedColl.retrieval_mode === "graph" ? "#ffffff" : "#6c7086"} />
                  )}
                  <Text className={`text-sm font-medium ${selectedColl.retrieval_mode === "graph" ? "text-white" : "text-muted"}`}>
                    Graph
                  </Text>
                </TouchableOpacity>
              </View>
              {selectedColl.retrieval_mode === "graph" && !selectedColl.graph_indexed && (
                <View className="flex-row items-center gap-1.5 mt-1">
                  <ActivityIndicator size={12} color="#f9e2af" />
                  <Text className="text-yellow-300 text-xs">Indexing into graph — search will improve as indexing completes</Text>
                </View>
              )}
            </View>

            {/* Ingest */}
            <View className="gap-2">
              <Text className="text-muted text-xs font-semibold uppercase tracking-wide">
                Ingest document
              </Text>
              <TextInput
                className="bg-crust border border-border rounded-xl px-3 py-3 text-foreground text-sm"
                value={ingestText}
                onChangeText={setIngestText}
                placeholder="Paste document text here…"
                placeholderTextColor="#6c7086"
                multiline
                style={{ minHeight: 80 }}
              />
              <TouchableOpacity
                onPress={handleIngest}
                className="bg-primary/20 border border-primary/30 rounded-xl py-2.5 items-center"
              >
                <Text className="text-primary font-medium text-sm">Ingest</Text>
              </TouchableOpacity>
            </View>

            {/* Search */}
            <View className="gap-2">
              <Text className="text-muted text-xs font-semibold uppercase tracking-wide">
                Search
              </Text>
              <View className="flex-row gap-2">
                <TextInput
                  className="flex-1 bg-crust border border-border rounded-xl px-3 py-3 text-foreground text-sm"
                  value={searchQuery}
                  onChangeText={setSearchQuery}
                  placeholder="Search query…"
                  placeholderTextColor="#6c7086"
                  onSubmitEditing={handleSearch}
                  returnKeyType="search"
                />
                <TouchableOpacity
                  onPress={handleSearch}
                  className="bg-primary rounded-xl px-4 items-center justify-center"
                >
                  <Search size={16} color="#1e1e2e" />
                </TouchableOpacity>
              </View>
              {searchResults.length > 0 && (
                <View className="gap-2 mt-1">
                  {(searchResults as { content?: string; score?: number; entities?: string[] }[]).map((r, i) => (
                    <View key={i} className="bg-crust border border-border/50 rounded-xl px-3 py-2 gap-1">
                      <View className="flex-row items-center justify-between">
                        <Text className="text-muted text-[10px]">Result {i + 1}</Text>
                        {r.score !== undefined && (
                          <View className="bg-primary/10 rounded-lg px-1.5 py-0.5">
                            <Text className="text-primary text-[10px] font-medium">
                              {(r.score * 100).toFixed(0)}% match
                            </Text>
                          </View>
                        )}
                      </View>
                      <Text className="text-foreground text-xs leading-5">{r.content}</Text>
                      {r.entities && r.entities.length > 0 && (
                        <View className="flex-row flex-wrap gap-1 mt-1">
                          {r.entities.slice(0, 4).map((entity, ei) => (
                            <View key={ei} className="bg-purple-500/15 border border-purple-500/20 rounded-lg px-1.5 py-0.5">
                              <Text className="text-[9px] text-purple-300">{entity}</Text>
                            </View>
                          ))}
                        </View>
                      )}
                    </View>
                  ))}
                </View>
              )}
            </View>
          </View>
        )}
      </ScrollView>

      {/* Create knowledge base modal */}
      {showCreate && (
        <View className="absolute inset-0 bg-black/70 items-center justify-center px-6">
          <View className="bg-surface border border-border rounded-2xl p-6 w-full gap-4">
            <Text className="text-foreground text-lg font-semibold">New Knowledge Base</Text>
            <TextInput
              className="bg-crust border border-border rounded-xl px-4 py-3 text-foreground text-sm"
              value={newName}
              onChangeText={setNewName}
              placeholder="e.g. Product Documentation"
              placeholderTextColor="#6c7086"
              autoFocus
            />
            <View className="flex-row gap-3">
              <TouchableOpacity
                onPress={() => setShowCreate(false)}
                className="flex-1 bg-crust border border-border rounded-xl py-3 items-center"
              >
                <Text className="text-muted font-medium">Cancel</Text>
              </TouchableOpacity>
              <TouchableOpacity
                onPress={handleCreate}
                className="flex-1 bg-primary rounded-xl py-3 items-center"
              >
                <Text className="text-background font-semibold">Create</Text>
              </TouchableOpacity>
            </View>
          </View>
        </View>
      )}
    </View>
  );
}
