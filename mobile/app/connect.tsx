import { useState } from "react";
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
  ScrollView,
  Text,
  TextInput,
  TouchableOpacity,
  View,
} from "react-native";
import { useConnectionStore } from "../stores/connectionStore";

export default function ConnectScreen() {
  const { setHost, checkConnection, isChecking, error } = useConnectionStore();
  const [host, setHostInput] = useState(
    process.env.EXPO_PUBLIC_DEFAULT_HOST || "http://192.168.1.100:3847"
  );
  const [token, setToken] = useState("");

  const handleConnect = async () => {
    const cleaned = host.trim().replace(/\/$/, "");
    await setHost(cleaned, token.trim() || null);
    await checkConnection();
  };

  return (
    <KeyboardAvoidingView
      className="flex-1 bg-background"
      behavior={Platform.OS === "ios" ? "padding" : "height"}
    >
      <ScrollView
        contentContainerClassName="flex-1 items-center justify-center px-6 py-12"
        keyboardShouldPersistTaps="handled"
      >
        {/* Logo / Brand */}
        <View className="items-center mb-10">
          <Text className="text-4xl font-bold text-primary mb-2">XandSuite</Text>
          <Text className="text-muted text-center text-base">
            Mobile — Connect to your desktop instance
          </Text>
        </View>

        {/* Connection form */}
        <View className="w-full max-w-sm gap-4">
          <View>
            <Text className="text-foreground text-sm font-medium mb-1.5">
              Backend host
            </Text>
            <TextInput
              className="bg-surface border border-border rounded-xl px-4 py-3 text-foreground text-base"
              value={host}
              onChangeText={setHostInput}
              placeholder="http://192.168.1.100:3847"
              placeholderTextColor="#6c7086"
              autoCapitalize="none"
              autoCorrect={false}
              keyboardType="url"
            />
            <Text className="text-muted text-xs mt-1">
              Enter your desktop machine's local IP address with port 3847 (or the port you configured in Desktop Settings → Mobile API).
            </Text>
          </View>

          <View>
            <Text className="text-foreground text-sm font-medium mb-1.5">
              API token (optional)
            </Text>
            <TextInput
              className="bg-surface border border-border rounded-xl px-4 py-3 text-foreground text-base"
              value={token}
              onChangeText={setToken}
              placeholder="Leave empty if no auth is set"
              placeholderTextColor="#6c7086"
              secureTextEntry
              autoCapitalize="none"
              autoCorrect={false}
            />
          </View>

          {error && (
            <View className="bg-destructive/10 border border-destructive/30 rounded-xl px-4 py-3">
              <Text className="text-destructive text-sm">{error}</Text>
            </View>
          )}

          <TouchableOpacity
            className={`rounded-xl px-6 py-4 items-center mt-2 ${
              isChecking ? "bg-primary/60" : "bg-primary"
            }`}
            onPress={handleConnect}
            disabled={isChecking}
          >
            {isChecking ? (
              <ActivityIndicator color="#1e1e2e" />
            ) : (
              <Text className="text-background font-semibold text-base">
                Connect
              </Text>
            )}
          </TouchableOpacity>
        </View>

        {/* Help text */}
        <View className="mt-10 px-4 max-w-sm">
          <Text className="text-muted text-xs text-center leading-relaxed">
            Open XandSuite on your desktop → Settings → Mobile API Bridge → Enable and copy your local IP address.
          </Text>
        </View>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}
