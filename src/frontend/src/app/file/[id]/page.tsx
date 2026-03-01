"use client";

import { useState } from "react";
import { useParams, useRouter } from "next/navigation";
import {
  ArrowLeft,
  Download,
  Languages,
  Sparkles,
  ExternalLink,
  Mail,
  FileText,
  Sheet,
  Presentation,
  FileImage,
  FileVideo,
  FileAudio,
  Archive,
  File,
  FileType2,
  ShieldAlert,
  Shield,
  Users,
  Calendar,
  HardDrive,
  Globe,
  Loader2,
  AlertTriangle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { TranslateModal } from "@/components/translate-modal";
import { useFile, useDownloadFile } from "@/hooks/use-files";
import { useAiSummarize } from "@/hooks/use-ai";
import type { FileType } from "@/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const FILE_TYPE_ICONS: Record<FileType, React.ElementType> = {
  email: Mail,
  document: FileText,
  spreadsheet: Sheet,
  presentation: Presentation,
  pdf: FileType2,
  image: FileImage,
  video: FileVideo,
  audio: FileAudio,
  archive: Archive,
  other: File,
};

function formatFileSize(bytes: number | null): string {
  if (bytes === null || bytes === 0) return "--";
  const units = ["B", "KB", "MB", "GB"];
  let unitIndex = 0;
  let size = bytes;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }
  return `${size.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function formatDate(dateStr: string): string {
  return new Date(dateStr).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

const SOURCE_LABELS: Record<string, string> = {
  gmail: "Gmail",
  outlook: "Outlook",
  google_drive: "Google Drive",
  onedrive: "OneDrive",
};

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------

export default function FileDetailPage() {
  const params = useParams();
  const router = useRouter();
  const fileId = params.id as string;

  const { data: file, isLoading, isError, error } = useFile(fileId);
  const downloadFile = useDownloadFile();
  const aiSummarize = useAiSummarize();

  const [translateOpen, setTranslateOpen] = useState(false);
  const [summary, setSummary] = useState<string | null>(null);

  const isQuarantined = file?.labels?.includes("quarantined") ?? false;
  const isPhiDetected = file?.labels?.includes("phi_detected") ?? false;
  const isShared =
    (file?.to && file.to.length > 0) || file?.labels?.includes("shared");

  const Icon = file ? (FILE_TYPE_ICONS[file.fileType] ?? File) : File;

  function handleDownload() {
    if (!file || isQuarantined) return;
    downloadFile.mutate({
      fileId: file.id,
      filename: file.title,
    });
  }

  function handleSummarize() {
    if (!file) return;
    aiSummarize.mutate(
      { fileId: file.id, format: "detailed" },
      {
        onSuccess: (response) => {
          setSummary(response.summary);
        },
      },
    );
  }

  // Loading state
  if (isLoading) {
    return (
      <div className="p-6 max-w-4xl mx-auto space-y-6">
        <Skeleton className="h-8 w-48" />
        <div className="grid gap-6 lg:grid-cols-3">
          <div className="lg:col-span-2 space-y-4">
            <Skeleton className="h-64 w-full rounded-xl" />
            <Skeleton className="h-[400px] w-full rounded-xl" />
          </div>
          <div className="space-y-4">
            <Skeleton className="h-48 w-full rounded-xl" />
            <Skeleton className="h-32 w-full rounded-xl" />
          </div>
        </div>
      </div>
    );
  }

  // Error state
  if (isError || !file) {
    return (
      <div className="flex flex-col items-center justify-center py-24 px-6 text-center">
        <div className="rounded-full bg-red-100 p-4 mb-4">
          <AlertTriangle className="h-8 w-8 text-red-500" />
        </div>
        <h2 className="text-lg font-medium mb-1">File not found</h2>
        <p className="text-sm text-muted-foreground max-w-sm mb-6">
          {error?.message ??
            "The file you are looking for does not exist or you do not have access."}
        </p>
        <Button variant="outline" onClick={() => router.push("/dashboard")}>
          <ArrowLeft className="h-4 w-4 mr-1" />
          Back to Dashboard
        </Button>
      </div>
    );
  }

  return (
    <div className="p-6 max-w-5xl mx-auto">
      {/* Back link and actions */}
      <div className="flex items-center justify-between mb-6">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => router.push("/dashboard")}
          className="gap-1"
        >
          <ArrowLeft className="h-4 w-4" />
          Back to Dashboard
        </Button>

        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setTranslateOpen(true)}
            className="gap-1.5"
          >
            <Languages className="h-4 w-4" />
            Translate
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={handleSummarize}
            disabled={aiSummarize.isPending}
            className="gap-1.5"
          >
            {aiSummarize.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Sparkles className="h-4 w-4" />
            )}
            Summarize
          </Button>

          <Button
            size="sm"
            onClick={handleDownload}
            disabled={isQuarantined || downloadFile.isPending}
            className="gap-1.5"
          >
            {downloadFile.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Download className="h-4 w-4" />
            )}
            Download
          </Button>
        </div>
      </div>

      {/* Quarantine warning banner */}
      {isQuarantined && (
        <div className="mb-6 flex items-center gap-3 rounded-lg border border-red-300 bg-red-50 px-4 py-3 text-red-800">
          <Shield className="h-5 w-5 flex-shrink-0" />
          <div>
            <p className="text-sm font-medium">
              This file has been quarantined
            </p>
            <p className="text-xs text-red-600 mt-0.5">
              A DLP policy violation was detected. Downloads are disabled.
              Contact your administrator for more information.
            </p>
          </div>
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-3">
        {/* Main content area */}
        <div className="lg:col-span-2 space-y-6">
          {/* File info card */}
          <Card>
            <CardHeader>
              <div className="flex items-start gap-3">
                <div
                  className={`flex h-12 w-12 items-center justify-center rounded-lg ${
                    isQuarantined
                      ? "bg-red-100 text-red-600"
                      : "bg-muted text-muted-foreground"
                  }`}
                >
                  <Icon className="h-6 w-6" />
                </div>
                <div className="flex-1 min-w-0">
                  <CardTitle className="text-lg truncate">
                    {file.title}
                  </CardTitle>
                  <CardDescription className="mt-1">
                    {SOURCE_LABELS[file.source] ?? file.source} &middot;{" "}
                    {file.mimeType ?? file.fileType}
                  </CardDescription>
                </div>
              </div>
            </CardHeader>
            <CardContent>
              {/* Badges */}
              <div className="flex flex-wrap gap-2 mb-4">
                {/* Provider badge */}
                <Badge variant="outline">
                  {SOURCE_LABELS[file.source] ?? file.source}
                </Badge>

                {/* PHI badge */}
                {isPhiDetected && (
                  <Badge variant="destructive" className="gap-1">
                    <ShieldAlert className="h-3 w-3" />
                    PHI Detected
                  </Badge>
                )}

                {/* Quarantine badge */}
                {isQuarantined && (
                  <Badge variant="destructive" className="gap-1">
                    <Shield className="h-3 w-3" />
                    Quarantined
                  </Badge>
                )}

                {/* Shared badge */}
                {isShared && (
                  <Badge variant="secondary" className="gap-1">
                    <Users className="h-3 w-3" />
                    Shared
                  </Badge>
                )}

                {/* Language badge */}
                {file.detectedLanguage && (
                  <Badge variant="outline" className="gap-1">
                    <Globe className="h-3 w-3" />
                    {file.detectedLanguage.toUpperCase()}
                  </Badge>
                )}
              </div>

              {/* Metadata grid */}
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div className="space-y-1">
                  <p className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                    <Calendar className="h-3 w-3" />
                    Modified
                  </p>
                  <p>{formatDate(file.updatedAt)}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                    <Calendar className="h-3 w-3" />
                    Created
                  </p>
                  <p>{formatDate(file.createdAt)}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs font-medium text-muted-foreground flex items-center gap-1">
                    <HardDrive className="h-3 w-3" />
                    Size
                  </p>
                  <p>{formatFileSize(file.size)}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs font-medium text-muted-foreground">
                    Type
                  </p>
                  <p className="capitalize">{file.fileType}</p>
                </div>
              </div>

              {/* Email-specific fields */}
              {file.fileType === "email" && (
                <>
                  <Separator className="my-4" />
                  <div className="space-y-2 text-sm">
                    {file.from && (
                      <div>
                        <span className="font-medium text-muted-foreground">
                          From:{" "}
                        </span>
                        <span>{file.from}</span>
                      </div>
                    )}
                    {file.to && file.to.length > 0 && (
                      <div>
                        <span className="font-medium text-muted-foreground">
                          To:{" "}
                        </span>
                        <span>{file.to.join(", ")}</span>
                      </div>
                    )}
                    {file.cc && file.cc.length > 0 && (
                      <div>
                        <span className="font-medium text-muted-foreground">
                          CC:{" "}
                        </span>
                        <span>{file.cc.join(", ")}</span>
                      </div>
                    )}
                  </div>
                </>
              )}

              {/* Snippet / Body preview */}
              {(file.snippet || file.body) && (
                <>
                  <Separator className="my-4" />
                  <div className="text-sm text-muted-foreground whitespace-pre-wrap">
                    {file.body ?? file.snippet}
                  </div>
                </>
              )}
            </CardContent>
          </Card>

          {/* Sandboxed preview */}
          {file.webUrl && !isQuarantined && (
            <Card>
              <CardHeader className="pb-3">
                <div className="flex items-center justify-between">
                  <CardTitle className="text-sm">Preview</CardTitle>
                  <Button
                    variant="ghost"
                    size="sm"
                    asChild
                    className="gap-1.5 text-xs"
                  >
                    <a
                      href={file.webUrl}
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      <ExternalLink className="h-3 w-3" />
                      Open in provider
                    </a>
                  </Button>
                </div>
              </CardHeader>
              <CardContent className="p-0 pb-0">
                <div className="relative w-full overflow-hidden rounded-b-xl border-t">
                  <iframe
                    src={file.webUrl}
                    title={`Preview: ${file.title}`}
                    sandbox="allow-scripts allow-same-origin"
                    className="h-[500px] w-full"
                    loading="lazy"
                  />
                </div>
              </CardContent>
            </Card>
          )}

          {/* AI Summary */}
          {summary && (
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm flex items-center gap-1.5">
                  <Sparkles className="h-4 w-4 text-primary" />
                  AI Summary
                </CardTitle>
              </CardHeader>
              <CardContent>
                <p className="text-sm whitespace-pre-wrap">{summary}</p>
              </CardContent>
            </Card>
          )}

          {aiSummarize.isError && (
            <Card className="border-red-200 bg-red-50/50">
              <CardContent className="py-4">
                <p className="text-sm text-red-700">
                  Failed to generate summary:{" "}
                  {aiSummarize.error?.message ?? "Unknown error"}
                </p>
              </CardContent>
            </Card>
          )}
        </div>

        {/* Sidebar */}
        <div className="space-y-6">
          {/* Attachments */}
          {file.attachments.length > 0 && (
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm">
                  Attachments ({file.attachments.length})
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-2">
                {file.attachments.map((attachment) => (
                  <div
                    key={attachment.id}
                    className="flex items-center gap-2 rounded-lg border p-2 text-sm"
                  >
                    <File className="h-4 w-4 text-muted-foreground shrink-0" />
                    <div className="flex-1 min-w-0">
                      <p className="truncate text-xs font-medium">
                        {attachment.filename}
                      </p>
                      <p className="text-[10px] text-muted-foreground">
                        {formatFileSize(attachment.size)}
                      </p>
                    </div>
                    {attachment.downloadUrl && !isQuarantined && (
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-7 w-7"
                        asChild
                      >
                        <a
                          href={attachment.downloadUrl}
                          download={attachment.filename}
                        >
                          <Download className="h-3 w-3" />
                        </a>
                      </Button>
                    )}
                  </div>
                ))}
              </CardContent>
            </Card>
          )}

          {/* Labels */}
          {file.labels.length > 0 && (
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm">Labels</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="flex flex-wrap gap-1.5">
                  {file.labels.map((label) => (
                    <Badge key={label} variant="secondary" className="text-xs">
                      {label}
                    </Badge>
                  ))}
                </div>
              </CardContent>
            </Card>
          )}

          {/* Translated content */}
          {(file.translatedTitle || file.translatedBody) && (
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-sm flex items-center gap-1.5">
                  <Languages className="h-4 w-4" />
                  Translation
                </CardTitle>
              </CardHeader>
              <CardContent className="space-y-2 text-sm">
                {file.translatedTitle && (
                  <div>
                    <p className="text-xs font-medium text-muted-foreground mb-0.5">
                      Title
                    </p>
                    <p>{file.translatedTitle}</p>
                  </div>
                )}
                {file.translatedBody && (
                  <div>
                    <p className="text-xs font-medium text-muted-foreground mb-0.5">
                      Body
                    </p>
                    <p className="whitespace-pre-wrap text-xs">
                      {file.translatedBody}
                    </p>
                  </div>
                )}
              </CardContent>
            </Card>
          )}
        </div>
      </div>

      {/* Translate modal */}
      <TranslateModal
        fileId={file.id}
        fileName={file.title}
        sourceLanguage={file.detectedLanguage}
        open={translateOpen}
        onOpenChange={setTranslateOpen}
      />
    </div>
  );
}
