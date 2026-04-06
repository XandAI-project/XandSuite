import { useEffect, useState } from "react";
import { Stack, useRouter, useSegments } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { useConnectionStore } from "../stores/connectionStore";
import "../global.css";

function useProtectedRoute(isConnected: boolean) {
  const segments = useSegments();
  const router = useRouter();

  useEffect(() => {
    const inAuthGroup = segments[0] === "connect";
    if (!isConnected && !inAuthGroup) {
      router.replace("/connect");
    } else if (isConnected && inAuthGroup) {
      router.replace("/(tabs)/chat");
    }
  }, [isConnected, segments]);
}

export default function RootLayout() {
  const { isConnected, loadSaved, checkConnection } = useConnectionStore();
  const [isInitialized, setIsInitialized] = useState(false);

  useEffect(() => {
    (async () => {
      await loadSaved();
      if (useConnectionStore.getState().host) {
        await checkConnection();
      }
      setIsInitialized(true);
    })();
  }, []);

  useProtectedRoute(isConnected);

  if (!isInitialized) return null;

  return (
    <>
      <StatusBar style="light" backgroundColor="#1e1e2e" />
      <Stack screenOptions={{ headerShown: false }}>
        <Stack.Screen name="connect" options={{ headerShown: false }} />
        <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
      </Stack>
    </>
  );
}
