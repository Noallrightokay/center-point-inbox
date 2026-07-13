"use client";

import { useCallback, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  Minus,
  UploadCloud,
  Languages,
  Loader2,
  Plus,
  Trash2,
  ArrowRight,
  FileOutput,
  ServerCog,
} from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { cn, formatDuration, relativeTime } from "@/lib/utils";
import {
  PROVIDER_TYPES,
  PROVIDER_LABEL,
  DOCUMENT_FORMATS,
  DOCUMENT_FORMAT_LABEL,
  parseDocumentFormat,
  parseProvider,
  parseTranslationStatus,
  type ProviderType,
} from "@/lib/enums";
import type { FormatPreference } from "@/types";
import { useToast } from "@/app/toast-provider";
import { PageHeader } from "@/components/page-header";
import { EmptyState } from "@/components/empty-state";
import { JobStatusChip } from "@/components/status-pill";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";

const CONVERT_TARGETS = DOCUMENT_FORMATS.filter((f) => f !== "Unknown");
const selectCls =
  "h-8 rounded-md border border-border bg-surface px-2 text-sm text-foreground outline-none focus:border-ring";

export default function TranslationPage() {
  return (
    <div className="space-y-6">
      <PageHeader
        title="Translation Studio"
        description="Convert formats, queue jobs, and set standing preferences."
      />
      <div className="grid gap-6 lg:grid-cols-2">
        <QuickConvert />
        <ConversionMatrix />
      </div>
      <JobQueue />
      <FormatPreferences />
    </div>
  );
}

