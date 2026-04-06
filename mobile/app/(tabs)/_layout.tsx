import { Tabs } from "expo-router";
import {
  MessageCircle,
  Code2,
  Image as ImageIcon,
  Settings,
  MoreHorizontal,
} from "lucide-react-native";
import { useEffect } from "react";
import { sseManager } from "../../api/sse";
import { useConnectionStore } from "../../stores/connectionStore";

export default function TabsLayout() {
  const { host, token } = useConnectionStore();

  useEffect(() => {
    if (host) {
      sseManager.connect();
    }
    return () => {
      sseManager.stop();
    };
  }, [host, token]);

  return (
    <Tabs
      screenOptions={{
        headerShown: false,
        tabBarStyle: {
          backgroundColor: "#1e1e2e",
          borderTopColor: "#313244",
          height: 60,
          paddingBottom: 8,
        },
        tabBarActiveTintColor: "#cba6f7",
        tabBarInactiveTintColor: "#6c7086",
        tabBarLabelStyle: { fontSize: 11, marginTop: -2 },
      }}
    >
      <Tabs.Screen
        name="chat"
        options={{
          title: "Chat",
          tabBarIcon: ({ color, size }) => (
            <MessageCircle color={color} size={size} />
          ),
        }}
      />
      <Tabs.Screen
        name="artifacts"
        options={{
          title: "Artifacts",
          tabBarIcon: ({ color, size }) => <Code2 color={color} size={size} />,
        }}
      />
      <Tabs.Screen
        name="gallery"
        options={{
          title: "Gallery",
          tabBarIcon: ({ color, size }) => (
            <ImageIcon color={color} size={size} />
          ),
        }}
      />
      <Tabs.Screen
        name="more"
        options={{
          title: "More",
          tabBarIcon: ({ color, size }) => (
            <MoreHorizontal color={color} size={size} />
          ),
        }}
      />
      <Tabs.Screen
        name="settings"
        options={{
          title: "Settings",
          tabBarIcon: ({ color, size }) => (
            <Settings color={color} size={size} />
          ),
        }}
      />
    </Tabs>
  );
}
