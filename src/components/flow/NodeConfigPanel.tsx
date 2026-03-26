import { useCallback } from "react";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { X, Trash2 } from "lucide-react";
import type { Node } from "@reactflow/core";

interface NodeConfigPanelProps {
  node: Node;
  onUpdate: (nodeId: string, data: Record<string, unknown>) => void;
  onClose: () => void;
  onDelete: (nodeId: string) => void;
}

function FieldLabel({ children }: { children: React.ReactNode }) {
  return (
    <label className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
      {children}
    </label>
  );
}

function FieldGroup({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <FieldLabel>{label}</FieldLabel>
      {children}
    </div>
  );
}

function SelectField({
  value,
  options,
  onChange,
}: {
  value: string;
  options: { value: string; label: string }[];
  onChange: (v: string) => void;
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="flex h-9 w-full rounded-md border border-input bg-background text-foreground px-3 py-1 text-sm shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring [&>option]:bg-background [&>option]:text-foreground"
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value} className="bg-background text-foreground">
          {opt.label}
        </option>
      ))}
    </select>
  );
}

function SliderField({
  value,
  min,
  max,
  step,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        className="flex-1 accent-primary h-1.5"
      />
      <span className="text-xs text-muted-foreground w-12 text-right tabular-nums">
        {value}
      </span>
    </div>
  );
}

export function NodeConfigPanel({ node, onUpdate, onClose, onDelete }: NodeConfigPanelProps) {
  const data = node.data as Record<string, unknown>;
  const nodeType = data.nodeType as string;

  const update = useCallback(
    (key: string, value: unknown) => {
      onUpdate(node.id, { ...data, [key]: value });
    },
    [node.id, data, onUpdate]
  );

  const renderFields = () => {
    switch (nodeType) {
      case "trigger":
        return <TriggerFields data={data} update={update} />;
      case "input":
        return <InputFields data={data} update={update} />;
      case "system_prompt":
        return <SystemPromptFields data={data} update={update} />;
      case "user_prompt":
        return <UserPromptFields data={data} update={update} />;
      case "template_prompt":
        return <TemplatePromptFields data={data} update={update} />;
      case "web_search":
        return <WebSearchFields data={data} update={update} />;
      case "code_exec":
        return <CodeExecFields data={data} update={update} />;
      case "http_api":
        return <HttpApiFields data={data} update={update} />;
      case "db_query":
        return <DbQueryFields data={data} update={update} />;
      case "conditional":
        return <ConditionalFields data={data} update={update} />;
      case "loop":
        return <LoopFields data={data} update={update} />;
      case "merge":
        return <MergeFields data={data} update={update} />;
      case "output":
        return <OutputFields data={data} update={update} />;
      default:
        return <div className="text-xs text-muted-foreground">No configurable parameters.</div>;
    }
  };

  return (
    <div className="w-72 border-l border-border bg-card/50 flex flex-col">
      <div className="flex items-center justify-between px-3 py-2.5 border-b border-border">
        <div className="flex items-center gap-2 min-w-0">
          <div className={`w-2.5 h-2.5 rounded-full ${getNodeDotColor(nodeType)}`} />
          <span className="text-sm font-semibold truncate">{data.label as string}</span>
        </div>
        <div className="flex items-center gap-1 shrink-0">
          <Button
            size="icon"
            variant="ghost"
            className="h-6 w-6 text-destructive hover:text-destructive hover:bg-destructive/10"
            onClick={() => { onDelete(node.id); onClose(); }}
            title="Delete node"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </Button>
          <Button size="icon" variant="ghost" className="h-6 w-6" onClick={onClose}>
            <X className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>

      <ScrollArea className="flex-1">
        <div className="p-3 space-y-4">
          <FieldGroup label="Label">
            <Input
              value={(data.label as string) || ""}
              onChange={(e) => update("label", e.target.value)}
              className="h-8 text-xs"
            />
          </FieldGroup>

          {renderFields()}

          <FieldGroup label="Description">
            <Textarea
              value={(data.description as string) || ""}
              onChange={(e) => update("description", e.target.value)}
              placeholder="Optional note about this node..."
              className="text-xs min-h-[50px] resize-none"
              rows={2}
            />
          </FieldGroup>

          <div className="pt-2 border-t border-border">
            <div className="text-[10px] text-muted-foreground">
              <span className="font-medium">ID:</span> {node.id}
            </div>
            <div className="text-[10px] text-muted-foreground">
              <span className="font-medium">Type:</span> {nodeType}
            </div>
          </div>
        </div>
      </ScrollArea>
    </div>
  );
}

