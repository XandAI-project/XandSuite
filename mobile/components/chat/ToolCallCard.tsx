import { useState } from "react";
import { Text, TouchableOpacity, View } from "react-native";
import { ChevronDown, ChevronRight, Wrench } from "lucide-react-native";

interface ToolCallCardProps {
  name: string;
  args?: unknown;
  result?: string;
  isActive?: boolean;
}

export function ToolCallCard({ name, args, result, isActive }: ToolCallCardProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <View className="border border-border rounded-xl overflow-hidden my-1">
      <TouchableOpacity
        className={`flex-row items-center px-3 py-2 gap-2 ${
          isActive ? "bg-primary/10" : "bg-surface"
        }`}
        onPress={() => setExpanded((v) => !v)}
        activeOpacity={0.7}
      >
        <Wrench size={14} color={isActive ? "#cba6f7" : "#7f849c"} />
        <Text
          className={`flex-1 text-sm font-medium ${
            isActive ? "text-primary" : "text-muted"
          }`}
        >
          {name}
        </Text>
        {isActive && (
          <View className="bg-primary/20 px-2 py-0.5 rounded-full">
            <Text className="text-primary text-xs">Running…</Text>
          </View>
        )}
        {expanded ? (
          <ChevronDown size={14} color="#6c7086" />
        ) : (
          <ChevronRight size={14} color="#6c7086" />
        )}
      </TouchableOpacity>

      {expanded && (
        <View className="bg-crust px-3 py-2 gap-2">
          {args !== undefined && (
            <View>
              <Text className="text-muted text-xs font-medium mb-1">Input</Text>
              <Text className="text-foreground text-xs font-mono">
                {typeof args === "string" ? args : JSON.stringify(args, null, 2)}
              </Text>
            </View>
          )}
          {result !== undefined && (
            <View>
              <Text className="text-muted text-xs font-medium mb-1">Output</Text>
              <Text className="text-foreground text-xs font-mono">{result}</Text>
            </View>
          )}
        </View>
      )}
    </View>
  );
}
