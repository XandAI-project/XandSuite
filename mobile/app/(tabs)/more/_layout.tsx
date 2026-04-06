import { Stack } from "expo-router";

export default function MoreLayout() {
  return (
    <Stack screenOptions={{ headerShown: false }}>
      <Stack.Screen name="index" />
      <Stack.Screen name="models" />
      <Stack.Screen name="rag" />
      <Stack.Screen name="skills" />
      <Stack.Screen name="memory" />
      <Stack.Screen name="logs" />
      <Stack.Screen name="agents" />
      <Stack.Screen name="database" />
    </Stack>
  );
}
