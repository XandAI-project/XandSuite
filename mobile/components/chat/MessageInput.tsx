import { useRef, useState } from "react";
import {
  ActivityIndicator,
  Platform,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import * as ImagePicker from "expo-image-picker";
import { Paperclip, Send, Square } from "lucide-react-native";

interface MessageInputProps {
  onSend: (text: string, images?: string[]) => void;
  onStop?: () => void;
  isStreaming?: boolean;
  disabled?: boolean;
}

export function MessageInput({
  onSend,
  onStop,
  isStreaming,
  disabled,
}: MessageInputProps) {
  const [text, setText] = useState("");
  const [attachedImages, setAttachedImages] = useState<string[]>([]);
  const inputRef = useRef<TextInput>(null);

  const handleSend = () => {
    const trimmed = text.trim();
    if (!trimmed && attachedImages.length === 0) return;
    onSend(trimmed, attachedImages.length > 0 ? attachedImages : undefined);
    setText("");
    setAttachedImages([]);
  };

  const handleAttach = async () => {
    const { status } = await ImagePicker.requestMediaLibraryPermissionsAsync();
    if (status !== "granted") return;
    const result = await ImagePicker.launchImageLibraryAsync({
      mediaTypes: ImagePicker.MediaTypeOptions.Images,
      allowsMultipleSelection: true,
      quality: 0.8,
      base64: false,
    });
    if (!result.canceled) {
      setAttachedImages((prev) => [
        ...prev,
        ...result.assets.map((a) => a.uri),
      ]);
    }
  };

  return (
    <View className="border-t border-border bg-base px-4 py-3">
      {/* Attached images preview */}
      {attachedImages.length > 0 && (
        <View className="flex-row flex-wrap gap-2 mb-2">
          {attachedImages.map((uri, i) => (
            <TouchableOpacity
              key={i}
              onPress={() =>
                setAttachedImages((prev) => prev.filter((_, j) => j !== i))
              }
              className="relative"
            >
              <View className="w-12 h-12 rounded-lg bg-surface border border-border items-center justify-center overflow-hidden">
                <Text className="text-muted text-xs">IMG</Text>
              </View>
              <View className="absolute -top-1 -right-1 w-4 h-4 bg-destructive rounded-full items-center justify-center">
                <Text className="text-white text-xs leading-none">×</Text>
              </View>
            </TouchableOpacity>
          ))}
        </View>
      )}

      <View className="flex-row items-end gap-2">
        {/* Attach button */}
        <TouchableOpacity
          onPress={handleAttach}
          disabled={disabled || isStreaming}
          className="w-10 h-10 items-center justify-center rounded-xl bg-surface"
          activeOpacity={0.7}
        >
          <Paperclip size={18} color="#6c7086" />
        </TouchableOpacity>

        {/* Text input */}
        <TextInput
          ref={inputRef}
          className="flex-1 bg-surface border border-border rounded-2xl px-4 py-3 text-foreground text-sm"
          style={{ maxHeight: 120, minHeight: 44 }}
          value={text}
          onChangeText={setText}
          placeholder="Message…"
          placeholderTextColor="#6c7086"
          multiline
          returnKeyType="default"
          onSubmitEditing={Platform.OS === "ios" ? undefined : handleSend}
          editable={!disabled && !isStreaming}
        />

        {/* Send / Stop button */}
        {isStreaming ? (
          <TouchableOpacity
            onPress={onStop}
            className="w-10 h-10 items-center justify-center rounded-xl bg-destructive"
            activeOpacity={0.7}
          >
            <Square size={16} color="#1e1e2e" fill="#1e1e2e" />
          </TouchableOpacity>
        ) : (
          <TouchableOpacity
            onPress={handleSend}
            disabled={
              disabled || (!text.trim() && attachedImages.length === 0)
            }
            className={`w-10 h-10 items-center justify-center rounded-xl ${
              text.trim() || attachedImages.length > 0
                ? "bg-primary"
                : "bg-surface"
            }`}
            activeOpacity={0.7}
          >
            <Send
              size={16}
              color={
                text.trim() || attachedImages.length > 0 ? "#1e1e2e" : "#6c7086"
              }
            />
          </TouchableOpacity>
        )}
      </View>
    </View>
  );
}
