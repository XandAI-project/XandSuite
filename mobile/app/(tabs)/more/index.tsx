import {
  ScrollView,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import { useRouter } from "expo-router";
import {
  Bot,
  Brain,
  Database,
  FlaskConical,
  Layers,
  MemoryStick,
  Server,
  ScrollText,
  Wrench,
} from "lucide-react-native";

const MENU_ITEMS = [
  { label: "Models", icon: Bot, route: "/more/models", desc: "Manage LLMs and local server" },
  { label: "RAG", icon: Layers, route: "/more/rag", desc: "Knowledge collections" },
  { label: "Skills / MCP", icon: Wrench, route: "/more/skills", desc: "Tool servers and MCP" },
  { label: "Memory", icon: MemoryStick, route: "/more/memory", desc: "Conversation memory" },
  { label: "Logs", icon: ScrollText, route: "/more/logs", desc: "Application logs" },
  { label: "Agents", icon: Brain, route: "/more/agents", desc: "Autonomous agent tasks" },
  { label: "Database", icon: Database, route: "/more/database", desc: "External DB connections" },
];

export default function MoreTab() {
  const router = useRouter();

  return (
    <View className="flex-1 bg-background">
      <View className="px-4 pt-14 pb-4 border-b border-border">
        <Text className="text-foreground text-xl font-semibold">More</Text>
      </View>

      <ScrollView contentContainerClassName="py-3 px-4 gap-2">
        {MENU_ITEMS.map((item) => (
          <TouchableOpacity
            key={item.route}
            onPress={() => router.push(item.route as never)}
            className="flex-row items-center bg-surface border border-border rounded-2xl px-4 py-4 gap-4"
            activeOpacity={0.75}
          >
            <View className="w-10 h-10 bg-primary/10 rounded-xl items-center justify-center">
              <item.icon size={20} color="#cba6f7" />
            </View>
            <View className="flex-1">
              <Text className="text-foreground font-semibold text-base">{item.label}</Text>
              <Text className="text-muted text-xs mt-0.5">{item.desc}</Text>
            </View>
          </TouchableOpacity>
        ))}
      </ScrollView>
    </View>
  );
}
