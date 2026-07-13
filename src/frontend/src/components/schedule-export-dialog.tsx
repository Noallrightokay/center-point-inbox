"use client";

import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { CADENCES, DOCUMENT_FORMAT_LABEL, DOCUMENT_FORMATS } from "@/lib/enums";
import type { CentraDocument } from "@/types";
import { useToast } from "@/app/toast-provider";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const EXPORT_FORMATS = DOCUMENT_FORMATS.filter(
  (f) => f !== "Unknown" && !f.startsWith("Google")
);

export function ScheduleExportDialog({
  open,
  onOpenChange,
  documents,
  presetDocumentId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  documents: CentraDocument[];
  presetDocumentId?: string;
}) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const [documentId, setDocumentId] = useState(presetDocumentId ?? "");
  const [targetFormat, setTargetFormat] = useState("Pdf");
  const [cadence, setCadence] = useState("Daily");

  useEffect(() => {
    if (open) setDocumentId(presetDocumentId ?? "");
  }, [open, presetDocumentId]);

  const mutation = useMutation({
    mutationFn: () => api.createExport({ documentId, targetFormat, cadence }),
    onSuccess: () => {
      toast({ title: "Export scheduled", variant: "success" });
      qc.invalidateQueries({ queryKey: ["exports"] });
      onOpenChange(false);
    },
    onError: (err) =>
      toast({ title: "Could not schedule", description: (err as ApiError).message, variant: "error" }),
  });

  const selectCls =
    "h-9 w-full rounded-md border border-border bg-surface px-2.5 text-sm text-foreground outline-none focus:border-ring";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Schedule export</DialogTitle>
          <DialogDescription>
            Automatically render a document to a target format on a cadence.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="exp-doc">Document</Label>
            <select
              id="exp-doc"
              className={selectCls}
              value={documentId}
              onChange={(e) => setDocumentId(e.target.value)}
              disabled={!!presetDocumentId}
            >
              <option value="" disabled>
                Select a document…
              </option>
              {documents.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.title}
                </option>
              ))}
            </select>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="exp-fmt">Target format</Label>
              <select
                id="exp-fmt"
                className={selectCls}
                value={targetFormat}
                onChange={(e) => setTargetFormat(e.target.value)}
              >
                {EXPORT_FORMATS.map((f) => (
                  <option key={f} value={f}>
                    {DOCUMENT_FORMAT_LABEL[f]}
                  </option>
                ))}
              </select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="exp-cad">Cadence</Label>
              <select
                id="exp-cad"
                className={selectCls}
                value={cadence}
                onChange={(e) => setCadence(e.target.value)}
              >
                {CADENCES.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={!documentId || mutation.isPending}>
            {mutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            Schedule
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
