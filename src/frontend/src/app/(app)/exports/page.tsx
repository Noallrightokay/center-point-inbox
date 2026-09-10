"use client";

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CalendarClock, Plus, Trash2, AlertTriangle, Loader2, Ban } from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { relativeTime } from "@/lib/utils";
import { DOCUMENT_FORMAT_LABEL, parseCadence, parseDocumentFormat } from "@/lib/enums";
import type { ScheduledExport } from "@/types";
import { useToast } from "@/app/toast-provider";
import { PageHeader } from "@/components/page-header";
import { EmptyState } from "@/components/empty-state";
import { ScheduleExportDialog } from "@/components/schedule-export-dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

function FailureBadge({ count }: { count: number }) {
  if (count >= 5) {
    return (
      <Badge variant="destructive" className="gap-1">
        <Ban className="h-3 w-3" /> Auto-disabled
      </Badge>
    );
  }
  if (count >= 3) {
    return (
      <Badge variant="warning" className="gap-1">
        <AlertTriangle className="h-3 w-3" /> {count} failures
      </Badge>
    );
  }
  if (count > 0) {
    return <span className="text-xs text-muted-foreground">{count} failures</span>;
  }
  return <span className="text-xs text-muted-foreground">—</span>;
}

export default function ExportsPage() {
  const qc = useQueryClient();
  const { toast } = useToast();
  const [createOpen, setCreateOpen] = useState(false);
  const [deleting, setDeleting] = useState<ScheduledExport | null>(null);

  const { data: exportsList, isLoading } = useQuery({
    queryKey: ["exports"],
    queryFn: api.exports,
  });
  const { data: documents } = useQuery({ queryKey: ["documents"], queryFn: api.documents });

  const titleFor = useMemo(() => {
    const map = new Map(documents?.map((d) => [d.id, d.title]));
    return (id: string) => map.get(id) ?? id;
  }, [documents]);

  const remove = useMutation({
    mutationFn: (id: string) => api.deleteExport(id),
    onSuccess: () => {
      toast({ title: "Export removed", variant: "success" });
      qc.invalidateQueries({ queryKey: ["exports"] });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: (err as ApiError).message, variant: "error" }),
    onSettled: () => setDeleting(null),
  });

  return (
    <div className="space-y-6">
      <PageHeader
        title="Scheduled Exports"
        description="Recurring renders of your documents to a target format."
        actions={
          <Button onClick={() => setCreateOpen(true)}>
            <Plus className="h-4 w-4" /> New export
          </Button>
        }
      />

      {isLoading ? (
        <div className="space-y-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-14 w-full" />
          ))}
        </div>
      ) : !exportsList || exportsList.length === 0 ? (
        <EmptyState
          icon={CalendarClock}
          title="No scheduled exports"
          description="Schedule a document to render automatically on a cadence."
          action={
            <Button onClick={() => setCreateOpen(true)}>
              <Plus className="h-4 w-4" /> New export
            </Button>
          }
        />
      ) : (
        <div className="overflow-x-auto rounded-md border border-border">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                <th className="px-4 py-2.5 font-medium">Document</th>
                <th className="px-4 py-2.5 font-medium">Format</th>
                <th className="px-4 py-2.5 font-medium">Cadence</th>
                <th className="px-4 py-2.5 font-medium">Status</th>
                <th className="px-4 py-2.5 font-medium">Health</th>
                <th className="px-4 py-2.5 font-medium">Last run</th>
                <th className="px-4 py-2.5 font-medium">Next run</th>
                <th className="px-4 py-2.5" />
              </tr>
            </thead>
            <tbody>
              {exportsList.map((x) => (
                <tr key={x.id} className="border-b border-border last:border-0 hover:bg-muted/20">
                  <td className="max-w-[220px] px-4 py-3">
                    <span className="block truncate font-medium text-foreground">
                      {titleFor(x.documentId)}
                    </span>
                  </td>
                  <td className="px-4 py-3">
                    <Badge variant="secondary">
                      {DOCUMENT_FORMAT_LABEL[parseDocumentFormat(x.targetFormat)]}
                    </Badge>
                  </td>
                  <td className="px-4 py-3 text-muted-foreground">{parseCadence(x.cadence)}</td>
                  <td className="px-4 py-3">
                    {x.enabled ? (
                      <Badge variant="success">Enabled</Badge>
                    ) : (
                      <Badge variant="secondary">Disabled</Badge>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    <FailureBadge count={x.consecutiveFailures} />
                  </td>
                  <td className="px-4 py-3 text-xs text-muted-foreground">
                    {relativeTime(x.lastRunAt)}
                  </td>
                  <td className="px-4 py-3 text-xs text-muted-foreground">
                    {relativeTime(x.nextRunAt)}
                  </td>
                  <td className="px-4 py-3 text-right">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      title="Delete"
                      className="text-muted-foreground hover:text-destructive"
                      onClick={() => setDeleting(x)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <ScheduleExportDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        documents={documents ?? []}
      />

      <Dialog open={!!deleting} onOpenChange={(o) => !o && setDeleting(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove scheduled export?</DialogTitle>
            <DialogDescription>
              “{deleting && titleFor(deleting.documentId)}” will stop rendering automatically.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleting(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => deleting && remove.mutate(deleting.id)}
              disabled={remove.isPending}
            >
              {remove.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              Remove
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
