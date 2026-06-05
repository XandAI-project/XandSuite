import { useEffect, useMemo, useState } from "react";
import {
  Cookie,
  Loader2,
  Plus,
  Save,
  Trash2,
  X,
  AlertCircle,
  CheckCircle2,
  KeyRound,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { invoke } from "@/lib/tauri";
import { cn } from "@/lib/utils";

/**
 * UI for the disk-backed `CookieVault` (Settings → Browser).
 *
 * The user pastes cookies once — typically copied from a browser extension
 * like Cookie-Editor — and the saved session can later be selected when
 * starting a Browser Agent run, so the embedded Chromium is already
 * authenticated against the target site.
 *
 * Raw cookie *values* are deliberately never returned by the backend list
 * endpoint, so this view only ever displays metadata (name, cookie count,
 * the domains they apply to).
 */

interface CookieSessionDigest {
  id: string;
  name: string;
  notes: string;
  default_domain: string | null;
  created_at: string;
  updated_at: string;
  cookie_count: number;
  domains: string[];
}

interface ParsePreview {
  cookie_count: number;
  domains: string[];
  missing_domain: number;
}

export function BrowserCookieSessions() {
  const [sessions, setSessions] = useState<CookieSessionDigest[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id?: string } | null>(null);

  const refresh = async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<CookieSessionDigest[]>(
        "list_browser_cookie_sessions"
      );
      // Most-recently-updated first.
      list.sort((a, b) => b.updated_at.localeCompare(a.updated_at));
      setSessions(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const handleDelete = async (id: string, name: string) => {
    if (!confirm(`Delete cookie session "${name}"? This cannot be undone.`))
      return;
    try {
      await invoke("delete_browser_cookie_session", { id });
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1 min-w-0">
          <h2 className="text-sm font-semibold flex items-center gap-2">
            <Cookie className="w-4 h-4" />
            Saved cookie sessions
          </h2>
          <p className="text-xs text-muted-foreground mt-1 leading-relaxed">
            Paste cookies once from a browser extension (Cookie-Editor,
            EditThisCookie) or a <code>cookies.txt</code> export. The Browser
            Agent will replay them on launch so you don&apos;t have to log in
            again. Cookie values are stored locally and never sent to the LLM.
          </p>
        </div>
        {!editing && (
          <Button
            size="sm"
            onClick={() => setEditing({})}
            className="gap-1.5 shrink-0"
          >
            <Plus className="w-3.5 h-3.5" />
            New session
          </Button>
        )}
      </div>

      {error && (
        <div className="flex items-start gap-2 px-3 py-2 rounded-md bg-destructive/10 text-destructive text-xs">
          <AlertCircle className="w-3.5 h-3.5 mt-0.5 shrink-0" />
          <span className="break-all">{error}</span>
        </div>
      )}

      {editing && (
        <CookieSessionForm
          existingId={editing.id}
          onSaved={async () => {
            setEditing(null);
            await refresh();
          }}
          onCancel={() => setEditing(null)}
          onError={setError}
        />
      )}

      {!editing && (
        <div className="space-y-2">
          {loading ? (
            <div className="flex items-center gap-2 text-xs text-muted-foreground py-4">
              <Loader2 className="w-3.5 h-3.5 animate-spin" />
              Loading sessions…
            </div>
          ) : sessions.length === 0 ? (
            <div className="text-xs text-muted-foreground py-4 px-3 rounded-md bg-muted/40">
              No saved cookie sessions yet. Click <strong>New session</strong>
              {" "}to add one.
            </div>
          ) : (
            sessions.map((s) => (
              <SessionRow
                key={s.id}
                session={s}
                onEdit={() => setEditing({ id: s.id })}
                onDelete={() => void handleDelete(s.id, s.name)}
              />
            ))
          )}
        </div>
      )}
    </div>
  );
}

function SessionRow({
  session,
  onEdit,
  onDelete,
}: {
  session: CookieSessionDigest;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const updated = useMemo(() => {
    try {
      return new Date(session.updated_at).toLocaleString();
    } catch {
      return session.updated_at;
    }
  }, [session.updated_at]);

  return (
    <div className="flex items-center gap-3 px-3 py-2.5 rounded-md border border-border bg-card hover:bg-card/70 transition">
      <KeyRound className="w-4 h-4 text-muted-foreground shrink-0" />
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium truncate">{session.name}</div>
        <div className="text-[11px] text-muted-foreground flex items-center gap-2 flex-wrap mt-0.5">
          <span>{session.cookie_count} cookies</span>
          {session.domains.length > 0 && (
            <>
              <span aria-hidden>·</span>
              <span className="truncate" title={session.domains.join(", ")}>
                {session.domains.slice(0, 3).join(", ")}
                {session.domains.length > 3 &&
                  ` +${session.domains.length - 3}`}
              </span>
            </>
          )}
          <span aria-hidden>·</span>
          <span title={`Updated ${updated}`}>updated {updated}</span>
        </div>
        {session.notes && (
          <div className="text-[11px] text-muted-foreground mt-1 truncate">
            {session.notes}
          </div>
        )}
      </div>
      <Button size="sm" variant="ghost" onClick={onEdit} className="h-7 px-2">
        Edit
      </Button>
      <Button
        size="icon"
        variant="ghost"
        onClick={onDelete}
        className="h-7 w-7 text-destructive hover:text-destructive"
        title="Delete"
      >
        <Trash2 className="w-3.5 h-3.5" />
      </Button>
    </div>
  );
}

// ───────────────────────────── Add / edit form ───────────────────────────

interface FormProps {
  existingId?: string;
  onSaved: () => Promise<void>;
  onCancel: () => void;
  onError: (msg: string) => void;
}

function CookieSessionForm({
  existingId,
  onSaved,
  onCancel,
  onError,
}: FormProps) {
  const [name, setName] = useState("");
  const [defaultDomain, setDefaultDomain] = useState("");
  const [notes, setNotes] = useState("");
  const [raw, setRaw] = useState("");
  const [preview, setPreview] = useState<ParsePreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [hydrating, setHydrating] = useState(!!existingId);

  // When editing, pre-fill from the existing record. We can't recover the raw
  // cookie blob (we only store canonical entries), so leave the textarea
  // empty — pasting again replaces the cookie list.
  useEffect(() => {
    let cancelled = false;
    if (!existingId) return;
    void (async () => {
      try {
        const list = await invoke<CookieSessionDigest[]>(
          "list_browser_cookie_sessions"
        );
        if (cancelled) return;
        const s = list.find((x) => x.id === existingId);
        if (s) {
          setName(s.name);
          setDefaultDomain(s.default_domain ?? "");
          setNotes(s.notes);
        }
      } finally {
        if (!cancelled) setHydrating(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [existingId]);

  // Debounced parse preview as the user types.
  useEffect(() => {
    if (!raw.trim()) {
      setPreview(null);
      setPreviewError(null);
      return;
    }
    const handle = setTimeout(async () => {
      try {
        const p = await invoke<ParsePreview>("preview_cookie_paste", {
          raw,
          defaultDomain: defaultDomain.trim() || null,
        });
        setPreview(p);
        setPreviewError(null);
      } catch (e) {
        setPreview(null);
        setPreviewError(String(e));
      }
    }, 300);
    return () => clearTimeout(handle);
  }, [raw, defaultDomain]);

  const canSave =
    !saving &&
    name.trim().length > 0 &&
    (existingId
      ? // editing: name change alone is enough; cookies optional.
        true
      : // creating: cookies required.
        !!preview && preview.cookie_count > 0);

  const handleSave = async () => {
    setSaving(true);
    try {
      if (existingId) {
        await invoke("update_browser_cookie_session", {
          id: existingId,
          name: name.trim() || null,
          raw: raw.trim() ? raw : null,
          defaultDomain: defaultDomain.trim()
            ? defaultDomain.trim()
            : raw.trim()
            ? null
            : undefined,
          notes: notes,
        });
      } else {
        await invoke("save_browser_cookie_session", {
          name: name.trim(),
          raw,
          defaultDomain: defaultDomain.trim() || null,
          notes: notes,
        });
      }
      await onSaved();
    } catch (e) {
      onError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="rounded-md border border-border bg-card/60 p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">
          {existingId ? "Edit cookie session" : "New cookie session"}
        </h3>
        <Button
          size="icon"
          variant="ghost"
          onClick={onCancel}
          className="h-7 w-7"
          title="Cancel"
        >
          <X className="w-3.5 h-3.5" />
        </Button>
      </div>

      {hydrating ? (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="w-3.5 h-3.5 animate-spin" />
          Loading…
        </div>
      ) : (
        <>
          <div>
            <label className="text-xs font-medium mb-1 block">Name</label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. LinkedIn — personal"
              className="h-8 text-sm"
            />
          </div>

          <div>
            <label className="text-xs font-medium mb-1 block">
              Default domain{" "}
              <span className="text-muted-foreground font-normal">
                (optional, used when cookies don&apos;t carry one)
              </span>
            </label>
            <Input
              value={defaultDomain}
              onChange={(e) => setDefaultDomain(e.target.value)}
              placeholder="linkedin.com"
              className="h-8 text-sm"
            />
          </div>

          <div>
            <label className="text-xs font-medium mb-1 block">
              Cookies
              {existingId && (
                <span className="text-muted-foreground font-normal">
                  {" "}— leave empty to keep current cookies
                </span>
              )}
            </label>
            <p className="text-[11px] text-muted-foreground mb-1.5">
              Paste from a Cookie-Editor JSON export, a <code>cookies.txt</code>
              {" "}file, or a raw <code>name=value; name2=value2</code> string.
            </p>
            <Textarea
              value={raw}
              onChange={(e) => setRaw(e.target.value)}
              placeholder={'[{"name": "li_at", "value": "AQEDA…", "domain": ".linkedin.com"}]'}
              rows={8}
              className="font-mono text-[11px]"
              spellCheck={false}
            />
            {previewError && (
              <div className="mt-2 text-[11px] text-destructive flex items-start gap-1.5">
                <AlertCircle className="w-3 h-3 mt-0.5 shrink-0" />
                <span className="break-all">{previewError}</span>
              </div>
            )}
            {preview && !previewError && (
              <div
                className={cn(
                  "mt-2 text-[11px] flex items-start gap-1.5 px-2 py-1.5 rounded",
                  preview.cookie_count > 0
                    ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                    : "bg-muted text-muted-foreground"
                )}
              >
                <CheckCircle2 className="w-3 h-3 mt-0.5 shrink-0" />
                <span>
                  Detected <strong>{preview.cookie_count}</strong> cookies
                  {preview.domains.length > 0 &&
                    ` for ${preview.domains.slice(0, 3).join(", ")}${
                      preview.domains.length > 3
                        ? ` +${preview.domains.length - 3} more`
                        : ""
                    }`}
                  {preview.missing_domain > 0 && (
                    <>
                      {" "}·{" "}
                      <span className="text-amber-500">
                        {preview.missing_domain} without a domain
                      </span>
                      {" — set a default domain above so they aren't dropped"}
                    </>
                  )}
                </span>
              </div>
            )}
          </div>

          <div>
            <label className="text-xs font-medium mb-1 block">
              Notes <span className="text-muted-foreground font-normal">(optional)</span>
            </label>
            <Input
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              placeholder="e.g. expires Dec 2026, work account"
              className="h-8 text-sm"
            />
          </div>

          <div className="flex items-center justify-end gap-2 pt-1">
            <Button size="sm" variant="ghost" onClick={onCancel}>
              Cancel
            </Button>
            <Button
              size="sm"
              onClick={() => void handleSave()}
              disabled={!canSave}
              className="gap-1.5"
            >
              {saving ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <Save className="w-3.5 h-3.5" />
              )}
              {existingId ? "Save changes" : "Save session"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
