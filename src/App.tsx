import { useEffect, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { Sidebar } from "@/components/layout/Sidebar";
import { ChatView } from "@/components/chat/ChatView";
import { ModelBrowser } from "@/components/models/ModelBrowser";
import { CodingView } from "@/components/coding/CodingView";
import { FlowCanvas } from "@/components/flow/FlowCanvas";
import { RagManager } from "@/components/rag/RagManager";
import { DatabaseView } from "@/components/database/DatabaseView";
import { SettingsView } from "@/components/layout/SettingsView";
import { SkillsPanel } from "@/components/skills/SkillsPanel";
import { ArtifactsView } from "@/components/artifacts/ArtifactsView";
import { LogView } from "@/components/logs/LogView";
import { OnboardingWizard } from "@/components/onboarding/OnboardingWizard";
import { ServerConnect } from "@/components/onboarding/ServerConnect";
import { PersonasView } from "@/components/personas/PersonasView";
import { TemplatesView } from "@/components/templates/TemplatesView";
import { PackagesView } from "@/components/packages/PackagesView";
import { useModelStore } from "@/stores/modelStore";
import { useLogStore } from "@/stores/logStore";
import { useServerStore } from "@/stores/serverStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { isTauri } from "@/lib/transport";
import { hasServerConfig } from "@/lib/serverConfig";

const codingEnabled = import.meta.env.VITE_ENABLE_CODING === "true";

function App() {
  const checkEngineStatus = useModelStore((s) => s.checkEngineStatus);
  const initLogs = useLogStore((s) => s.init);
  const fetchServerStatus = useServerStore((s) => s.fetchStatus);
  const { settings, fetchSettings } = useSettingsStore();

  // Web mode: show a connection screen until the user has configured a backend URL.
  const [serverReady, setServerReady] = useState(() => isTauri() || hasServerConfig());

  useEffect(() => {
    if (!serverReady) return;
    checkEngineStatus();
    initLogs();
    fetchServerStatus();
    fetchSettings();
  }, [serverReady]);

  // In web mode, show the connect screen before anything else loads.
  if (!serverReady) {
    return <ServerConnect onConnected={() => setServerReady(true)} />;
  }

  const showOnboarding = settings !== null && !settings.onboarding_completed;

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-hidden">
        <Routes>
          <Route path="/" element={<Navigate to="/chat" replace />} />
          <Route path="/chat" element={<ChatView />} />
          <Route path="/personas" element={<PersonasView />} />
          <Route path="/templates" element={<TemplatesView />} />
          {codingEnabled && <Route path="/coding" element={<CodingView />} />}
          <Route path="/flows" element={<FlowCanvas />} />
          <Route path="/models" element={<ModelBrowser />} />
          <Route path="/rag" element={<RagManager />} />
          <Route path="/database" element={<DatabaseView />} />
          <Route path="/skills" element={<SkillsPanel />} />
          <Route path="/packages" element={<PackagesView />} />
          <Route path="/artifacts" element={<ArtifactsView />} />
          <Route path="/logs" element={<LogView />} />
          <Route path="/settings" element={<SettingsView />} />
        </Routes>
      </main>

      {/* One-time onboarding overlay — rendered above everything else */}
      {showOnboarding && <OnboardingWizard />}
    </div>
  );
}

export default App;