function getNodeDotColor(nodeType: string): string {
  const map: Record<string, string> = {
    trigger: "bg-rose-400",
    input: "bg-green-400",
    system_prompt: "bg-purple-400",
    user_prompt: "bg-blue-400",
    template_prompt: "bg-indigo-400",
    web_search: "bg-emerald-400",
    code_exec: "bg-orange-400",
    http_api: "bg-cyan-400",
    db_query: "bg-amber-400",
    conditional: "bg-yellow-400",
    loop: "bg-pink-400",
    merge: "bg-teal-400",
    output: "bg-red-400",
  };
  return map[nodeType] || "bg-gray-400";
}

// ─── Per-Node-Type Field Components ──────────────────────────────────────────

interface FieldProps {
  data: Record<string, unknown>;
  update: (key: string, value: unknown) => void;
}

function TriggerFields({ data, update }: FieldProps) {
  const triggerType = (data.trigger_type as string) || "manual";
  return (
    <>
      <FieldGroup label="Trigger Type">
        <SelectField
          value={triggerType}
          options={[
            { value: "manual", label: "Manual" },
            { value: "schedule", label: "On Schedule (Cron)" },
            { value: "webhook", label: "On Webhook / API Call" },
            { value: "user_request", label: "On User Request" },
            { value: "event", label: "On Event" },
          ]}
          onChange={(v) => update("trigger_type", v)}
        />
      </FieldGroup>

      {triggerType === "schedule" && (
        <FieldGroup label="Cron Expression">
          <Input
            value={(data.schedule_cron as string) || ""}
            onChange={(e) => update("schedule_cron", e.target.value)}
            placeholder="0 */5 * * * (every 5 min)"
            className="h-8 text-xs font-mono"
          />
        </FieldGroup>
      )}

      {triggerType === "webhook" && (
        <FieldGroup label="Webhook Path">
          <Input
            value={(data.webhook_path as string) || ""}
            onChange={(e) => update("webhook_path", e.target.value)}
            placeholder="/api/trigger/my-flow"
            className="h-8 text-xs font-mono"
          />
        </FieldGroup>
      )}

      {triggerType === "event" && (
        <FieldGroup label="Event Name">
          <Input
            value={(data.event_name as string) || ""}
            onChange={(e) => update("event_name", e.target.value)}
            placeholder="e.g. file_uploaded"
            className="h-8 text-xs"
          />
        </FieldGroup>
      )}
    </>
  );
}

function InputFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Variable Name">
        <Input
          value={(data.variable as string) || "input"}
          onChange={(e) => update("variable", e.target.value)}
          placeholder="input"
          className="h-8 text-xs font-mono"
        />
      </FieldGroup>
      <FieldGroup label="Default Value">
        <Textarea
          value={(data.default_value as string) || ""}
          onChange={(e) => update("default_value", e.target.value)}
          placeholder="Default value if no input provided..."
          className="text-xs min-h-[50px] resize-none"
          rows={2}
        />
      </FieldGroup>
    </>
  );
}

function SystemPromptFields({ data, update }: FieldProps) {
  return (
    <FieldGroup label="System Prompt">
      <Textarea
        value={(data.prompt as string) || ""}
        onChange={(e) => update("prompt", e.target.value)}
        placeholder="You are a helpful assistant..."
        className="text-xs min-h-[100px] resize-none font-mono"
        rows={5}
      />
      <div className="text-[10px] text-muted-foreground mt-1">
        Sets the system context for downstream LLM nodes. Supports {"{{variable}}"} templates.
      </div>
    </FieldGroup>
  );
}

function UserPromptFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="User Prompt">
        <Textarea
          value={(data.prompt as string) || ""}
          onChange={(e) => update("prompt", e.target.value)}
          placeholder="Analyze the following: {{input}}"
          className="text-xs min-h-[100px] resize-none font-mono"
          rows={5}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Receives input from previous node. Sends prompt to LLM. Output goes to next node.
          Supports {"{{variable}}"} templates.
        </div>
      </FieldGroup>
      <FieldGroup label="Temperature">
        <SliderField
          value={(data.temperature as number) ?? 0.7}
          min={0}
          max={2}
          step={0.1}
          onChange={(v) => update("temperature", v)}
        />
      </FieldGroup>
      <FieldGroup label="Max Tokens">
        <Input
          type="number"
          value={(data.max_tokens as number) ?? 2048}
          onChange={(e) => update("max_tokens", parseInt(e.target.value) || 2048)}
          className="h-8 text-xs"
          min={1}
          max={32768}
        />
      </FieldGroup>
      <FieldGroup label="Top P">
        <SliderField
          value={(data.top_p as number) ?? 0.9}
          min={0}
          max={1}
          step={0.05}
          onChange={(v) => update("top_p", v)}
        />
      </FieldGroup>
    </>
  );
}

function TemplatePromptFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Template Prompt">
        <Textarea
          value={(data.prompt as string) || ""}
          onChange={(e) => update("prompt", e.target.value)}
          placeholder="Summarize: {{last_response}}"
          className="text-xs min-h-[100px] resize-none font-mono"
          rows={5}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Template with {"{{variable}}"} placeholders. Receives context from previous nodes, sends to LLM, delivers output to next node.
        </div>
      </FieldGroup>
      <FieldGroup label="Temperature">
        <SliderField
          value={(data.temperature as number) ?? 0.7}
          min={0}
          max={2}
          step={0.1}
          onChange={(v) => update("temperature", v)}
        />
      </FieldGroup>
      <FieldGroup label="Max Tokens">
        <Input
          type="number"
          value={(data.max_tokens as number) ?? 2048}
          onChange={(e) => update("max_tokens", parseInt(e.target.value) || 2048)}
          className="h-8 text-xs"
          min={1}
          max={32768}
        />
      </FieldGroup>
      <FieldGroup label="Top P">
        <SliderField
          value={(data.top_p as number) ?? 0.9}
          min={0}
          max={1}
          step={0.05}
          onChange={(v) => update("top_p", v)}
        />
      </FieldGroup>
    </>
  );
}

function WebSearchFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Search Query">
        <Textarea
          value={(data.query as string) || ""}
          onChange={(e) => update("query", e.target.value)}
          placeholder="{{input}} latest news"
          className="text-xs min-h-[60px] resize-none font-mono"
          rows={3}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Receives input from previous node. Performs web search. Results delivered to next node.
          Supports {"{{variable}}"} templates.
        </div>
      </FieldGroup>
      <FieldGroup label="Max Results">
        <Input
          type="number"
          value={(data.max_results as number) ?? 5}
          onChange={(e) => update("max_results", parseInt(e.target.value) || 5)}
          className="h-8 text-xs"
          min={1}
          max={20}
        />
      </FieldGroup>
    </>
  );
}

function CodeExecFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Language">
        <SelectField
          value={(data.language as string) || "bash"}
          options={[
            { value: "bash", label: "Bash / Shell" },
            { value: "python", label: "Python" },
            { value: "node", label: "Node.js" },
            { value: "powershell", label: "PowerShell" },
          ]}
          onChange={(v) => update("language", v)}
        />
      </FieldGroup>
      <FieldGroup label="Command / Code">
        <Textarea
          value={(data.command as string) || ""}
          onChange={(e) => update("command", e.target.value)}
          placeholder="echo 'Hello World'"
          className="text-xs min-h-[100px] resize-none font-mono"
          rows={5}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Receives input from previous node via {"{{variable}}"} templates. Executes code. stdout delivered to next node.
        </div>
      </FieldGroup>
      <FieldGroup label="Timeout (seconds)">
        <Input
          type="number"
          value={(data.timeout_seconds as number) ?? 30}
          onChange={(e) => update("timeout_seconds", parseInt(e.target.value) || 30)}
          className="h-8 text-xs"
          min={1}
          max={300}
        />
      </FieldGroup>
    </>
  );
}

function HttpApiFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Method">
        <SelectField
          value={(data.method as string) || "GET"}
          options={[
            { value: "GET", label: "GET" },
            { value: "POST", label: "POST" },
            { value: "PUT", label: "PUT" },
            { value: "PATCH", label: "PATCH" },
            { value: "DELETE", label: "DELETE" },
          ]}
          onChange={(v) => update("method", v)}
        />
      </FieldGroup>
      <FieldGroup label="URL">
        <Input
          value={(data.url as string) || ""}
          onChange={(e) => update("url", e.target.value)}
          placeholder="https://api.example.com/data"
          className="h-8 text-xs font-mono"
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Supports {"{{variable}}"} templates in URL.
        </div>
      </FieldGroup>
      <FieldGroup label="Content Type">
        <SelectField
          value={(data.content_type as string) || "application/json"}
          options={[
            { value: "application/json", label: "application/json" },
            { value: "application/x-www-form-urlencoded", label: "form-urlencoded" },
            { value: "text/plain", label: "text/plain" },
            { value: "multipart/form-data", label: "multipart/form-data" },
          ]}
          onChange={(v) => update("content_type", v)}
        />
      </FieldGroup>
      <FieldGroup label="Headers (JSON)">
        <Textarea
          value={(data.headers as string) || ""}
          onChange={(e) => update("headers", e.target.value)}
          placeholder={'{"Authorization": "Bearer ..."}'}
          className="text-xs min-h-[50px] resize-none font-mono"
          rows={2}
        />
      </FieldGroup>
      <FieldGroup label="Request Body">
        <Textarea
          value={(data.body as string) || ""}
          onChange={(e) => update("body", e.target.value)}
          placeholder={'{"key": "{{input}}"}'}
          className="text-xs min-h-[60px] resize-none font-mono"
          rows={3}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Receives input from previous node. Makes HTTP request. Response delivered to next node.
        </div>
      </FieldGroup>
    </>
  );
}

function DbQueryFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Database Type">
        <SelectField
          value={(data.db_type as string) || "postgresql"}
          options={[
            { value: "postgresql", label: "PostgreSQL" },
            { value: "mysql", label: "MySQL" },
            { value: "mongodb", label: "MongoDB" },
          ]}
          onChange={(v) => update("db_type", v)}
        />
      </FieldGroup>
      <FieldGroup label="Connection ID">
        <Input
          value={(data.connection_id as string) || ""}
          onChange={(e) => update("connection_id", e.target.value)}
          placeholder="Connection ID from Settings"
          className="h-8 text-xs font-mono"
        />
      </FieldGroup>
      <FieldGroup label="Query">
        <Textarea
          value={(data.query as string) || ""}
          onChange={(e) => update("query", e.target.value)}
          placeholder="SELECT * FROM users WHERE id = {{input}}"
          className="text-xs min-h-[80px] resize-none font-mono"
          rows={4}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Receives input from previous node. Executes query. Result set delivered to next node.
          Supports {"{{variable}}"} templates.
        </div>
      </FieldGroup>
    </>
  );
}

function ConditionalFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Condition Expression">
        <Textarea
          value={(data.condition as string) || ""}
          onChange={(e) => update("condition", e.target.value)}
          placeholder='last_response contains "error"'
          className="text-xs min-h-[60px] resize-none font-mono"
          rows={3}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Evaluates condition from previous node output. Supports: <code>==</code>, <code>!=</code>, <code>contains</code>.
          Routes to True/False branches.
        </div>
      </FieldGroup>
      <FieldGroup label="True Branch Label">
        <Input
          value={(data.true_label as string) || "True"}
          onChange={(e) => update("true_label", e.target.value)}
          className="h-8 text-xs"
        />
      </FieldGroup>
      <FieldGroup label="False Branch Label">
        <Input
          value={(data.false_label as string) || "False"}
          onChange={(e) => update("false_label", e.target.value)}
          className="h-8 text-xs"
        />
      </FieldGroup>
    </>
  );
}

function LoopFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Iterations">
        <Input
          type="number"
          value={(data.iterations as number) ?? 1}
          onChange={(e) => update("iterations", parseInt(e.target.value) || 1)}
          className="h-8 text-xs"
          min={1}
          max={1000}
        />
      </FieldGroup>
      <FieldGroup label="Loop Variable Name">
        <Input
          value={(data.loop_variable as string) || "i"}
          onChange={(e) => update("loop_variable", e.target.value)}
          placeholder="i"
          className="h-8 text-xs font-mono"
        />
      </FieldGroup>
      <FieldGroup label="Collection Variable (optional)">
        <Input
          value={(data.collection_variable as string) || ""}
          onChange={(e) => update("collection_variable", e.target.value)}
          placeholder="items"
          className="h-8 text-xs font-mono"
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          If set, iterates over items in this variable instead of a fixed count.
          Loop variable and current item available to child nodes.
        </div>
      </FieldGroup>
    </>
  );
}

function MergeFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Merge Strategy">
        <SelectField
          value={(data.merge_strategy as string) || "concat"}
          options={[
            { value: "concat", label: "Concatenate" },
            { value: "array", label: "Collect as Array" },
            { value: "first", label: "First Non-null" },
            { value: "last", label: "Last Value" },
          ]}
          onChange={(v) => update("merge_strategy", v)}
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Receives multiple inputs from parallel branches. Merges them using the selected strategy.
          Delivers merged output to next node.
        </div>
      </FieldGroup>
      {((data.merge_strategy as string) || "concat") === "concat" && (
        <FieldGroup label="Separator">
          <Input
            value={(data.separator as string) || "\\n"}
            onChange={(e) => update("separator", e.target.value)}
            placeholder="\n"
            className="h-8 text-xs font-mono"
          />
        </FieldGroup>
      )}
    </>
  );
}

function OutputFields({ data, update }: FieldProps) {
  return (
    <>
      <FieldGroup label="Output Variable">
        <Input
          value={(data.variable as string) || "last_response"}
          onChange={(e) => update("variable", e.target.value)}
          placeholder="last_response"
          className="h-8 text-xs font-mono"
        />
        <div className="text-[10px] text-muted-foreground mt-1">
          Reads the specified variable from context. Falls back to last node output if not found.
        </div>
      </FieldGroup>
      <FieldGroup label="Output Format">
        <SelectField
          value={(data.format as string) || "text"}
          options={[
            { value: "text", label: "Plain Text" },
            { value: "json", label: "JSON" },
            { value: "markdown", label: "Markdown" },
          ]}
          onChange={(v) => update("format", v)}
        />
      </FieldGroup>
    </>
  );
}
