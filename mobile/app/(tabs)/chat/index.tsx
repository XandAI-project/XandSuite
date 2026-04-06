import { useEffect } from "react";
import {
  ActivityIndicator,
  FlatList,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import { Plus, MessageCircle, Trash2 } from "lucide-react-native";
import { useChatStore } from "../../../stores/chatStore";
import { formatDate, truncate } from "../../../lib/utils";

export default function ConversationList() {
  const { conversations, fetchConversations, selectConversation, createConversation, deleteConversation } =
    useChatStore();
  const router = useRouter();

  useEffect(() => {
    fetchConversations();
  }, []);

  const handleSelect = async (id: string) => {
    await selectConversation(id);
    router.push(`/chat/${id}`);
  };

  const handleNew = async () => {
    await createConversation("New Conversation");
    const id = useChatStore.getState().activeConversationId;
    if (id) router.push(`/chat/${id}`);
  };

  return (
    <View className="flex-1 bg-background">
      {/* Header */}
      <View className="flex-row items-center justify-between px-4 pt-14 pb-4 border-b border-border">
        <Text className="text-foreground text-xl font-semibold">Conversations</Text>
        <TouchableOpacity
          onPress={handleNew}
          className="w-9 h-9 bg-primary rounded-xl items-center justify-center"
          activeOpacity={0.8}
        >
          <Plus size={18} color="#1e1e2e" />
        </TouchableOpacity>
      </View>

      <FlatList
        data={conversations}
        keyExtractor={(item) => item.id}
        contentContainerClassName="py-2"
        ListEmptyComponent={
          <View className="flex-1 items-center justify-center py-20 gap-4">
            <MessageCircle size={48} color="#313244" />
            <Text className="text-muted text-base">No conversations yet</Text>
            <TouchableOpacity
              onPress={handleNew}
              className="bg-primary rounded-xl px-6 py-3"
            >
              <Text className="text-background font-medium">Start chatting</Text>
            </TouchableOpacity>
          </View>
        }
        renderItem={({ item }) => (
          <TouchableOpacity
            onPress={() => handleSelect(item.id)}
            className="flex-row items-center px-4 py-3 border-b border-border/50 active:bg-surface"
            activeOpacity={0.7}
          >
            <View className="flex-1">
              <Text className="text-foreground font-medium" numberOfLines={1}>
                {item.title || "Untitled"}
              </Text>
              <Text className="text-muted text-xs mt-0.5" numberOfLines={1}>
                {item.messages?.length
                  ? truncate(item.messages[item.messages.length - 1]?.content || "", 60)
                  : "No messages"}
              </Text>
            </View>
            <View className="items-end ml-3 gap-1">
              <Text className="text-muted text-xs">
                {formatDate(item.updated_at)}
              </Text>
              <TouchableOpacity
                onPress={() => deleteConversation(item.id)}
                hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}
              >
                <Trash2 size={14} color="#6c7086" />
              </TouchableOpacity>
            </View>
          </TouchableOpacity>
        )}
      />
    </View>
  );
}
