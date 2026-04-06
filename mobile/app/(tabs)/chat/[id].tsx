import { useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  FlatList,
  KeyboardAvoidingView,
  Platform,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { useLocalSearchParams, useRouter } from "expo-router";
import { ChevronLeft, MoreVertical } from "lucide-react-native";
import { useChatStore } from "../../../stores/chatStore";
import { useSkillsStore } from "../../../stores/skillsStore";
import { MessageBubble } from "../../../components/chat/MessageBubble";
import { MessageInput } from "../../../components/chat/MessageInput";
import { ToolCallCard } from "../../../components/chat/ToolCallCard";
import { Message } from "../../../lib/types";

export default function ChatDetail() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const router = useRouter();
  const flatListRef = useRef<FlatList>(null);

  const {
    activeConversation,
    selectConversation,
    sendMessage,
    isStreaming,
    streamingToken,
    streamingThinking,
  } = useChatStore();

  const { activeToolSteps, completedToolSteps } = useSkillsStore();

  useEffect(() => {
    if (id) selectConversation(id);
  }, [id]);

  useEffect(() => {
    if (isStreaming) {
      setTimeout(() => flatListRef.current?.scrollToEnd({ animated: true }), 100);
    }
  }, [streamingToken, isStreaming]);

  const messages: Message[] = activeConversation?.messages || [];

  const handleSend = async (text: string, images?: string[]) => {
    if (!id) return;
    await sendMessage(id, text, images);
  };

  const allData: Array<Message | { _type: "streaming" }> = [
    ...messages,
    ...(isStreaming ? [{ _type: "streaming" as const }] : []),
  ];

  return (
    <KeyboardAvoidingView
      className="flex-1 bg-background"
      behavior={Platform.OS === "ios" ? "padding" : "height"}
      keyboardVerticalOffset={0}
    >
      {/* Header */}
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-3">
        <TouchableOpacity
          onPress={() => router.back()}
          hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}
        >
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="flex-1 text-foreground font-semibold text-base" numberOfLines={1}>
          {activeConversation?.title || "Chat"}
        </Text>
      </View>

      {/* Active tool steps */}
      {activeToolSteps.length > 0 && (
        <View className="px-4 py-2 bg-surface border-b border-border">
          {activeToolSteps.map((step) => (
            <ToolCallCard
              key={step.tool_call_id}
              name={step.function_name}
              args={step.arguments}
              isActive
            />
          ))}
        </View>
      )}

      {/* Messages */}
      <FlatList
        ref={flatListRef}
        data={allData}
        keyExtractor={(item, i) =>
          "_type" in item ? "streaming" : item.id || String(i)
        }
        contentContainerClassName="py-4"
        onContentSizeChange={() =>
          flatListRef.current?.scrollToEnd({ animated: false })
        }
        renderItem={({ item }) => {
          if ("_type" in item) {
            return (
              <View className="px-4 pb-2">
                {streamingThinking && (
                  <View className="bg-surface border border-border rounded-xl px-3 py-2 mb-2 max-w-[90%]">
                    <Text className="text-muted text-xs italic mb-1">Thinking…</Text>
                    <Text className="text-muted text-xs font-mono leading-5">
                      {streamingThinking}
                    </Text>
                  </View>
                )}
                <View className="bg-surface border border-border rounded-2xl rounded-bl-md px-4 py-3 max-w-[90%]">
                  <Text className="text-foreground text-sm leading-6">
                    {streamingToken || ""}
                    <Text className="text-primary animate-pulse"> ▌</Text>
                  </Text>
                </View>
              </View>
            );
          }
          return <MessageBubble message={item} />;
        }}
        ListEmptyComponent={
          <View className="flex-1 items-center justify-center py-20">
            <Text className="text-muted text-base">Send a message to begin</Text>
          </View>
        }
      />

      <MessageInput
        onSend={handleSend}
        isStreaming={isStreaming}
        disabled={!activeConversation}
      />
    </KeyboardAvoidingView>
  );
}
