"use client";

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Download,
  Upload,
  FileIcon,
  ExternalLink,
  Inbox,
  FolderOpen,
  Loader2,
  Cable,
} from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { cn, formatBytes, relativeTime } from "@/lib/utils";
import { PROVIDER_LABEL, parseProvider } from "@/lib/enums";
import type { Connection, RemoteFile } from "@/types";
import { useToast } from "@/app/toast-provider";
import { PageHeader } from "@/components/page-header";
import { EmptyState } from "@/components/empty-state";
import { ProviderIcon } from "@/components/provider-icon";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const GOOGLE_EXPORTS: { label: string; mime: string }[] = [
  { label: "Word (.docx)", mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document" },
  { label: "Excel (.xlsx)", mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" },
  { label: "PowerPoint (.pptx)", mime: "application/vnd.openxmlformats-officedocument.presentationml.presentation" },
  { label: "PDF (.pdf)", mime: "application/pdf" },
];

function ConnectionPicker({
  connections,
  selectedId,
  onSelect,
}: {
  connections: Connection[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {connections.map((c) => (
        <button
          key={c.id}
          onClick={() => onSelect(c.id)}
          className={cn(
            "flex items-center gap-2 rounded-md border px-3 py-1.5 text-sm transition-colors",
            selectedId === c.id
              ? "border-primary bg-primary/10 text-foreground"
              : "border-border bg-surface text-muted-foreground hover:text-foreground"
          )}
        >
          <ProviderIcon provider={c.provider} size={16} />
          {c.accountEmail}
        </button>
      ))}
    </div>
  );
}

export default function BrowserPage() {
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const { data: connections, isLoading: loadingConns } = useQuery({
    queryKey: ["connections"],
    queryFn: api.connections,
  });

  const selected = connections?.find((c) => c.id === selectedId) ?? connections?.[0] ?? null;
  const activeId = selected?.id ?? null;

  return (
    <div className="space-y-6">
      <PageHeader title="Files & Email" description="Browse and move content inside a connection." />

      {loadingConns ? (
        <Skeleton className="h-10 w-full max-w-md" />
      ) : !connections || connections.length === 0 ? (
        <EmptyState
          icon={Cable}
          title="No connections yet"
          description="Connect an account to browse its files and email."
          action={
            <Button asChild>
              <a href="/connections">Go to Connections</a>
            </Button>
          }
        />
      ) : (
        <>
          <ConnectionPicker
            connections={connections}
            selectedId={activeId}
            onSelect={setSelectedId}
          />

          <Tabs defaultValue="files">
            <TabsList>
              <TabsTrigger value="files">
                <FolderOpen className="h-4 w-4" /> Files
              </TabsTrigger>
              <TabsTrigger value="email">
                <Inbox className="h-4 w-4" /> Email
              </TabsTrigger>
            </TabsList>

            <TabsContent value="files">
              {selected && <FilesTab connection={selected} />}
            </TabsContent>
            <TabsContent value="email">
              {selected && <EmailTab connection={selected} />}
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  );
}

function FilesTab({ connection }: { connection: Connection }) {
  const qc = useQueryClient();
  const { toast } = useToast();
  const provider = parseProvider(connection.provider);
  const fileInput = useRef<HTMLInputElement>(null);
  const [downloading, setDownloading] = useState<string | null>(null);

  const { data: files, isLoading } = useQuery({
    queryKey: ["files", connection.id, provider],
    queryFn: () => api.files(connection.id, provider),
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) => api.uploadFile(connection.id, provider, file),
    onSuccess: () => {
      toast({ title: "File uploaded", variant: "success" });
      qc.invalidateQueries({ queryKey: ["files", connection.id, provider] });
    },
    onError: (err) =>
      toast({ title: "Upload failed", description: (err as ApiError).message, variant: "error" }),
  });

  async function doDownload(file: RemoteFile, targetMime?: string) {
    setDownloading(file.externalId);
    try {
      await api.downloadFile(
        connection.id,
        { externalId: file.externalId, provider, targetMime },
        file.name
      );
    } catch (err) {
      toast({ title: "Download failed", description: (err as ApiError).message, variant: "error" });
    } finally {
      setDownloading(null);
    }
  }

  return (
    <div className="mt-2 space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-xs text-muted-foreground">
          {PROVIDER_LABEL[provider]} · {connection.accountEmail}
        </p>
        <Button
          size="sm"
          variant="secondary"
          onClick={() => fileInput.current?.click()}
          disabled={uploadMutation.isPending}
        >
          {uploadMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Upload className="h-4 w-4" />
          )}
          Upload
        </Button>
        <input
          ref={fileInput}
          type="file"
          className="hidden"
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) uploadMutation.mutate(f);
            e.target.value = "";
          }}
        />
      </div>

      {isLoading ? (
        <div className="space-y-2">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-12 w-full" />
          ))}
        </div>
      ) : !files || files.length === 0 ? (
        <EmptyState icon={FolderOpen} title="No files" description="This connection has no files to show." />
      ) : (
        <div className="overflow-hidden rounded-md border border-border">
          {files.map((f) => (
            <div
              key={f.externalId}
              className="flex items-center gap-3 border-b border-border px-4 py-2.5 last:border-0 hover:bg-muted/30"
            >
              <FileIcon className="h-4 w-4 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm text-foreground">{f.name}</p>
                <p className="text-xs text-muted-foreground">
                  {formatBytes(f.size)} · {relativeTime(f.modifiedAt)}
                </p>
              </div>
              {f.isGoogleNative ? (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button size="sm" variant="ghost" disabled={downloading === f.externalId}>
                      {downloading === f.externalId ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Download className="h-4 w-4" />
                      )}
                      Export as
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    {GOOGLE_EXPORTS.map((g) => (
                      <DropdownMenuItem key={g.mime} onClick={() => doDownload(f, g.mime)}>
                        {g.label}
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : (
                <Button
                  size="icon-sm"
                  variant="ghost"
                  title="Download"
                  onClick={() => doDownload(f)}
                  disabled={downloading === f.externalId}
                >
                  {downloading === f.externalId ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Download className="h-4 w-4" />
                  )}
                </Button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function EmailTab({ connection }: { connection: Connection }) {
  const provider = parseProvider(connection.provider);
  const { data: messages, isLoading } = useQuery({
    queryKey: ["email", connection.id, provider],
    queryFn: () => api.email(connection.id, provider, 50),
  });

  return (
    <div className="mt-2 space-y-3">
      <p className="text-xs text-muted-foreground">
        {PROVIDER_LABEL[provider]} · {connection.accountEmail} · latest 50
      </p>

      {isLoading ? (
        <div className="space-y-2">
          {Array.from({ length: 6 }).map((_, i) => (
            <Skeleton key={i} className="h-16 w-full" />
          ))}
        </div>
      ) : !messages || messages.length === 0 ? (
        <EmptyState icon={Inbox} title="No email" description="This connection has no messages to show." />
      ) : (
        <div className="overflow-hidden rounded-md border border-border">
          {messages.map((m) => (
            <a
              key={m.id}
              href={m.webUrl ?? undefined}
              target="_blank"
              rel="noopener noreferrer"
              className={cn(
                "group flex items-start gap-3 border-b border-border px-4 py-3 last:border-0 hover:bg-muted/30",
                !m.webUrl && "pointer-events-none"
              )}
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm font-medium text-foreground">{m.from}</span>
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {relativeTime(m.receivedAt)}
                  </span>
                </div>
                <p className="truncate text-sm text-foreground">{m.subject}</p>
                {m.snippet && (
                  <p className="truncate text-xs text-muted-foreground">{m.snippet}</p>
                )}
              </div>
              {m.webUrl && (
                <ExternalLink className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
              )}
            </a>
          ))}
        </div>
      )}
    </div>
  );
}
