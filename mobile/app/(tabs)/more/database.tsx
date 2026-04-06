import { useEffect, useState } from "react";
import {
  Alert,
  Modal,
  ScrollView,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { ChevronLeft, Database, Play, Plus, Trash2, X } from "lucide-react-native";
import { databaseApi } from "../../../api/endpoints";
import { DbConnection } from "../../../lib/types";

export default function DatabaseScreen() {
  const router = useRouter();
  const [connections, setConnections] = useState<DbConnection[]>([]);
  const [activeConn, setActiveConn] = useState<DbConnection | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<unknown>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [showAdd, setShowAdd] = useState(false);
  const [newName, setNewName] = useState("");
  const [newUrl, setNewUrl] = useState("");
  const [newType, setNewType] = useState("postgres");

  const fetchConnections = async () => {
    const conns = await databaseApi.list();
    setConnections(conns);
  };

  useEffect(() => {
    fetchConnections();
  }, []);

  const handleAdd = async () => {
    if (!newName.trim() || !newUrl.trim()) return;
    await databaseApi.add(newName.trim(), newUrl.trim(), newType);
    await fetchConnections();
    setShowAdd(false);
    setNewName("");
    setNewUrl("");
  };

  const handleDelete = (conn: DbConnection) => {
    Alert.alert("Delete connection?", `"${conn.name}" will be removed.`, [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: async () => {
          await databaseApi.delete(conn.id);
          await fetchConnections();
          if (activeConn?.id === conn.id) setActiveConn(null);
        },
      },
    ]);
  };

  const handleTest = async (conn: DbConnection) => {
    const ok = await databaseApi.test(conn.id);
    Alert.alert(ok ? "Connected" : "Failed", ok ? "Connection successful." : "Could not connect.");
  };

  const handleRunQuery = async () => {
    if (!activeConn || !query.trim()) return;
    setIsRunning(true);
    try {
      const res = await databaseApi.query(activeConn.id, query.trim());
      setResults(res);
    } catch (e) {
      setResults({ error: String(e) });
    } finally {
      setIsRunning(false);
    }
  };

  return (
    <View className="flex-1 bg-background">
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity onPress={() => router.back()}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground text-xl font-semibold">Database</Text>
        <TouchableOpacity
          onPress={() => setShowAdd(true)}
          className="w-9 h-9 bg-primary rounded-xl items-center justify-center"
        >
          <Plus size={18} color="#1e1e2e" />
        </TouchableOpacity>
      </View>

      <ScrollView className="flex-1" contentContainerClassName="p-4 gap-4">
        {/* Connections */}
        <Text className="text-muted text-xs font-semibold uppercase tracking-widest mb-1">Connections</Text>
        {connections.length === 0 ? (
          <View className="items-center py-6 gap-2">
            <Database size={40} color="#313244" />
            <Text className="text-muted text-sm">No database connections</Text>
          </View>
        ) : (
          connections.map((conn) => (
            <View
              key={conn.id}
              className={`bg-surface border rounded-2xl p-4 gap-2 ${
                activeConn?.id === conn.id ? "border-primary" : "border-border"
              }`}
            >
              <View className="flex-row items-center justify-between">
                <TouchableOpacity
                  onPress={() => setActiveConn(activeConn?.id === conn.id ? null : conn)}
                  className="flex-1"
                >
                  <Text className="text-foreground font-medium">{conn.name}</Text>
                  <Text className="text-muted text-xs">{conn.db_type} · {conn.connection_string?.slice(0, 40)}…</Text>
                </TouchableOpacity>
                <View className="flex-row gap-2">
                  <TouchableOpacity
                    onPress={() => handleTest(conn)}
                    className="bg-green/10 border border-green/30 rounded-lg px-3 py-1.5"
                  >
                    <Text className="text-green text-xs font-medium">Test</Text>
                  </TouchableOpacity>
                  <TouchableOpacity onPress={() => handleDelete(conn)} hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}>
                    <Trash2 size={16} color="#f38ba8" />
                  </TouchableOpacity>
                </View>
              </View>
            </View>
          ))
        )}

        {/* Query editor */}
        {activeConn && (
          <View className="bg-surface border border-primary/30 rounded-2xl p-4 gap-3">
            <Text className="text-primary font-semibold text-sm">
              Query: {activeConn.name}
            </Text>
            <TextInput
              className="bg-crust border border-border rounded-xl px-3 py-3 text-foreground text-sm font-mono"
              style={{ minHeight: 100 }}
              value={query}
              onChangeText={setQuery}
              placeholder="SELECT * FROM …"
              placeholderTextColor="#6c7086"
              multiline
              autoCapitalize="none"
              autoCorrect={false}
            />
            <TouchableOpacity
              onPress={handleRunQuery}
              disabled={isRunning || !query.trim()}
              className={`flex-row items-center justify-center gap-2 rounded-xl py-3 ${
                query.trim() && !isRunning ? "bg-primary" : "bg-primary/40"
              }`}
            >
              <Play size={14} color="#1e1e2e" />
              <Text className="text-background font-semibold text-sm">
                {isRunning ? "Running…" : "Run Query"}
              </Text>
            </TouchableOpacity>

            {results !== null && (
              <View className="bg-crust border border-border rounded-xl p-3">
                <Text className="text-muted text-xs font-semibold mb-2">Results</Text>
                <Text className="text-foreground text-xs font-mono leading-5">
                  {JSON.stringify(results, null, 2)}
                </Text>
              </View>
            )}
          </View>
        )}
      </ScrollView>

      {/* Add connection modal */}
      <Modal visible={showAdd} transparent animationType="fade" onRequestClose={() => setShowAdd(false)}>
        <View className="flex-1 bg-black/70 items-center justify-center px-6">
          <View className="bg-surface border border-border rounded-2xl p-6 w-full gap-4">
            <View className="flex-row items-center justify-between">
              <Text className="text-foreground text-lg font-semibold">Add Connection</Text>
              <TouchableOpacity onPress={() => setShowAdd(false)}>
                <X size={20} color="#6c7086" />
              </TouchableOpacity>
            </View>
            <TextInput
              className="bg-crust border border-border rounded-xl px-4 py-3 text-foreground text-sm"
              value={newName}
              onChangeText={setNewName}
              placeholder="Connection name"
              placeholderTextColor="#6c7086"
            />
            <View className="flex-row gap-2">
              {["postgres", "mysql", "sqlite"].map((t) => (
                <TouchableOpacity
                  key={t}
                  onPress={() => setNewType(t)}
                  className={`flex-1 py-2.5 rounded-xl border items-center ${
                    newType === t ? "bg-primary border-primary" : "bg-crust border-border"
                  }`}
                >
                  <Text className={`text-xs font-medium ${newType === t ? "text-background" : "text-muted"}`}>
                    {t}
                  </Text>
                </TouchableOpacity>
              ))}
            </View>
            <TextInput
              className="bg-crust border border-border rounded-xl px-4 py-3 text-foreground text-sm"
              value={newUrl}
              onChangeText={setNewUrl}
              placeholder="postgresql://user:pass@host/db"
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
                onPress={handleAdd}
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