function ConversionMatrix() {
  const { data, isLoading } = useQuery({
    queryKey: ["translation-matrix"],
    queryFn: api.translationMatrix,
    refetchInterval: 15000,
  });

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <CardTitle className="text-base">Conversion matrix</CardTitle>
        {data && (
          <Badge variant={data.gotenbergAvailable ? "success" : "warning"} className="gap-1.5">
            <ServerCog className="h-3 w-3" />
            Gotenberg {data.gotenbergAvailable ? "online" : "offline"}
          </Badge>
        )}
      </CardHeader>
      <CardContent>
        {isLoading ? (
          <Skeleton className="h-40 w-full" />
        ) : !data ? (
          <p className="text-sm text-muted-foreground">Matrix unavailable.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-xs">
              <thead>
                <tr>
                  <th className="p-1.5 text-left font-medium text-muted-foreground">from ／ to</th>
                  {data.targets.map((t) => (
                    <th key={t} className="p-1.5 text-center font-mono font-normal text-muted-foreground">
                      {t}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {data.sources.map((s) => (
                  <tr key={s} className="border-t border-border">
                    <td className="p-1.5 font-mono text-muted-foreground">{s}</td>
                    {data.targets.map((t) => {
                      const ok = data.supported?.[s]?.[t];
                      return (
                        <td key={t} className="p-1.5 text-center">
                          {ok ? (
                            <Check className="mx-auto h-3.5 w-3.5 text-success" />
                          ) : (
                            <Minus className="mx-auto h-3 w-3 text-muted-foreground/40" />
                          )}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function QuickConvert() {
  const { toast } = useToast();
  const [file, setFile] = useState<File | null>(null);
  const [target, setTarget] = useState("Pdf");
  const [dragging, setDragging] = useState(false);

  const mutation = useMutation({
    mutationFn: () => api.convert(file!, target),
    onSuccess: () => toast({ title: "Converted — download started", variant: "success" }),
    onError: (err) =>
      toast({ title: "Conversion failed", description: (err as ApiError).message, variant: "error" }),
  });

  const onDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragging(false);
    const f = e.dataTransfer.files?.[0];
    if (f) setFile(f);
  }, []);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Quick convert</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <label
          onDragOver={(e) => {
            e.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={onDrop}
          className={cn(
            "flex cursor-pointer flex-col items-center justify-center gap-2 rounded-md border border-dashed px-4 py-8 text-center transition-colors",
            dragging ? "border-primary bg-primary/5" : "border-border hover:border-ring/50"
          )}
        >
          <UploadCloud className="h-6 w-6 text-muted-foreground" />
          {file ? (
            <span className="text-sm font-medium text-foreground">{file.name}</span>
          ) : (
            <span className="text-sm text-muted-foreground">
              Drop a file here, or click to choose
            </span>
          )}
          <input
            type="file"
            className="hidden"
            onChange={(e) => setFile(e.target.files?.[0] ?? null)}
          />
        </label>

        <div className="flex items-center gap-2">
          <Label htmlFor="qc-target" className="shrink-0">
            Convert to
          </Label>
          <select
            id="qc-target"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            className={cn(selectCls, "flex-1")}
          >
            {CONVERT_TARGETS.map((f) => (
              <option key={f} value={f}>
                {DOCUMENT_FORMAT_LABEL[f]}
              </option>
            ))}
          </select>
          <Button
            size="sm"
            disabled={!file || mutation.isPending}
            onClick={() => mutation.mutate()}
          >
            {mutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Languages className="h-4 w-4" />
            )}
            Convert
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function JobQueue() {
  const qc = useQueryClient();
  const { toast } = useToast();

  const { data: documents } = useQuery({ queryKey: ["documents"], queryFn: api.documents });
  const { data: jobs, isLoading } = useQuery({
    queryKey: ["translation-jobs"],
    queryFn: api.translationJobs,
    refetchInterval: (q) => {
      const list = q.state.data;
      const active = list?.some((j) => {
        const s = parseTranslationStatus(j.status);
        return s === "Queued" || s === "Processing";
      });
      return active ? 3000 : false;
    },
  });

  const [docId, setDocId] = useState("");
  const [target, setTarget] = useState("Pdf");

  const enqueue = useMutation({
    mutationFn: () => api.createTranslationJob({ nativeDocumentId: docId, targetFormat: target }),
    onSuccess: () => {
      toast({ title: "Job queued", variant: "success" });
      qc.invalidateQueries({ queryKey: ["translation-jobs"] });
    },
    onError: (err) =>
      toast({ title: "Could not queue", description: (err as ApiError).message, variant: "error" }),
  });

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Job queue</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/30 p-2.5">
          <span className="text-xs text-muted-foreground">Enqueue from document</span>
          <select value={docId} onChange={(e) => setDocId(e.target.value)} className={selectCls}>
            <option value="">Select document…</option>
            {documents?.map((d) => (
              <option key={d.id} value={d.id}>
                {d.title}
              </option>
            ))}
          </select>
          <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" />
          <select value={target} onChange={(e) => setTarget(e.target.value)} className={selectCls}>
            {CONVERT_TARGETS.map((f) => (
              <option key={f} value={f}>
                {DOCUMENT_FORMAT_LABEL[f]}
              </option>
            ))}
          </select>
          <Button size="sm" disabled={!docId || enqueue.isPending} onClick={() => enqueue.mutate()}>
            <Plus className="h-4 w-4" /> Queue
          </Button>
        </div>

        {isLoading ? (
          <Skeleton className="h-24 w-full" />
        ) : !jobs || jobs.length === 0 ? (
          <EmptyState icon={Languages} title="No jobs yet" description="Queued conversions appear here." />
        ) : (
          <div className="overflow-hidden rounded-md border border-border">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                  <th className="px-3 py-2 font-medium">Source</th>
                  <th className="px-3 py-2 font-medium">Target</th>
                  <th className="px-3 py-2 font-medium">Status</th>
                  <th className="px-3 py-2 font-medium">Duration</th>
                  <th className="px-3 py-2 font-medium">Output</th>
                </tr>
              </thead>
              <tbody>
                {jobs.map((j) => (
                  <tr key={j.id} className="border-b border-border last:border-0">
                    <td className="max-w-[200px] truncate px-3 py-2 text-foreground">
                      {j.sourceLabel || j.nativeDocumentId || j.sourceExternalId || "—"}
                    </td>
                    <td className="px-3 py-2 text-muted-foreground">
                      {DOCUMENT_FORMAT_LABEL[parseDocumentFormat(j.targetFormat)]}
                    </td>
                    <td className="px-3 py-2">
                      <JobStatusChip status={j.status} />
                      {j.error && (
                        <span className="ml-1 block max-w-[220px] truncate text-xs text-destructive">
                          {j.error}
                        </span>
                      )}
                    </td>
                    <td className="px-3 py-2 font-mono text-xs text-muted-foreground">
                      {formatDuration(j.durationMs)}
                    </td>
                    <td className="px-3 py-2">
                      {j.outputDocumentId ? (
                        <a
                          href="/documents"
                          className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline"
                        >
                          <FileOutput className="h-3.5 w-3.5" /> Open
                        </a>
                      ) : (
                        <span className="text-xs text-muted-foreground">
                          {relativeTime(j.createdAt)}
                        </span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function FormatPreferences() {
  const qc = useQueryClient();
  const { toast } = useToast();
  const { data: prefs, isLoading } = useQuery({
    queryKey: ["format-preferences"],
    queryFn: api.formatPreferences,
  });

  const [sourceFormat, setSourceFormat] = useState("Word");
  const [provider, setProvider] = useState<"Anywhere" | ProviderType>("Anywhere");
  const [targetFormat, setTargetFormat] = useState("Pdf");
  const [autoApply, setAutoApply] = useState(true);

  const create = useMutation({
    mutationFn: (input: {
      sourceFormat: string;
      provider: string | null;
      targetFormat: string;
      autoApply: boolean;
    }) => api.createFormatPreference(input),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["format-preferences"] }),
    onError: (err) =>
      toast({ title: "Could not save", description: (err as ApiError).message, variant: "error" }),
  });

  const remove = useMutation({
    mutationFn: (id: string) => api.deleteFormatPreference(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["format-preferences"] }),
    onError: (err) =>
      toast({ title: "Could not delete", description: (err as ApiError).message, variant: "error" }),
  });

  function addRule() {
    create.mutate({
      sourceFormat,
      provider: provider === "Anywhere" ? null : provider,
      targetFormat,
      autoApply,
    });
  }

  // No PUT endpoint: toggle AutoApply by replacing the rule.
  async function toggleAuto(p: FormatPreference) {
    await remove.mutateAsync(p.id);
    create.mutate({
      sourceFormat: parseDocumentFormat(p.sourceFormat),
      provider: p.provider == null ? null : parseProvider(p.provider),
      targetFormat: parseDocumentFormat(p.targetFormat),
      autoApply: !p.autoApply,
    });
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Format preferences</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/30 p-2.5 text-sm">
          <span className="text-muted-foreground">When I get</span>
          <select value={sourceFormat} onChange={(e) => setSourceFormat(e.target.value)} className={selectCls}>
            {CONVERT_TARGETS.map((f) => (
              <option key={f} value={f}>
                {DOCUMENT_FORMAT_LABEL[f]}
              </option>
            ))}
          </select>
          <span className="text-muted-foreground">from</span>
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as "Anywhere" | ProviderType)}
            className={selectCls}
          >
            <option value="Anywhere">Anywhere</option>
            {PROVIDER_TYPES.map((p) => (
              <option key={p} value={p}>
                {PROVIDER_LABEL[p]}
              </option>
            ))}
          </select>
          <span className="text-muted-foreground">give me</span>
          <select value={targetFormat} onChange={(e) => setTargetFormat(e.target.value)} className={selectCls}>
            {CONVERT_TARGETS.map((f) => (
              <option key={f} value={f}>
                {DOCUMENT_FORMAT_LABEL[f]}
              </option>
            ))}
          </select>
          <label className="flex items-center gap-1.5 text-muted-foreground">
            Auto <Switch checked={autoApply} onCheckedChange={setAutoApply} aria-label="Auto-apply" />
          </label>
          <Button size="sm" onClick={addRule} disabled={create.isPending}>
            <Plus className="h-4 w-4" /> Add
          </Button>
        </div>

        {isLoading ? (
          <Skeleton className="h-20 w-full" />
        ) : !prefs || prefs.length === 0 ? (
          <p className="py-2 text-sm text-muted-foreground">No preferences yet.</p>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border">
            {prefs.map((p) => (
              <li key={p.id} className="flex items-center gap-2 px-3 py-2.5 text-sm">
                <span className="flex flex-1 flex-wrap items-center gap-1.5">
                  <span className="text-muted-foreground">When I get</span>
                  <Badge variant="secondary">{DOCUMENT_FORMAT_LABEL[parseDocumentFormat(p.sourceFormat)]}</Badge>
                  <span className="text-muted-foreground">from</span>
                  <Badge variant="secondary">
                    {p.provider == null ? "Anywhere" : PROVIDER_LABEL[parseProvider(p.provider)]}
                  </Badge>
                  <ArrowRight className="h-3.5 w-3.5 text-muted-foreground" />
                  <Badge variant="default">{DOCUMENT_FORMAT_LABEL[parseDocumentFormat(p.targetFormat)]}</Badge>
                </span>
                <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  Auto
                  <Switch
                    checked={p.autoApply}
                    onCheckedChange={() => toggleAuto(p)}
                    aria-label="Toggle auto-apply"
                  />
                </label>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="text-muted-foreground hover:text-destructive"
                  onClick={() => remove.mutate(p.id)}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
