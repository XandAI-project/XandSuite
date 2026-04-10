import { useEffect, useState } from "react";
import { invoke } from "@/lib/tauri";
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import {
  Tv,
  Package,
  Code,
  Plus,
  Trash2,
  Play,
  Square,
  // ChevronDown,
  // ChevronUp,
  CheckCircle2,
  AlertCircle,
  Loader2,
  RefreshCw,
  BookOpen,
  Wrench,
  Copy,
  Check,
  Video,
  Image,
  PencilLine,
  LayoutDashboard,
  FileText,
  FolderOpen,
  Clapperboard,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  usePackagesStore,
  type OfficialPackage,
  type CustomPackage,
} from "@/stores/packagesStore";

// ── Icon map ─────────────────────────────────────────────────────────────────

const ICON_MAP: Record<string, React.ElementType> = {
  Tv,
  Package,
  Code,
  Wrench,
  Video,
  Image,
  PencilLine,
  LayoutDashboard,
  FileText,
  Clapperboard,
};

function PkgIcon({ name, className }: { name: string; className?: string }) {
  const Icon = ICON_MAP[name] ?? Package;
  return <Icon className={cn("w-5 h-5", className)} />;
}

// ── Custom code template ─────────────────────────────────────────────────────

const CUSTOM_TEMPLATE = `"""
XandSuite Custom Package: My Package
Short description of what this package does.
"""
from mcp.server.fastmcp import FastMCP

mcp = FastMCP("my-package-name", version="1.0.0")


@mcp.tool()
def my_tool(param1: str, param2: int = 10) -> str:
    """
    Description of what this tool does.
    The LLM will read this docstring to understand when to call this tool.

    Args:
        param1: First parameter description.
        param2: Second parameter description (optional, default 10).
    """
    # Your code here
    import json
    return json.dumps({"result": f"You said: {param1}, number: {param2}"})


if __name__ == "__main__":
    mcp.run(transport="stdio")
`;

// ── Documentation content ─────────────────────────────────────────────────────

const DOC_SECTIONS = [
  {
    title: "What is a Package?",
    content: `A Package is a Python script that exposes tools to the LLM using the **FastMCP** library — the same format used by XandSuite's built-in tools (web search, calculator, etc.).

When you install a package, XandSuite starts it as a background process and makes its tools available in any conversation where Skills are enabled.`,
  },
  {
    title: "Requirements",
    content: `Your script must:
- Import **FastMCP** from \`mcp.server.fastmcp\`
- Create an \`mcp\` instance with a unique name
- Define tools using the \`@mcp.tool()\` decorator
- Call \`mcp.run(transport="stdio")\` in the \`__main__\` block

Your environment needs:
\`\`\`
pip install mcp
\`\`\`
For HTTP requests in your tool: \`pip install requests\``,
  },
  {
    title: "Tool Guidelines",
    content: `**Return value:** Tools must return a **string**. Return JSON strings for structured data.

**Docstrings:** Write clear docstrings — the LLM reads them to decide when and how to call your tool.

**Error handling:** Always catch exceptions and return an error JSON instead of raising:
\`\`\`python
try:
    # your code
    return json.dumps({"result": ...})
except Exception as e:
    return json.dumps({"error": str(e)})
\`\`\`

**Args:** Type-annotate all parameters. FastMCP uses these annotations to build the JSON Schema the LLM sees.`,
  },
  {
    title: "Minimal Example",
    content: `\`\`\`python
from mcp.server.fastmcp import FastMCP
import json

mcp = FastMCP("my-tools", version="1.0.0")

@mcp.tool()
def greet(name: str) -> str:
    """Greet a person by name."""
    return json.dumps({"message": f"Hello, {name}!"})

if __name__ == "__main__":
    mcp.run(transport="stdio")
\`\`\``,
  },
  {
    title: "Package ID Rules",
    content: `The Package ID is used as a filename (\`{id}.py\`) and as an internal server identifier.

- Only letters, numbers, underscores \`_\` and hyphens \`-\`
- No spaces or special characters
- Must be unique among your custom packages

Examples: \`my_weather\`, \`home-assistant\`, \`notion2\``,
  },
];

// ── Dynamic select field (fetches options from a URL at config time) ──────────

