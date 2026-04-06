import { NavLink } from "react-router-dom";
import {
  MessageSquare,
  Code2,
  Cpu,
  Database,
  FileText,
  GitBranch,
  Settings,
  Zap,
  Wrench,
  Layers,
  ScrollText,
  UserCircle2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useModelStore } from "@/stores/modelStore";
import { useServerStore } from "@/stores/serverStore";
import { useSkillsStore } from "@/stores/skillsStore";
import { useLogStore } from "@/stores/logStore";

const codingEnabled = import.meta.env.VITE_ENABLE_CODING === "true";

const navItems = [
  { to: "/chat",      icon: MessageSquare, label: "Chat" },
  { to: "/personas",  icon: UserCircle2,   label: "Personas" },
  ...(codingEnabled ? [{ to: "/coding", icon: Code2, label: "Coding" }] : []),
  { to: "/flows",     icon: GitBranch,     label: "Flows" },
  { to: "/skills",    icon: Wrench,        label: "Skills" },
  { to: "/models",    icon: Cpu,           label: "Models" },
  { to: "/rag",       icon: FileText,      label: "RAG" },
  { to: "/artifacts", icon: Layers,        label: "Artifacts" },
  { to: "/logs",      icon: ScrollText,    label: "Logs" },
  { to: "/database",  icon: Database,      label: "Database" },
  { to: "/settings",  icon: Settings,      label: "Settings" },
];

export function Sidebar() {
  const isEngineLoaded = useModelStore((s) => s.isEngineLoaded);
  const serverRunning = useServerStore((s) => s.status.running);
  const toolCount = useSkillsStore((s) => s.tools.length);
  const errorCount = useLogStore((s) => s.entries.filter((e) => e.level === "error").length);

  return (
    <div className="flex flex-col w-14 bg-card border-r border-border h-full">
      {/* Logo */}
      <div className="flex items-center justify-center h-14 border-b border-border">
        <div className="flex items-center justify-center w-8 h-8 rounded-lg bg-primary">
          <Zap className="w-4 h-4 text-primary-foreground" />
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex flex-col items-center gap-1 p-2 flex-1">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            title={label}
            className={({ isActive }) =>
              cn(
                "flex items-center justify-center w-10 h-10 rounded-lg transition-colors relative group",
                isActive
                  ? "bg-primary text-primary-foreground"
                  : "text-muted-foreground hover:bg-secondary hover:text-foreground"
              )
            }
          >
            <Icon className="w-5 h-5" />
            {/* Tool count badge */}
            {to === "/skills" && toolCount > 0 && (
              <span className="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 rounded-full bg-violet-500 text-[9px] text-white flex items-center justify-center font-bold">
                {toolCount > 9 ? "9+" : toolCount}
              </span>
            )}
            {/* Error count badge on Logs */}
            {to === "/logs" && errorCount > 0 && (
              <span className="absolute -top-0.5 -right-0.5 w-3.5 h-3.5 rounded-full bg-red-500 text-[9px] text-white flex items-center justify-center font-bold">
                {errorCount > 9 ? "9+" : errorCount}
              </span>
            )}
            {/* Tooltip */}
            <div className="absolute left-full ml-2 px-2 py-1 bg-popover text-popover-foreground text-xs rounded-md shadow-md opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none whitespace-nowrap z-50">
              {label}
            </div>
          </NavLink>
        ))}
      </nav>

      {/* Server + engine status */}
      <div className="flex flex-col items-center gap-1.5 p-2 pb-3 border-t border-border">
        {/* Internal server dot */}
        <div
          title={serverRunning ? "Local server running" : "Local server stopped"}
          className="flex items-center gap-1"
        >
          <div className={cn(
            "w-2 h-2 rounded-full",
            serverRunning ? "bg-emerald-400 animate-pulse" : "bg-muted-foreground/40"
          )} />
        </div>
        {/* Engine/remote dot */}
        <div
          title={isEngineLoaded ? "Engine connected" : "No engine connected"}
          className="flex items-center gap-1"
        >
          <div className={cn(
            "w-1.5 h-1.5 rounded-full",
            isEngineLoaded ? "bg-blue-400" : "bg-muted-foreground/20"
          )} />
        </div>
      </div>
    </div>
  );
}
