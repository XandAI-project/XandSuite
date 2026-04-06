import { Image, Text, TouchableOpacity, View } from "react-native";
import Markdown from "react-native-markdown-display";
import { useState } from "react";
import { BookOpen, ChevronRight } from "lucide-react-native";
import { Message, RagSource } from "../../lib/types";
import { stripThinking } from "../../lib/utils";
import { ToolCallCard } from "./ToolCallCard";

interface MessageBubbleProps {
  message: Message;
}

const markdownStyles = {
  body: { color: "#cdd6f4", fontSize: 14, lineHeight: 22 },
  code_block: {
    backgroundColor: "#1e1e2e",
    borderRadius: 8,
    padding: 10,
    color: "#cdd6f4",
    fontFamily: "monospace",
    fontSize: 12,
  },
  code_inline: {
    backgroundColor: "#313244",
    borderRadius: 4,
    paddingHorizontal: 4,
    color: "#f38ba8",
    fontFamily: "monospace",
    fontSize: 12,
  },
  fence: {
    backgroundColor: "#1e1e2e",
    borderRadius: 8,
    padding: 10,
    color: "#cdd6f4",
    fontFamily: "monospace",
    fontSize: 12,
  },
  link: { color: "#89b4fa" },
  blockquote: { borderLeftColor: "#585b70", borderLeftWidth: 3, paddingLeft: 10 },
  heading1: { color: "#cba6f7", fontSize: 20, fontWeight: "700" as const },
  heading2: { color: "#cba6f7", fontSize: 17, fontWeight: "600" as const },
  heading3: { color: "#cba6f7", fontSize: 15, fontWeight: "600" as const },
  bullet_list_icon: { color: "#6c7086" },
};

export function MessageBubble({ message }: MessageBubbleProps) {
  const isUser = message.role === "user";
  const isAssistant = message.role === "assistant";

  const { thinking, content } = extractThinking(message.content || "");

  return (
    <View
      className={`mb-3 ${isUser ? "items-end" : "items-start"} px-4`}
    >
      {/* Role label */}
      <Text className="text-xs text-muted mb-1 mx-1">
        {isUser ? "You" : "Assistant"}
      </Text>

      {/* Thinking block (collapsible-style, shown before main content) */}
      {thinking && (
        <View className="bg-surface border border-border rounded-xl px-3 py-2 mb-2 max-w-[90%]">
          <Text className="text-muted text-xs italic mb-1">Thinking…</Text>
          <Text className="text-muted text-xs font-mono leading-5">{thinking}</Text>
        </View>
      )}

      {/* Tool calls */}
      {message.tool_calls && message.tool_calls.length > 0 && (
        <View className="w-full max-w-[90%] mb-2">
          {message.tool_calls.map((tc, i) => (
            <ToolCallCard
              key={tc.id || i}
              name={tc.function?.name || "tool"}
              args={tc.function?.arguments}
              result={message.tool_results?.[tc.id]}
            />
          ))}
        </View>
      )}

      {/* Image attachments */}
      {message.images && message.images.length > 0 && (
        <View className={`flex-row flex-wrap gap-2 mb-2 ${isUser ? "justify-end" : "justify-start"}`}>
          {message.images.map((uri, i) => (
            <Image
              key={i}
              source={{ uri }}
              className="rounded-xl"
              style={{ width: 160, height: 160 }}
              resizeMode="cover"
            />
          ))}
        </View>
      )}

      {/* Main text */}
      {content.trim().length > 0 && (
        <View
          className={`rounded-2xl px-4 py-3 max-w-[90%] ${
            isUser
              ? "bg-primary rounded-br-md"
              : "bg-surface border border-border rounded-bl-md"
          }`}
        >
          {isUser ? (
            <Text className="text-background text-sm leading-6">{content}</Text>
          ) : (
            <Markdown style={markdownStyles}>{content}</Markdown>
          )}
        </View>
      )}

      {/* RAG source attribution */}
      {isAssistant && (() => {
        const sources = message.metadata?.sources;
        return sources && sources.length > 0
          ? <SourcesCard sources={sources} />
          : null;
      })()}
    </View>
  );
}

// ── SourcesCard ────────────────────────────────────────────────────────────────

function SourcesCard({ sources }: { sources: RagSource[] }) {
  const [open, setOpen] = useState(false);
  return (
    <View className="mt-1.5 max-w-[90%] border border-border/60 rounded-xl overflow-hidden bg-surface/60">
      <TouchableOpacity
        onPress={() => setOpen((v) => !v)}
        className="flex-row items-center gap-2 px-3 py-2"
      >
        <BookOpen size={12} color="#89b4fa" />
        <Text className="flex-1 text-xs text-muted font-medium">
          {sources.length} source{sources.length > 1 ? "s" : ""} retrieved
        </Text>
        <ChevronRight
          size={12}
          color="#6c7086"
          style={{ transform: [{ rotate: open ? "90deg" : "0deg" }] }}
        />
      </TouchableOpacity>
      {open && (
        <View className="border-t border-border/60">
          {sources.map((s, i) => (
            <View key={i} className="px-3 py-2 border-b border-border/30 last:border-0 gap-1">
              <View className="flex-row items-center justify-between gap-2">
                <Text className="text-muted text-xs font-medium flex-1 truncate">
                  {s.source || "document"}
                </Text>
                <View className="bg-primary/10 rounded-md px-1.5 py-0.5">
                  <Text className="text-primary text-[10px] font-medium">
                    {(s.score * 100).toFixed(0)}%
                  </Text>
                </View>
              </View>
              <Text className="text-muted/80 text-xs leading-5" numberOfLines={3}>
                {s.content}
              </Text>
              {s.entities && s.entities.length > 0 && (
                <View className="flex-row flex-wrap gap-1 mt-0.5">
                  {s.entities.slice(0, 4).map((entity, ei) => (
                    <View key={ei} className="bg-purple-500/15 border border-purple-500/20 rounded-md px-1.5 py-0.5">
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
  );
}

function extractThinking(raw: string): { thinking: string; content: string } {
  const match = raw.match(/^<think>([\s\S]*?)<\/think>([\s\S]*)$/);
  if (match) {
    return { thinking: match[1].trim(), content: match[2].trim() };
  }
  return { thinking: "", content: stripThinking(raw) };
}