interface ArgSchema {
  name: string;
  label: string;
  type: string;
  required: boolean;
  placeholder?: string;
  arg_prefix: string;
  depends_on?: string;
  fetch_endpoint?: string;
  file_extensions?: string[];
}

function DynamicSelectField({
  field,
  configValues,
  onChange,
}: {
  field: ArgSchema;
  configValues: Record<string, string>;
  onChange: (value: string) => void;
}) {
  const [options, setOptions] = useState<string[]>([]);
  const [fetching, setFetching] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);

  const dependsOnValue = field.depends_on ? (configValues[field.depends_on] ?? "") : "";
  const baseUrl = dependsOnValue.trim().replace(/\/$/, "");

  const fetchOptions = async () => {
    if (!baseUrl) return;
    setFetching(true);
    setFetchError(null);
    try {
      // Fetched via Rust to avoid CORS/CSP restrictions in the WebView.
      const names = await invoke<string[]>("fetch_comfyui_workflows", { baseUrl });
      const sorted = [...names].sort((a, b) => a.localeCompare(b));
      setOptions(sorted);
      // Auto-select if only one option
      if (sorted.length === 1 && !configValues[field.name]) {
        onChange(sorted[0]);
      }
    } catch (e) {
      setFetchError(`Could not fetch workflows: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setFetching(false);
    }
  };

  // Auto-fetch when the URL dependency changes
  useEffect(() => {
    if (baseUrl) {
      fetchOptions();
    } else {
      setOptions([]);
    }
  }, [baseUrl]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-sm font-medium">
        {field.label}
        {field.required && <span className="text-destructive ml-0.5">*</span>}
      </label>
      <div className="flex gap-2">
        <select
          className={cn(
            "flex-1 h-9 rounded-md border border-input bg-background px-3 py-1 text-sm",
            "focus:outline-none focus:ring-1 focus:ring-ring",
            "disabled:opacity-50 disabled:cursor-not-allowed"
          )}
          value={configValues[field.name] ?? ""}
          onChange={(e) => onChange(e.target.value)}
          disabled={fetching || options.length === 0}
        >
          {options.length === 0 ? (
            <option value="">
              {fetching ? "Loading…" : baseUrl ? "No workflows found" : "Enter URL first"}
            </option>
          ) : (
            <>
              <option value="">{field.placeholder ?? "Select…"}</option>
              {options.map((opt) => (
                <option key={opt} value={opt}>
                  {opt}
                </option>
              ))}
            </>
          )}
        </select>
        <Button
          type="button"
          size="icon"
          variant="outline"
          className="h-9 w-9 shrink-0"
          onClick={fetchOptions}
          disabled={fetching || !baseUrl}
          title="Refresh workflow list"
        >
          {fetching ? (
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
          ) : (
            <RefreshCw className="w-3.5 h-3.5" />
          )}
        </Button>
      </div>
      {fetchError && (
        <p className="text-xs text-destructive mt-0.5">{fetchError}</p>
      )}
      {!fetchError && options.length > 0 && (
        <p className="text-[11px] text-muted-foreground">
          {options.length} workflow{options.length !== 1 ? "s" : ""} found on server
        </p>
      )}
    </div>
  );
}

// ── File picker field ─────────────────────────────────────────────────────────

function FilePickerField({
  field,
  value,
  onChange,
}: {
  field: ArgSchema;
  value: string;
  onChange: (path: string) => void;
}) {
  const handleBrowse = async () => {
    const extensions = field.file_extensions ?? [];
    const selected = await openFilePicker({
      multiple: false,
      filters:
        extensions.length > 0
          ? [{ name: extensions.map((e: string) => e.toUpperCase()).join("/"), extensions }]
          : [],
    });
    if (typeof selected === "string") onChange(selected);
  };

  // Show just the filename for brevity, keep full path as title tooltip
  const display = value ? value.split(/[\\/]/).pop()! : "";

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-sm font-medium">
        {field.label}
        {field.required && <span className="text-destructive ml-0.5">*</span>}
      </label>
      <div className="flex gap-2">
        <div
          className={cn(
            "flex-1 h-9 rounded-md border border-input bg-background px-3 py-1 text-sm",
            "flex items-center overflow-hidden",
            !value && "text-muted-foreground"
          )}
          title={value || field.placeholder}
        >
          <span className="truncate">{display || field.placeholder}</span>
        </div>
        <Button
          type="button"
          size="icon"
          variant="outline"
          className="h-9 w-9 shrink-0"
          onClick={handleBrowse}
          title="Browse for file"
        >
          <FolderOpen className="w-3.5 h-3.5" />
        </Button>
      </div>
      {value && (
        <p className="text-[11px] text-muted-foreground truncate" title={value}>
          {value}
        </p>
      )}
    </div>
  );
}

// ── Official package card ────────────────────────────────────────────────────

function OfficialPackageCard({ pkg }: { pkg: OfficialPackage }) {
  const { installPackage, uninstallPackage, isLoading } = usePackagesStore();
  const [configOpen, setConfigOpen] = useState(false);
  const [configValues, setConfigValues] = useState<Record<string, string>>({});
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [busyLabel, setBusyLabel] = useState("Installing…");

  const handleInstall = async () => {
    setActionError(null);
    if (!pkg.installed && pkg.args_schema.length > 0) {
      const initial: Record<string, string> = {};
      pkg.args_schema.forEach((s) => {
        initial[s.name] = pkg.config[s.name] ?? "";
      });
      setConfigValues(initial);
      setConfigOpen(true);
    } else if (!pkg.installed) {
      setBusy(true);
      setBusyLabel("Installing dependencies…");
      try {
        await installPackage(pkg.id, {});
      } catch (e) {
        setActionError(String(e));
      } finally {
        setBusy(false);
      }
    }
  };

  const handleUninstall = async () => {
    setActionError(null);
    setBusy(true);
    setBusyLabel("Uninstalling…");
    try {
      await uninstallPackage(pkg.id);
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleConfigSubmit = async () => {
    setActionError(null);
    setBusy(true);
    setBusyLabel("Installing dependencies…");
    try {
      await installPackage(pkg.id, configValues);
      setConfigOpen(false);
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="rounded-xl border border-border bg-card p-5 flex flex-col gap-3 hover:border-border/80 transition-colors">
        {/* Header */}
        <div className="flex items-start gap-3">
          <div className="flex items-center justify-center w-10 h-10 rounded-lg bg-primary/10 shrink-0">
            <PkgIcon name={pkg.icon} className="text-primary" />
          </div>
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <span className="font-semibold text-sm">{pkg.name}</span>
              <Badge variant="secondary" className="text-[10px] px-1.5 py-0">
                {pkg.category}
              </Badge>
              {pkg.installed && (
                <Badge className="text-[10px] px-1.5 py-0 bg-emerald-500/15 text-emerald-600 border-emerald-500/20">
                  <CheckCircle2 className="w-2.5 h-2.5 mr-1" />
                  Active
                </Badge>
              )}
            </div>
            <p className="text-xs text-muted-foreground mt-0.5 leading-relaxed">
              {pkg.description}
            </p>
          </div>
        </div>

        {/* Requires */}
        {pkg.requires.length > 0 && (
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="text-[10px] text-muted-foreground uppercase tracking-wide">
              Requires:
            </span>
            {pkg.requires.map((dep) => (
              <code key={dep} className="text-[10px] bg-secondary px-1.5 py-0.5 rounded font-mono">
                {dep}
              </code>
            ))}
          </div>
        )}

        {/* Error */}
        {actionError && (
          <div className="flex items-start gap-2 text-xs text-destructive bg-destructive/10 rounded-lg p-2.5">
            <AlertCircle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
            <pre className="whitespace-pre-wrap break-all font-sans">{actionError}</pre>
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-2 mt-auto pt-1">
          {pkg.installed ? (
            <>
              <Button
                size="sm"
                variant="outline"
                className="gap-1.5 text-xs"
                onClick={() => {
                  const initial: Record<string, string> = {};
                  pkg.args_schema.forEach((s) => {
                    initial[s.name] = pkg.config[s.name] ?? "";
                  });
                  setConfigValues(initial);
                  setConfigOpen(true);
                }}
                disabled={busy || isLoading}
              >
                <Wrench className="w-3.5 h-3.5" />
                Reconfigure
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="gap-1.5 text-xs text-destructive hover:text-destructive"
                onClick={handleUninstall}
                disabled={busy || isLoading}
              >
                {busy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Square className="w-3.5 h-3.5" />}
                {busy ? busyLabel : "Uninstall"}
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              className="gap-1.5 text-xs"
              onClick={handleInstall}
              disabled={busy || isLoading}
            >
              {busy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />}
              {busy ? busyLabel : "Install"}
            </Button>
          )}
        </div>
      </div>

      {/* Config dialog */}
      <Dialog open={configOpen} onOpenChange={setConfigOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Configure {pkg.name}</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-4 py-2">
            {pkg.args_schema.map((field) => {
              if (field.type === "dynamic_select") {
                return (
                  <DynamicSelectField
                    key={field.name}
                    field={field}
                    configValues={configValues}
                    onChange={(val) =>
                      setConfigValues((prev) => ({ ...prev, [field.name]: val }))
                    }
                  />
                );
              }
              if (field.type === "file") {
                return (
                  <FilePickerField
                    key={field.name}
                    field={field}
                    value={configValues[field.name] ?? ""}
                    onChange={(val) =>
                      setConfigValues((prev) => ({ ...prev, [field.name]: val }))
                    }
                  />
                );
              }
              return (
                <div key={field.name} className="flex flex-col gap-1.5">
                  <label className="text-sm font-medium">
                    {field.label}
                    {field.required && <span className="text-destructive ml-0.5">*</span>}
                  </label>
                  <Input
                    value={configValues[field.name] ?? ""}
                    onChange={(e) =>
                      setConfigValues((prev) => ({ ...prev, [field.name]: e.target.value }))
                    }
                    placeholder={field.placeholder}
                    type={
                      field.name.includes("key") || field.name.includes("token")
                        ? "password"
                        : "text"
                    }
                  />
                </div>
              );
            })}
            {actionError && (
              <div className="flex items-start gap-2 text-xs text-destructive bg-destructive/10 rounded-lg p-2.5">
                <AlertCircle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                <pre className="whitespace-pre-wrap break-all font-sans">{actionError}</pre>
              </div>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfigOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button
              onClick={handleConfigSubmit}
              disabled={busy || pkg.args_schema.some((s) => s.required && !configValues[s.name])}
            >
              {busy ? <Loader2 className="w-4 h-4 animate-spin mr-2" /> : null}
              {busy ? busyLabel : pkg.installed ? "Save & Reconnect" : "Install"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}

// ── Custom package card ────────────────────────────────────────────────────────

function CustomPackageCard({ pkg }: { pkg: CustomPackage }) {
  const {
    installCustomPackage,
    uninstallCustomPackage,
    deleteCustomPackage,
    getCustomPackageCode,
    saveCustomPackage,
    isLoading,
  } = usePackagesStore();
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [editName, setEditName] = useState(pkg.name);
  const [editDesc, setEditDesc] = useState(pkg.description);
  const [editReqs, setEditReqs] = useState(pkg.requirements);
  const [editCode, setEditCode] = useState("");
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [busyLabel, setBusyLabel] = useState("");

  const handleEdit = async () => {
    setBusy(true);
    try {
      const code = await getCustomPackageCode(pkg.id);
      setEditCode(code);
      setEditName(pkg.name);
      setEditDesc(pkg.description);
      setEditReqs(pkg.requirements);
      setEditOpen(true);
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleSaveEdit = async () => {
    setActionError(null);
    setBusy(true);
    setBusyLabel("Saving…");
    try {
      await saveCustomPackage(pkg.id, editName, editDesc, editReqs, editCode);
      setEditOpen(false);
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleToggle = async () => {
    setActionError(null);
    setBusy(true);
    setBusyLabel(pkg.installed ? "Stopping…" : "Installing dependencies…");
    try {
      if (pkg.installed) {
        await uninstallCustomPackage(pkg.id);
      } else {
        await installCustomPackage(pkg.id);
      }
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleDelete = async () => {
    setBusy(true);
    try {
      await deleteCustomPackage(pkg.id);
    } catch (e) {
      setActionError(String(e));
      setBusy(false);
    }
  };

  return (
    <>
      <div className="rounded-xl border border-border bg-card px-4 py-3 flex items-center gap-3">
        <div className="flex items-center justify-center w-9 h-9 rounded-lg bg-secondary shrink-0">
          <Code className="w-4 h-4 text-muted-foreground" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-medium text-sm truncate">{pkg.name}</span>
            {pkg.installed && (
              <Badge className="text-[10px] px-1.5 py-0 bg-emerald-500/15 text-emerald-600 border-emerald-500/20 shrink-0">
                <CheckCircle2 className="w-2.5 h-2.5 mr-1" />
                Active
              </Badge>
            )}
          </div>
          {pkg.description && (
            <p className="text-xs text-muted-foreground truncate">{pkg.description}</p>
          )}
        </div>

        {actionError && (
          <span className="text-xs text-destructive max-w-[140px] truncate" title={actionError}>
            {actionError}
          </span>
        )}

        <div className="flex items-center gap-1.5 shrink-0">
          <Button
            size="sm"
            variant="ghost"
            className="h-8 px-2 text-xs gap-1"
            onClick={handleEdit}
            disabled={busy || isLoading}
            title="Edit source"
          >
            <Code className="w-3.5 h-3.5" />
          </Button>
          <Button
            size="sm"
            variant={pkg.installed ? "outline" : "default"}
            className={cn("h-8 px-3 text-xs gap-1.5", pkg.installed && !busy && "text-destructive")}
            onClick={handleToggle}
            disabled={busy || isLoading}
          >
            <Loader2 className={cn("w-3.5 h-3.5", busy ? "animate-spin" : "hidden")} />
            {!busy && (pkg.installed ? <Square className="w-3 h-3" /> : <Play className="w-3 h-3" />)}
            {busy ? busyLabel : pkg.installed ? "Stop" : "Run"}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            className="h-8 w-8 p-0 text-muted-foreground hover:text-destructive"
            onClick={() => setDeleteOpen(true)}
            disabled={busy}
            title="Delete"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>

      {/* Edit dialog */}
      <Dialog open={editOpen} onOpenChange={setEditOpen}>
        <DialogContent className="sm:max-w-2xl max-h-[80vh] flex flex-col">
          <DialogHeader>
            <DialogTitle>Edit Custom Package</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-3 overflow-y-auto flex-1 pr-1">
            <div className="flex gap-3">
              <div className="flex-1 flex flex-col gap-1">
                <label className="text-xs font-medium text-muted-foreground">Name</label>
                <Input
                  value={editName}
                  onChange={(e) => setEditName(e.target.value)}
                  placeholder="My Package"
                />
              </div>
              <div className="flex-1 flex flex-col gap-1">
                <label className="text-xs font-medium text-muted-foreground">Description</label>
                <Input
                  value={editDesc}
                  onChange={(e) => setEditDesc(e.target.value)}
                  placeholder="What does this package do?"
                />
              </div>
            </div>
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-muted-foreground">
                Requirements
                <span className="ml-1 font-normal text-muted-foreground/60">(one package per line, auto-installed)</span>
              </label>
              <textarea
                className="min-h-[60px] font-mono text-xs bg-secondary border border-border rounded-lg p-3 resize-y focus:outline-none focus:ring-1 focus:ring-primary"
                value={editReqs}
                onChange={(e) => setEditReqs(e.target.value)}
                placeholder={"requests\nbeautifulsoup4\npandas>=1.5.0"}
                spellCheck={false}
              />
            </div>
            <div className="flex flex-col gap-1 flex-1">
              <label className="text-xs font-medium text-muted-foreground">Python Code</label>
              <textarea
                className="flex-1 min-h-[260px] font-mono text-xs bg-secondary border border-border rounded-lg p-3 resize-none focus:outline-none focus:ring-1 focus:ring-primary"
                value={editCode}
                onChange={(e) => setEditCode(e.target.value)}
                spellCheck={false}
              />
            </div>
            {actionError && (
              <div className="flex items-start gap-2 text-xs text-destructive bg-destructive/10 rounded-lg p-2.5">
                <AlertCircle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
                <pre className="whitespace-pre-wrap break-all font-sans">{actionError}</pre>
              </div>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditOpen(false)} disabled={busy}>
              Cancel
            </Button>
            <Button onClick={handleSaveEdit} disabled={busy || !editName.trim() || !editCode.trim()}>
              {busy && <Loader2 className="w-4 h-4 animate-spin mr-2" />}
              {busy ? busyLabel : "Save"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Delete confirm */}
      <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{pkg.name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently delete the package script and remove it from the LLM's available
              tools. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleDelete}>Delete</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

// ── Add custom package dialog ────────────────────────────────────────────────

function AddCustomPackageDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { saveCustomPackage, isLoading } = usePackagesStore();
  const [id, setId] = useState("");
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [requirements, setRequirements] = useState("");
  const [code, setCode] = useState(CUSTOM_TEMPLATE);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const handleSubmit = async () => {
    setError(null);
    setBusy(true);
    try {
      await saveCustomPackage(id.trim(), name.trim(), description.trim(), requirements, code);
      setId("");
      setName("");
      setDescription("");
      setRequirements("");
      setCode(CUSTOM_TEMPLATE);
      onOpenChange(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const isValid =
    id.trim().length > 0 &&
    name.trim().length > 0 &&
    code.trim().length > 0 &&
    /^[a-zA-Z0-9_-]+$/.test(id.trim());

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>Add Custom Package</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3 overflow-y-auto flex-1 pr-1">
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-muted-foreground">
                Package ID <span className="text-destructive">*</span>
              </label>
              <Input
                value={id}
                onChange={(e) => setId(e.target.value.replace(/[^a-zA-Z0-9_-]/g, ""))}
                placeholder="my_package"
                className="font-mono"
              />
              <span className="text-[10px] text-muted-foreground">Letters, numbers, _ and - only</span>
            </div>
            <div className="flex flex-col gap-1">
              <label className="text-xs font-medium text-muted-foreground">
                Display Name <span className="text-destructive">*</span>
              </label>
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="My Package"
              />
            </div>
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-muted-foreground">Description</label>
            <Input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="What does this package do?"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs font-medium text-muted-foreground">
              Requirements
              <span className="ml-1 font-normal text-muted-foreground/60">
                (one pip package per line — installed automatically)
              </span>
            </label>
            <textarea
              className="min-h-[60px] font-mono text-xs bg-secondary border border-border rounded-lg p-3 resize-y focus:outline-none focus:ring-1 focus:ring-primary"
              value={requirements}
              onChange={(e) => setRequirements(e.target.value)}
              placeholder={"requests\nbeautifulsoup4\npandas>=1.5.0"}
              spellCheck={false}
            />
          </div>
          <div className="flex flex-col gap-1 flex-1">
            <label className="text-xs font-medium text-muted-foreground">
              Python Code <span className="text-destructive">*</span>
            </label>
            <textarea
              className="flex-1 min-h-[260px] font-mono text-xs bg-secondary border border-border rounded-lg p-3 resize-none focus:outline-none focus:ring-1 focus:ring-primary"
              value={code}
              onChange={(e) => setCode(e.target.value)}
              spellCheck={false}
            />
          </div>
          {error && (
            <div className="flex items-start gap-2 text-xs text-destructive bg-destructive/10 rounded-lg p-2.5">
              <AlertCircle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
              <pre className="whitespace-pre-wrap break-all font-sans">{error}</pre>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={!isValid || busy || isLoading}>
            {busy && <Loader2 className="w-4 h-4 animate-spin mr-2" />}
            Create Package
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ── Documentation tab ─────────────────────────────────────────────────────────

function DocSection({ title, content }: { title: string; content: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = (text: string) => {
    const code = text.match(/```(?:python)?\n([\s\S]*?)```/)?.[1] ?? text;
    navigator.clipboard.writeText(code.trim());
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const renderContent = (raw: string) => {
    const segments = raw.split(/(```(?:python|bash)?\n[\s\S]*?```)/g);
    return segments.map((seg, i) => {
      const codeMatch = seg.match(/```(?:python|bash)?\n([\s\S]*?)```/);
      if (codeMatch) {
        return (
          <div key={i} className="relative group mt-2 mb-1">
            <pre className="bg-secondary rounded-lg p-3 text-xs font-mono overflow-x-auto whitespace-pre">
              {codeMatch[1]}
            </pre>
            <button
              onClick={() => handleCopy(seg)}
              className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded bg-background/80 hover:bg-background"
              title="Copy"
            >
              {copied ? (
                <Check className="w-3 h-3 text-emerald-500" />
              ) : (
                <Copy className="w-3 h-3 text-muted-foreground" />
              )}
            </button>
          </div>
        );
      }
      // Render inline code and bold
      const parts = seg.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
      return (
        <p key={i} className="text-sm text-foreground/80 leading-relaxed whitespace-pre-line">
          {parts.map((part, j) => {
            if (part.startsWith("`") && part.endsWith("`")) {
              return (
                <code key={j} className="bg-secondary px-1 py-0.5 rounded text-xs font-mono">
                  {part.slice(1, -1)}
                </code>
              );
            }
            if (part.startsWith("**") && part.endsWith("**")) {
              return <strong key={j}>{part.slice(2, -2)}</strong>;
            }
            return part;
          })}
        </p>
      );
    });
  };

  return (
    <div className="rounded-xl border border-border bg-card p-5">
      <h3 className="font-semibold text-sm mb-3">{title}</h3>
      <div className="space-y-1">{renderContent(content)}</div>
    </div>
  );
}

function DocumentationTab() {
  const [copied, setCopied] = useState(false);

  const handleCopyTemplate = () => {
    navigator.clipboard.writeText(CUSTOM_TEMPLATE.trim());
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="space-y-4">
      {/* Starter template */}
      <div className="rounded-xl border border-primary/30 bg-primary/5 p-5">
        <div className="flex items-center justify-between mb-3">
          <h3 className="font-semibold text-sm flex items-center gap-2">
            <BookOpen className="w-4 h-4 text-primary" />
            Starter Template
          </h3>
          <Button size="sm" variant="outline" className="gap-1.5 text-xs h-7" onClick={handleCopyTemplate}>
            {copied ? (
              <>
                <Check className="w-3 h-3 text-emerald-500" />
                Copied!
              </>
            ) : (
              <>
                <Copy className="w-3 h-3" />
                Copy
              </>
            )}
          </Button>
        </div>
        <pre className="text-xs font-mono text-foreground/80 overflow-x-auto whitespace-pre bg-background/50 rounded-lg p-3 border border-border">
          {CUSTOM_TEMPLATE.trim()}
        </pre>
      </div>

      {/* Sections */}
      {DOC_SECTIONS.map((s) => (
        <DocSection key={s.title} title={s.title} content={s.content} />
      ))}
    </div>
  );
}

// ── Main view ─────────────────────────────────────────────────────────────────

export function PackagesView() {
  const {
    officialPackages,
    customPackages,
    isLoading,
    error,
    fetchOfficial,
    fetchCustom,
    clearError,
  } = usePackagesStore();

  const [addOpen, setAddOpen] = useState(false);
  const [activeTab, setActiveTab] = useState("official");

  useEffect(() => {
    fetchOfficial();
    fetchCustom();
  }, []);

  const installedCount = officialPackages.filter((p) => p.installed).length +
    customPackages.filter((p) => p.installed).length;

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-border shrink-0">
        <div>
          <h1 className="text-lg font-semibold flex items-center gap-2">
            <Package className="w-5 h-5 text-primary" />
            Package Manager
          </h1>
          <p className="text-xs text-muted-foreground mt-0.5">
            Install tool packages to extend the LLM's capabilities
            {installedCount > 0 && (
              <span className="ml-1.5 inline-flex items-center gap-1 text-emerald-600">
                · {installedCount} active
              </span>
            )}
          </p>
        </div>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => { fetchOfficial(); fetchCustom(); }}
          disabled={isLoading}
          className="gap-1.5 text-xs h-8"
        >
          <RefreshCw className={cn("w-3.5 h-3.5", isLoading && "animate-spin")} />
          Refresh
        </Button>
      </div>

      {/* Error banner */}
      {error && (
        <div className="mx-6 mt-4 flex items-start gap-2 text-sm text-destructive bg-destructive/10 rounded-lg p-3 border border-destructive/20">
          <AlertCircle className="w-4 h-4 mt-0.5 shrink-0" />
          <span className="flex-1 break-all">{error}</span>
          <button
            onClick={clearError}
            className="text-xs underline underline-offset-2 shrink-0"
          >
            Dismiss
          </button>
        </div>
      )}

      <Tabs
        value={activeTab}
        onValueChange={setActiveTab}
        className="flex flex-col flex-1 overflow-hidden"
      >
        {/* Tab nav */}
        <div className="border-b border-border px-6 shrink-0">
          <TabsList className="h-10 bg-transparent gap-1 p-0">
            <TabsTrigger
              value="official"
              className="h-9 px-3 text-sm data-[state=active]:bg-transparent data-[state=active]:border-b-2 data-[state=active]:border-primary data-[state=active]:shadow-none rounded-none font-medium"
            >
              Official
              {officialPackages.length > 0 && (
                <span className="ml-1.5 text-[10px] bg-secondary px-1.5 py-0.5 rounded-full">
                  {officialPackages.length}
                </span>
              )}
            </TabsTrigger>
            <TabsTrigger
              value="custom"
              className="h-9 px-3 text-sm data-[state=active]:bg-transparent data-[state=active]:border-b-2 data-[state=active]:border-primary data-[state=active]:shadow-none rounded-none font-medium"
            >
              Custom
              {customPackages.length > 0 && (
                <span className="ml-1.5 text-[10px] bg-secondary px-1.5 py-0.5 rounded-full">
                  {customPackages.length}
                </span>
              )}
            </TabsTrigger>
            <TabsTrigger
              value="docs"
              className="h-9 px-3 text-sm data-[state=active]:bg-transparent data-[state=active]:border-b-2 data-[state=active]:border-primary data-[state=active]:shadow-none rounded-none font-medium"
            >
              <BookOpen className="w-3.5 h-3.5 mr-1.5" />
              Documentation
            </TabsTrigger>
          </TabsList>
        </div>

        {/* Official tab */}
        <TabsContent value="official" className="flex-1 overflow-hidden mt-0">
          <ScrollArea className="h-full px-6 py-6">
            {officialPackages.length === 0 && !isLoading ? (
              <div className="flex flex-col items-center justify-center py-16 text-center">
                <Package className="w-10 h-10 text-muted-foreground/30 mb-3" />
                <p className="text-sm text-muted-foreground">No official packages found.</p>
                <p className="text-xs text-muted-foreground/60 mt-1">
                  Check that <code className="text-[11px] font-mono">tools/packages/registry.json</code> exists.
                </p>
              </div>
            ) : (
              <div className="grid gap-4 max-w-3xl">
                {officialPackages.map((pkg) => (
                  <OfficialPackageCard key={pkg.id} pkg={pkg} />
                ))}
              </div>
            )}
          </ScrollArea>
        </TabsContent>

        {/* Custom tab */}
        <TabsContent value="custom" className="flex-1 overflow-hidden mt-0">
          <ScrollArea className="h-full px-6 py-6">
            <div className="max-w-3xl">
              {/* Add button */}
              <div className="flex items-center justify-between mb-5">
                <div>
                  <h2 className="text-sm font-semibold">Custom Packages</h2>
                  <p className="text-xs text-muted-foreground mt-0.5">
                    Write your own Python tools using FastMCP
                  </p>
                </div>
                <Button
                  size="sm"
                  className="gap-1.5 text-xs"
                  onClick={() => setAddOpen(true)}
                >
                  <Plus className="w-3.5 h-3.5" />
                  Add Custom Package
                </Button>
              </div>

              {customPackages.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-14 text-center border border-dashed border-border rounded-xl">
                  <Code className="w-9 h-9 text-muted-foreground/30 mb-3" />
                  <p className="text-sm text-muted-foreground">No custom packages yet.</p>
                  <p className="text-xs text-muted-foreground/60 mt-1 mb-4">
                    Create a package to add your own tools to the LLM.
                  </p>
                  <Button
                    size="sm"
                    variant="outline"
                    className="gap-1.5 text-xs"
                    onClick={() => setAddOpen(true)}
                  >
                    <Plus className="w-3.5 h-3.5" />
                    Add Custom Package
                  </Button>
                </div>
              ) : (
                <div className="flex flex-col gap-2">
                  {customPackages.map((pkg) => (
                    <CustomPackageCard key={pkg.id} pkg={pkg} />
                  ))}
                </div>
              )}
            </div>
          </ScrollArea>
        </TabsContent>

        {/* Documentation tab */}
        <TabsContent value="docs" className="flex-1 overflow-hidden mt-0">
          <ScrollArea className="h-full px-6 py-6">
            <div className="max-w-2xl">
              <DocumentationTab />
            </div>
          </ScrollArea>
        </TabsContent>
      </Tabs>

      <AddCustomPackageDialog open={addOpen} onOpenChange={setAddOpen} />
    </div>
  );
}
