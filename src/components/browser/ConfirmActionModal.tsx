import { AlertTriangle } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useBrowserAgentStore } from "@/stores/browserAgentStore";

/**
 * Modal rendered when the backend `SafetyGate` raises a confirmation request
 * (e.g. cross-origin submit, sensitive domain navigation, download). The user
 * must explicitly approve or deny before the agent proceeds.
 */
export function ConfirmActionModal() {
  const pending = useBrowserAgentStore((s) => s.pendingConfirm);
  const resolve = useBrowserAgentStore((s) => s.resolveConfirmation);

  const open = !!pending;

  if (!pending) {
    return <Dialog open={false} onOpenChange={() => undefined} />;
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) resolve(pending.request_id, false);
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <AlertTriangle className="w-5 h-5 text-amber-400" />
            Confirm action
          </DialogTitle>
          <DialogDescription>
            The agent wants to perform an action that requires your approval.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 text-sm">
          <div>
            <span className="text-muted-foreground">Action: </span>
            <span className="font-mono">{pending.action}</span>
          </div>
          {pending.target && (
            <div>
              <span className="text-muted-foreground">Target: </span>
              <span className="font-mono break-all">{pending.target}</span>
            </div>
          )}
          {pending.rationale && (
            <div className="text-xs text-muted-foreground border-l-2 border-amber-400/50 pl-2 py-1">
              {pending.rationale}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => resolve(pending.request_id, false)}
          >
            Deny
          </Button>
          <Button
            variant="default"
            onClick={() => resolve(pending.request_id, true)}
          >
            Approve
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
