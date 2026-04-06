import { useEffect, useState } from "react";
import {
  Alert,
  ScrollView,
  Share,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { useLocalSearchParams, useRouter } from "expo-router";
import { WebView } from "react-native-webview";
import Markdown from "react-native-markdown-display";
import { ChevronLeft, Edit2, Share2, Trash2 } from "lucide-react-native";
import { useArtifactStore } from "../../../stores/artifactStore";
import { artifactIcon } from "../../../lib/utils";

const CODE_BG = "#1e1e2e";

const markdownStyles = {
  body: { color: "#cdd6f4", fontSize: 14, lineHeight: 22, padding: 16 },
  code_block: { backgroundColor: "#181825", borderRadius: 8, padding: 10, color: "#cdd6f4", fontFamily: "monospace", fontSize: 12 },
  code_inline: { backgroundColor: "#313244", borderRadius: 4, paddingHorizontal: 4, color: "#f38ba8", fontFamily: "monospace", fontSize: 12 },
  link: { color: "#89b4fa" },
};

export default function ArtifactDetail() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const router = useRouter();
  const { allArtifacts, update, delete: deleteArtifact } = useArtifactStore();
  const artifact = allArtifacts.find((a) => a.id === id);

  const handleShare = async () => {
    if (!artifact) return;
    await Share.share({ message: artifact.content, title: artifact.title });
  };

  const handleDelete = () => {
    Alert.alert("Delete artifact?", "This cannot be undone.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: async () => {
          await deleteArtifact(artifact!.id);
          router.back();
        },
      },
    ]);
  };

  if (!artifact) {
    return (
      <View className="flex-1 bg-background items-center justify-center">
        <Text className="text-muted">Artifact not found</Text>
      </View>
    );
  }

  const type = artifact.artifact_type || "text";

  return (
    <View className="flex-1 bg-background">
      {/* Header */}
      <View className="flex-row items-center px-4 pt-14 pb-3 border-b border-border gap-2">
        <TouchableOpacity onPress={() => router.back()} hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}>
          <ChevronLeft size={24} color="#cdd6f4" />
        </TouchableOpacity>
        <Text className="text-lg mr-1">{artifactIcon(type)}</Text>
        <Text className="flex-1 text-foreground font-semibold text-base" numberOfLines={1}>
          {artifact.title}
        </Text>
        <TouchableOpacity onPress={handleShare} hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }}>
          <Share2 size={18} color="#6c7086" />
        </TouchableOpacity>
        <TouchableOpacity onPress={handleDelete} hitSlop={{ top: 8, bottom: 8, left: 8, right: 8 }} className="ml-2">
          <Trash2 size={18} color="#f38ba8" />
        </TouchableOpacity>
      </View>

      {/* Content viewer */}
      {type === "html" ? (
        <WebView
          source={{ html: artifact.content }}
          style={{ flex: 1, backgroundColor: "#1e1e2e" }}
        />
      ) : type === "markdown" ? (
        <ScrollView className="flex-1">
          <Markdown style={markdownStyles}>{artifact.content}</Markdown>
        </ScrollView>
      ) : type === "csv" ? (
        <ScrollView horizontal className="flex-1">
          <ScrollView>
            <CsvView content={artifact.content} />
          </ScrollView>
        </ScrollView>
      ) : (
        <ScrollView className="flex-1" horizontal>
          <ScrollView>
            <View className="p-4">
              <Text
                className="text-foreground font-mono text-xs leading-5"
                selectable
              >
                {artifact.content}
              </Text>
            </View>
          </ScrollView>
        </ScrollView>
      )}
    </View>
  );
}

function CsvView({ content }: { content: string }) {
  const rows = content.split("\n").map((r) => r.split(","));
  const headers = rows[0] || [];
  const dataRows = rows.slice(1);

  return (
    <View className="p-4">
      {/* Header row */}
      <View className="flex-row border-b border-border pb-2 mb-1">
        {headers.map((h, i) => (
          <Text
            key={i}
            className="text-primary text-xs font-semibold px-2"
            style={{ minWidth: 80 }}
          >
            {h}
          </Text>
        ))}
      </View>
      {/* Data rows */}
      {dataRows.map((row, ri) => (
        <View key={ri} className="flex-row border-b border-border/30 py-1">
          {row.map((cell, ci) => (
            <Text
              key={ci}
              className="text-foreground text-xs px-2"
              style={{ minWidth: 80 }}
            >
              {cell}
            </Text>
          ))}
        </View>
      ))}
    </View>
  );
}
