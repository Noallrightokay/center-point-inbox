"use client";

import { useRouter } from "next/navigation";
import {
  Mail, FileText, Sheet, Presentation, FileImage, FileVideo,
  FileAudio, Archive, File, FileType2, Shield, ShieldAlert,
  Users, Star, Paperclip,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import type { UnifiedFile, FileSource, FileType } from "@/types";

const FILE_TYPE_ICONS: Record<FileType, React.ElementType> = {
  email: Mail, document: FileText, spreadsheet: Sheet,
  presentation: Presentation, pdf: FileType2, image: FileImage,
  video: FileVideo, audio: FileAudio, archive: Archive, other: File,
};

const SOURCE_CONFIG: Record<FileSource, { label: string; color: string }> = {
  gmail: { label: "Gmail", color: "bg-red-500/15 text-red-400 border-red-500/20" },
  outlook: { label: "Outlook", color: "bg-blue-500/15 text-blue-400 border-blue-500/20" },
  google_drive: { label: "Google Drive", color: "bg-green-500/15 text-green-400 border-green-500/20" },
  onedrive: { label: "OneDrive", color: "bg-sky-500/15 text-sky-400 border-sky-500/20" },
};

function formatFileSize(bytes: number | null): string {
  if (bytes === null || bytes === 0) return "--";
  const units = ["B", "KB", "MB", "GB"];
  let unitIndex = 0;
  let size = bytes;
  while (size >= 1024 && unitIndex < units.length - 1) { size /= 1024; unitIndex++; }
  return `${size.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function formatRelativeDate(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMinutes = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMinutes / 60);
  const diffDays = Math.floor(diffHours / 24);
  if (diffMinutes < 1) return "just now";
  if (diffMinutes < 60) return `${diffMinutes}m`;
  if (diffHours < 24) return `${diffHours}h`;
  if (diffDays < 7) return `${diffDays}d`;
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

interface FileCardProps {
  file: UnifiedFile;
}

export function FileCard({ file }: FileCardProps) {
  const router = useRouter();
  const Icon = FILE_TYPE_ICONS[file.fileType] ?? File;
  const sourceConfig = SOURCE_CONFIG[file.source];
  const isQuarantined = file.labels?.includes("quarantined") ?? false;
  const isPhiDetected = file.labels?.includes("phi_detected") ?? false;
  const isShared = (file.to && file.to.length > 0) || file.labels?.includes("shared");

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => router.push(`/file/${file.id}`)}
      onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); router.push(`/file/${file.id}`); } }}
      className={`group cursor-pointer rounded-xl border p-4 transition-all duration-200 hover:shadow-amex ${
        isQuarantined
          ? "border-red-500/30 bg-red-500/5"
          : !file.isRead
            ? "border-primary-500/20 bg-primary-500/[0.03] hover:border-primary-500/40"
            : "border-white/5 bg-white/[0.02] hover:border-white/15 hover:bg-white/[0.04]"
      }`}
    >
      <div className="flex items-start gap-3">
        {/* Icon */}
        <div className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
          isQuarantined ? "bg-red-500/15 text-red-400" :
          !file.isRead ? "bg-primary-500/15 text-primary-400" :
          "bg-white/5 text-secondary-400 group-hover:bg-white/10"
        }`}>
          <Icon className="h-5 w-5" />
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                {!file.isRead && <div className="h-2 w-2 rounded-full bg-primary-500 shrink-0" />}
                <h3 className={`text-sm truncate ${!file.isRead ? "font-semibold text-white" : "font-medium text-secondary-200"}`}>
                  {file.title}
                </h3>
                {file.isStarred && <Star className="h-3.5 w-3.5 text-primary-400 fill-primary-400 shrink-0" />}
              </div>
              {file.from && (
                <p className="text-xs text-secondary-500 mt-0.5 truncate">{file.from}</p>
              )}
              {file.snippet && (
                <p className="text-xs text-secondary-500 mt-1 line-clamp-1">{file.snippet}</p>
              )}
            </div>

            <div className="flex-shrink-0 text-right">
              <p className="text-[11px] text-secondary-500 font-medium">{formatRelativeDate(file.updatedAt)}</p>
              {file.size !== null && <p className="text-[10px] text-secondary-600 mt-0.5">{formatFileSize(file.size)}</p>}
            </div>
          </div>

          {/* Badges */}
          <div className="flex flex-wrap items-center gap-1.5 mt-2">
            {sourceConfig && (
              <Badge variant="outline" className={`text-[10px] px-1.5 py-0 ${sourceConfig.color}`}>
                {sourceConfig.label}
              </Badge>
            )}
            {isPhiDetected && (
              <Badge className="text-[10px] px-1.5 py-0 gap-0.5 bg-red-500/15 text-red-400 border-red-500/20">
                <ShieldAlert className="h-3 w-3" /> PHI
              </Badge>
            )}
            {isQuarantined && (
              <Badge className="text-[10px] px-1.5 py-0 gap-0.5 bg-red-500/15 text-red-400 border-red-500/20">
                <Shield className="h-3 w-3" /> Quarantined
              </Badge>
            )}
            {isShared && (
              <Badge variant="outline" className="text-[10px] px-1.5 py-0 gap-0.5 border-white/10 text-secondary-400">
                <Users className="h-3 w-3" /> Shared
              </Badge>
            )}
            {file.hasAttachments && (
              <Badge variant="outline" className="text-[10px] px-1.5 py-0 gap-0.5 border-white/10 text-secondary-400">
                <Paperclip className="h-3 w-3" /> {file.attachments.length}
              </Badge>
            )}
            {file.detectedLanguage && file.detectedLanguage !== "en" && (
              <Badge variant="outline" className="text-[10px] px-1.5 py-0 border-primary-500/20 text-primary-400">
                {file.detectedLanguage.toUpperCase()}
              </Badge>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
