"use client";

import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Cable, Plus, RefreshCw, Trash2, AlertCircle, Loader2 } from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { relativeTime } from "@/lib/utils";
import { PROVIDER_TYPES, PROVIDER_LABEL, parseProvider, parseSyncStatus } from "@/lib/enums";
import type { Connection } from "@/types";
import { useToast } from "@/app/toast-provider";
import { PageHeader } from "@/components/page-header";
import { EmptyState } from "@/components/empty-state";
import { ProviderIcon } from "@/components/provider-icon";
import { ConnectionStatusPill, SyncStatusChip } from "@/components/status-pill";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const OAUTH_KEY = "centra-oauth-provider";

function openPopup(url: string) {
  const w = 560;
  const h = 720;
  const left = window.screenX + (window.outerWidth - w) / 2;
  const top = window.screenY + (window.outerHeight - h) / 2;
  return window.open(
    url,
    "centra-oauth",
    `width=${w},height=${h},left=${left},top=${top}`
  );
}

export default function ConnectionsPage() {
  const qc = useQueryClient();
  const { toast } = useToast();
  const [confirming, setConfirming] = useState<Connection | null>(null);
  const [connecting, setConnecting] = useState<string | null>(null);

  const { data: connections, isLoading } = useQuery({
    queryKey: ["connections"],
    queryFn: api.connections,
  });

  const { data: syncStatuses } = useQuery({
    queryKey: ["sync-status"],
    queryFn: api.syncStatus,
    refetchInterval: (q) => {
      const list = q.state.data;
      return list?.some((s) => parseSyncStatus(s.status) === "Running") ? 5000 : false;
    },
  });

  const syncFor = (id: string) => syncStatuses?.find((s) => s.connectionId === id);

  // Listen for the OAuth popup completion message.
  useEffect(() => {
    function onMessage(e: MessageEvent) {
      if (e.origin !== window.location.origin) return;
      if (e.data?.type !== "centra-oauth") return;
      setConnecting(null);
      if (e.data.ok) {
        toast({ title: "Account connected", variant: "success" });
        qc.invalidateQueries({ queryKey: ["connections"] });
      } else {
        toast({ title: "Connection failed", description: e.data.message, variant: "error" });
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  }, [qc, toast]);

  async function connect(provider: string) {
    setConnecting(provider);
    try {
      const { authorizationUrl } = await api.startConnection(provider);
      try {
        window.localStorage.setItem(OAUTH_KEY, provider);
      } catch {
        /* ignore */
      }
      const popup = openPopup(authorizationUrl);
      if (!popup) {
        setConnecting(null);
        toast({
          title: "Popup blocked",
          description: "Allow popups for this site to connect an account.",
          variant: "warning",
        });
      }
    } catch (err) {
      setConnecting(null);
      const e = err as ApiError;
      toast({ title: "Could not start connection", description: e.message, variant: "error" });
    }
  }

  const syncMutation = useMutation({
    mutationFn: (id: string) => api.sync(id),
    onSuccess: () => {
      toast({ title: "Sync started", variant: "success" });
      qc.invalidateQueries({ queryKey: ["sync-status"] });
    },
    onError: (err) =>
      toast({ title: "Sync failed", description: (err as ApiError).message, variant: "error" }),
  });

  const disconnectMutation = useMutation({
    mutationFn: (id: string) => api.disconnect(id),
    onSuccess: () => {
      toast({ title: "Account disconnected", variant: "success" });
      qc.invalidateQueries({ queryKey: ["connections"] });
    },
    onError: (err) =>
      toast({ title: "Disconnect failed", description: (err as ApiError).message, variant: "error" }),
    onSettled: () => setConfirming(null),
  });

  return (
    <div className="space-y-6">
      <PageHeader
        title="Connections"
        description="Linked accounts across your providers."
        actions={
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button>
                <Plus className="h-4 w-4" /> Connect
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {PROVIDER_TYPES.map((p) => (
                <DropdownMenuItem key={p} onClick={() => connect(p)} disabled={!!connecting}>
                  <ProviderIcon provider={p} size={18} />
                  {PROVIDER_LABEL[p]}
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        }
      />

      {isLoading ? (
        <div className="grid gap-3 sm:grid-cols-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Card key={i} className="p-4">
              <Skeleton className="h-10 w-10 rounded-md" />
              <Skeleton className="mt-3 h-4 w-1/2" />
              <Skeleton className="mt-2 h-3 w-1/3" />
            </Card>
          ))}
        </div>
      ) : !connections || connections.length === 0 ? (
        <EmptyState
          icon={Cable}
          title="Connect your first account"
          description="Link a provider to start indexing email and documents."
          action={
            <div className="flex flex-wrap justify-center gap-2">
              {PROVIDER_TYPES.map((p) => (
                <Button key={p} variant="outline" onClick={() => connect(p)} disabled={!!connecting}>
                  {connecting === p ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <ProviderIcon provider={p} size={18} />
                  )}
                  {PROVIDER_LABEL[p]}
                </Button>
              ))}
            </div>
          }
        />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2">
          {connections.map((c) => {
            const sync = syncFor(c.id);
            const running = sync && parseSyncStatus(sync.status) === "Running";
            return (
              <Card key={c.id} className="p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex min-w-0 items-start gap-3">
                    <ProviderIcon provider={c.provider} size={38} className="rounded-md" />
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium text-foreground">
                        {c.accountEmail}
                      </p>
                      <p className="text-xs text-muted-foreground">
                        {c.displayName || PROVIDER_LABEL[parseProvider(c.provider)]}
                      </p>
                    </div>
                  </div>
                  <ConnectionStatusPill status={c.status} />
                </div>

                <div className="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
                  <span>Last sync {relativeTime(c.lastSyncAt)}</span>
                  {sync && (
                    <>
                      <span>·</span>
                      <SyncStatusChip status={sync.status} />
                      {typeof sync.itemsIndexed === "number" && (
                        <span>{sync.itemsIndexed.toLocaleString()} indexed</span>
                      )}
                    </>
                  )}
                </div>

                {c.lastError && (
                  <div className="mt-3 flex items-start gap-2 rounded-md border border-destructive/25 bg-destructive/10 px-2.5 py-1.5 text-xs text-destructive">
                    <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                    <span className="min-w-0 break-words">{c.lastError}</span>
                  </div>
                )}

                <div className="mt-4 flex items-center gap-2">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => syncMutation.mutate(c.id)}
                    disabled={running || (syncMutation.isPending && syncMutation.variables === c.id)}
                  >
                    <RefreshCw className={running ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                    {running ? "Syncing…" : "Sync now"}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-muted-foreground hover:text-destructive"
                    onClick={() => setConfirming(c)}
                  >
                    <Trash2 className="h-4 w-4" /> Disconnect
                  </Button>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      <Dialog open={!!confirming} onOpenChange={(o) => !o && setConfirming(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Disconnect account?</DialogTitle>
            <DialogDescription>
              {confirming?.accountEmail} will be unlinked and its indexed items removed from search.
              This can&apos;t be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirming(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => confirming && disconnectMutation.mutate(confirming.id)}
              disabled={disconnectMutation.isPending}
            >
              {disconnectMutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              Disconnect
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
