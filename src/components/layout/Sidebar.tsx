import { NavLink } from "react-router-dom";
import {
  MessageSquare,
  Code2,
  Cpu,
  Database,
  FileText,
  Settings,
  Zap,
  Wrench,
  Layers,
  ScrollText,
  UserCircle2,
  LayoutTemplate,
  Package,
  Globe,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useModelStore } from "@/stores/modelStore";
import { useServerStore } from "@/stores/serverStore";
import { useSkillsStore } from "@/stores/skillsStore";
import { useLogStore } from "@/stores/logStore";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const codingEnabled = import.meta.env.VITE_ENABLE_CODING === "true";

// AI-facing tools group
const aiNavItems = [
  { to: "/chat",       icon: MessageSquare,  label: "Chat" },
  { to: "/browser",    icon: Globe,          label: "Browser Agent" },
  { to: "/personas",   icon: UserCircle2,    label: "Personas" },
  { to: "/templates",  icon: LayoutTemplate, label: "Templates" },
  ...(codingEnabled ? [{ to: "/coding", icon: Code2, label: "Coding" }] : []),
  { to: "/skills",     icon: Wrench,         label: "Skills" },
];

// Management / configuration group
const mgmtNavItems = [
  { to: "/models",    icon: Cpu,        label: "Models" },
  { to: "/packages",  icon: Package,    label: "Packages" },
  { to: "/rag",       icon: FileText,   label: "Knowledge" },
  { to: "/artifacts", icon: Layers,     label: "Artifacts" },
  { to: "/logs",      icon: ScrollText, label: "Logs" },
  { to: "/database",  icon: Database,   label: "Database" },
  { to: "/settings",  icon: Settings,   label: "Settings" },
];

interface NavItemProps {
  to: string;
  icon: React.ElementType;
  label: string;
  badge?: React.ReactNode;
}

function NavItem({ to, icon: Icon, label, badge }: NavItemProps) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <NavLink
          to={to}
          aria-label={label}
          className={({ isActive }) =>
            cn(
              "relative flex items-center justify-center w-10 h-10 rounded-lg transition-all duration-150",
              isActive
                ? "glass-btn text-primary border-primary/30 shadow-sm shadow-primary/20"
                : "text-muted-foreground hover:glass-btn hover:text-foreground"
            )
          }
        >
          <Icon className="w-5 h-5" />
          {badge}
        </NavLink>
      </TooltipTrigger>
      <TooltipContent side="right" className="font-medium">
        {label}
      </TooltipContent>
    </Tooltip>
  );
}

export function Sidebar() {
  const isEngineLoaded = useModelStore((s) => s.isEngineLoaded);
  const serverRunning = useServerStore((s) => s.status.running);
  const toolCount = useSkillsStore((s) => s.tools.length);
  const errorCount = useLogStore((s) => s.entries.filter((e) => e.level === "error").length);

  return (
    <TooltipProvider delayDuration={400}>
      <div className="flex flex-col w-14 bg-card border-r border-border h-full shrink-0">
        {/* Logo */}
        <div className="flex items-center justify-center h-14 border-b border-border shrink-0">
          <div className="flex items-center justify-center w-8 h-8 rounded-lg bg-primary shadow-sm shadow-primary/40">
            <Zap className="w-4 h-4 text-primary-foreground" />
          </div>
        </div>

        {/* Navigation */}
        <nav className="flex flex-col items-center gap-1 p-2 flex-1 overflow-hidden">
          {/* AI tools group */}
          {aiNavItems.map(({ to, icon, label }) => (
            <NavItem
              key={to}
              to={to}
              icon={icon}
              label={label}
              badge={
                to === "/skills" && toolCount > 0 ? (
                  <span className="absolute bottom-0.5 right-0.5 w-3.5 h-3.5 rounded-full bg-violet-500 text-[9px] text-white flex items-center justify-center font-bold pointer-events-none">
                    {toolCount > 9 ? "9+" : toolCount}
                  </span>
                ) : undefined
              }
            />
          ))}

          {/* Divider between groups */}
          <div className="w-8 h-px bg-border my-1 shrink-0" />

          {/* Management group */}
          {mgmtNavItems.map(({ to, icon, label }) => (
            <NavItem
              key={to}
              to={to}
              icon={icon}
              label={label}
              badge={
                to === "/logs" && errorCount > 0 ? (
                  <span className="absolute bottom-0.5 right-0.5 w-3.5 h-3.5 rounded-full bg-red-500 text-[9px] text-white flex items-center justify-center font-bold pointer-events-none">
                    {errorCount > 9 ? "9+" : errorCount}
                  </span>
                ) : undefined
              }
            />
          ))}
        </nav>

        {/* Server + engine status */}
        <div className="flex flex-col items-center gap-2 p-2 pb-3 border-t border-border shrink-0">
          <Tooltip>
            <TooltipTrigger asChild>
              <div className="flex items-center justify-center w-7 h-7 rounded-lg hover:bg-secondary cursor-default transition-colors">
                <div className={cn(
                  "w-2 h-2 rounded-full transition-colors",
                  serverRunning ? "bg-emerald-400 animate-pulse" : "bg-muted-foreground/40"
                )} />
              </div>
            </TooltipTrigger>
            <TooltipContent side="right">
              {serverRunning ? "Local server running" : "Local server stopped"}
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <div className="flex items-center justify-center w-7 h-7 rounded-lg hover:bg-secondary cursor-default transition-colors">
                <div className={cn(
                  "w-2 h-2 rounded-full transition-colors",
                  isEngineLoaded ? "bg-blue-400" : "bg-muted-foreground/20"
                )} />
              </div>
            </TooltipTrigger>
            <TooltipContent side="right">
              {isEngineLoaded ? "Engine connected" : "No engine connected"}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
    </TooltipProvider>
  );
}
