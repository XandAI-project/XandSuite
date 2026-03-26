import { useEffect } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import { Sidebar } from "@/components/layout/Sidebar";
import { ChatView } from "@/components/chat/ChatView";
import { ModelBrowser } from "@/components/models/ModelBrowser";
import { AgentTaskView } from "@/components/agents/AgentTaskView";
import { FlowCanvas } from "@/components/flow/FlowCanvas";
import { RagManager } from "@/components/rag/RagManager";
import { DatabaseView } from "@/components/database/DatabaseView";
import { SettingsView } from "@/components/layout/SettingsView";
import { SkillsPanel } from "@/components/skills/SkillsPanel";
import { ArtifactsView } from "@/components/artifacts/ArtifactsView";
import { LogView } from "@/components/logs/LogView";
import { useModelStore } from "@/stores/modelStore";
import { useLogStore } from "@/stores/logStore";

function App() {
  const checkEngineStatus = useModelStore((s) => s.checkEngineStatus);
  const initLogs = useLogStore((s) => s.init);

  useEffect(() => {
    checkEngineStatus();
    initLogs();
  }, []);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-hidden">
        <Routes>
          <Route path="/" element={<Navigate to="/chat" replace />} />
          <Route path="/chat" element={<ChatView />} />
          <Route path="/agents" element={<AgentTaskView />} />
          <Route path="/flows" element={<FlowCanvas />} />
          <Route path="/models" element={<ModelBrowser />} />
          <Route path="/rag" element={<RagManager />} />
          <Route path="/database" element={<DatabaseView />} />
          <Route path="/skills" element={<SkillsPanel />} />
          <Route path="/artifacts" element={<ArtifactsView />} />
          <Route path="/logs" element={<LogView />} />
          <Route path="/settings" element={<SettingsView />} />
        </Routes>
      </main>
    </div>
  );
}

export default App;
