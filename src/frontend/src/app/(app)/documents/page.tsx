"use client";

import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  FileText,
  Plus,
  Languages,
  CalendarClock,
  Trash2,
  Loader2,
  Save,
  Check,
} from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { relativeTime } from "@/lib/utils";
import { renderMarkdown } from "@/lib/markdown";
import {
  AUTHORABLE_FORMATS,
  DOCUMENT_FORMAT_LABEL,
  DOCUMENT_FORMATS,
  parseDocumentFormat,
} from "@/lib/enums";
import type { CentraDocument } from "@/types";
import { useToast } from "@/app/toast-provider";
import { PageHeader } from "@/components/page-header";
import { EmptyState } from "@/components/empty-state";
import { DocumentFormatChip } from "@/components/chips";
import { ScheduleExportDialog } from "@/components/schedule-export-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { Sheet, SheetContent } from "@/components/ui/sheet";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

type Editor =
  | { mode: "closed" }
  | { mode: "new" }
  | { mode: "edit"; doc: CentraDocument };

const TRANSLATE_TARGETS = DOCUMENT_FORMATS.filter((f) => f !== "Unknown");

export default function DocumentsPage() {
  const qc = useQueryClient();
  const { toast } = useToast();
  const [editor, setEditor] = useState<Editor>({ mode: "closed" });
  const [translateDoc, setTranslateDoc] = useState<CentraDocument | null>(null);
  const [exportDoc, setExportDoc] = useState<CentraDocument | null>(null);
  const [deleteDoc, setDeleteDoc] = useState<CentraDocument | null>(null);

  const { data: documents, isLoading } = useQuery({
    queryKey: ["documents"],
    queryFn: api.documents,
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteDocument(id),
    onSuccess: () => {
      toast({ title: "Document deleted", variant: "success" });
      qc.invalidateQueries({ queryKey: ["documents"] });
    },
    onError: (err) =>
      toast({ title: "Delete failed", description: (err as ApiError).message, variant: "error" }),
    onSettled: () => setDeleteDoc(null),
  });

  return (
    <div className="space-y-6">
      <PageHeader
        title="Documents"
        description="Author and manage native Centra documents."
        actions={
          <Button onClick={() => setEditor({ mode: "new" })}>
            <Plus className="h-4 w-4" /> New document
          </Button>
        }
      />

      {isLoading ? (
        <div className="space-y-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-14 w-full" />
          ))}
        </div>
      ) : !documents || documents.length === 0 ? (
        <EmptyState
          icon={FileText}
          title="Create your first document"
          description="Draft notes, briefs, or structured content that Centra can translate and export."
          action={
            <Button onClick={() => setEditor({ mode: "new" })}>
              <Plus className="h-4 w-4" /> New document
            </Button>
          }
        />
      ) : (
        <div className="overflow-hidden rounded-md border border-border">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                <th className="px-4 py-2.5 font-medium">Title</th>
                <th className="px-4 py-2.5 font-medium">Format</th>
                <th className="px-4 py-2.5 font-medium">Version</th>
                <th className="px-4 py-2.5 font-medium">Updated</th>
                <th className="px-4 py-2.5" />
              </tr>
            </thead>
            <tbody>
              {documents.map((d) => (
                <tr
                  key={d.id}
                  className="cursor-pointer border-b border-border last:border-0 hover:bg-muted/30"
                  onClick={() => setEditor({ mode: "edit", doc: d })}
                >
                  <td className="max-w-0 px-4 py-3">
                    <span className="block truncate font-medium text-foreground">{d.title}</span>
                  </td>
                  <td className="px-4 py-3">
                    <DocumentFormatChip format={d.format} />
                  </td>
                  <td className="px-4 py-3 font-mono text-xs text-muted-foreground">v{d.version}</td>
                  <td className="px-4 py-3 text-xs text-muted-foreground">
                    {relativeTime(d.updatedAt)}
                  </td>
                  <td className="px-4 py-3" onClick={(e) => e.stopPropagation()}>
                    <div className="flex items-center justify-end gap-0.5">
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        title="Translate"
                        onClick={() => setTranslateDoc(d)}
                      >
                        <Languages className="h-4 w-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        title="Schedule export"
                        onClick={() => setExportDoc(d)}
                      >
                        <CalendarClock className="h-4 w-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        title="Delete"
                        className="text-muted-foreground hover:text-destructive"
                        onClick={() => setDeleteDoc(d)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Editor drawer */}
      <Sheet open={editor.mode !== "closed"} onOpenChange={(o) => !o && setEditor({ mode: "closed" })}>
        <SheetContent side="right" className="w-full p-0 sm:max-w-none md:w-[70vw] lg:w-[64vw]">
          {editor.mode !== "closed" && (
            <DocumentEditor
              key={editor.mode === "edit" ? editor.doc.id : "new"}
              initial={editor.mode === "edit" ? editor.doc : null}
              onClose={() => setEditor({ mode: "closed" })}
            />
          )}
        </SheetContent>
      </Sheet>

      {/* Translate dialog */}
      <TranslateDocumentDialog doc={translateDoc} onClose={() => setTranslateDoc(null)} />

      {/* Schedule export dialog */}
      <ScheduleExportDialog
        open={!!exportDoc}
        onOpenChange={(o) => !o && setExportDoc(null)}
        documents={documents ?? []}
        presetDocumentId={exportDoc?.id}
      />

      {/* Delete confirm */}
      <Dialog open={!!deleteDoc} onOpenChange={(o) => !o && setDeleteDoc(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete document?</DialogTitle>
            <DialogDescription>
              “{deleteDoc?.title}” will be soft-deleted. You can ask an administrator to restore it.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteDoc(null)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => deleteDoc && deleteMutation.mutate(deleteDoc.id)}
              disabled={deleteMutation.isPending}
            >
              {deleteMutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function DocumentEditor({
  initial,
  onClose,
}: {
  initial: CentraDocument | null;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const [doc, setDoc] = useState<CentraDocument | null>(initial);
  const [title, setTitle] = useState(initial?.title ?? "");
  const [format, setFormat] = useState<string>(
    initial ? parseDocumentFormat(initial.format) : "Markdown"
  );
  const [content, setContent] = useState(initial?.content ?? "");
  const [savedAt, setSavedAt] = useState<number | null>(null);

  const isMarkdown = format === "Markdown";
  const version = doc?.version ?? 0;

  const dirty = useMemo(() => {
    if (!doc) return title.trim().length > 0 || content.length > 0;
    return title !== doc.title || content !== (doc.content ?? "");
  }, [doc, title, content]);

  const createMutation = useMutation({
    mutationFn: () => api.createDocument({ title: title.trim() || "Untitled", format, content }),
    onSuccess: (created) => {
      setDoc(created);
      setSavedAt(Date.now());
      qc.invalidateQueries({ queryKey: ["documents"] });
      toast({ title: "Document created", variant: "success" });
    },
    onError: (err) =>
      toast({ title: "Save failed", description: (err as ApiError).message, variant: "error" }),
  });

  const updateMutation = useMutation({
    mutationFn: () => api.updateDocument(doc!.id, { title: title.trim() || "Untitled", content }),
    onSuccess: (updated) => {
      setDoc(updated);
      setSavedAt(Date.now());
      qc.invalidateQueries({ queryKey: ["documents"] });
    },
    onError: (err) =>
      toast({ title: "Save failed", description: (err as ApiError).message, variant: "error" }),
  });

  const saving = createMutation.isPending || updateMutation.isPending;

  function save() {
    if (saving) return;
    if (!doc) createMutation.mutate();
    else if (dirty) updateMutation.mutate();
  }

  // Autosave-on-blur for existing documents only.
  function handleBlur() {
    if (doc && dirty && !saving) updateMutation.mutate();
  }

  useEffect(() => {
    if (savedAt) {
      const t = setTimeout(() => setSavedAt(null), 2000);
      return () => clearTimeout(t);
    }
  }, [savedAt]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between gap-3 border-b border-border px-5 py-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="font-mono text-xs text-muted-foreground">v{version}</span>
          {saving ? (
            <span className="flex items-center gap-1 text-xs text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" /> Saving…
            </span>
          ) : savedAt ? (
            <span className="flex items-center gap-1 text-xs text-success">
              <Check className="h-3 w-3" /> Saved
            </span>
          ) : dirty ? (
            <span className="text-xs text-muted-foreground">Unsaved changes</span>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={save} disabled={saving || (!!doc && !dirty)}>
            <Save className="h-4 w-4" /> Save
          </Button>
          <Button size="sm" variant="outline" onClick={onClose}>
            Close
          </Button>
        </div>
      </div>

      <div className="space-y-3 border-b border-border px-5 py-3">
        <Input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={handleBlur}
          placeholder="Document title"
          className="h-10 border-0 bg-transparent px-0 text-lg font-semibold shadow-none focus-visible:ring-0"
        />
        <div className="flex items-center gap-2">
          <Label htmlFor="doc-format">Format</Label>
          <select
            id="doc-format"
            value={format}
            onChange={(e) => setFormat(e.target.value)}
            disabled={!!doc}
            className="h-8 rounded-md border border-border bg-surface px-2 text-sm text-foreground outline-none focus:border-ring disabled:opacity-60"
          >
            {(doc ? DOCUMENT_FORMATS.filter((f) => f !== "Unknown") : AUTHORABLE_FORMATS).map((f) => (
              <option key={f} value={f}>
                {DOCUMENT_FORMAT_LABEL[f]}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className={`min-h-0 flex-1 ${isMarkdown ? "grid md:grid-cols-2" : ""}`}>
        <Textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          onBlur={handleBlur}
          placeholder={isMarkdown ? "# Write Markdown…" : "Start writing…"}
          className="h-full resize-none rounded-none border-0 border-border font-mono text-sm shadow-none focus-visible:ring-0 md:border-r"
        />
        {isMarkdown && (
          <div
            className="h-full overflow-y-auto px-5 py-3 text-sm"
            dangerouslySetInnerHTML={{ __html: renderMarkdown(content) }}
          />
        )}
      </div>
    </div>
  );
}

function TranslateDocumentDialog({
  doc,
  onClose,
}: {
  doc: CentraDocument | null;
  onClose: () => void;
}) {
  const { toast } = useToast();
  const qc = useQueryClient();
  const [target, setTarget] = useState("Pdf");

  const mutation = useMutation({
    mutationFn: () =>
      api.createTranslationJob({ nativeDocumentId: doc!.id, targetFormat: target }),
    onSuccess: () => {
      toast({ title: "Translation queued", variant: "success" });
      qc.invalidateQueries({ queryKey: ["translation-jobs"] });
      onClose();
    },
    onError: (err) =>
      toast({ title: "Could not queue", description: (err as ApiError).message, variant: "error" }),
  });

  return (
    <Dialog open={!!doc} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Translate document</DialogTitle>
          <DialogDescription>
            Convert “{doc?.title}” into another format. A job is queued and appears in Translation
            Studio.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-1.5">
          <Label htmlFor="tr-target">Target format</Label>
          <select
            id="tr-target"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            className="h-9 w-full rounded-md border border-border bg-surface px-2.5 text-sm text-foreground outline-none focus:border-ring"
          >
            {TRANSLATE_TARGETS.map((f) => (
              <option key={f} value={f}>
                {DOCUMENT_FORMAT_LABEL[f]}
              </option>
            ))}
          </select>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            {mutation.isPending && <Loader2 className="h-4 w-4 animate-spin" />}
            Queue translation
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
