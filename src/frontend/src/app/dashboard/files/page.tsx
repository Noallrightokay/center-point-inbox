"use client";

import { useState, useMemo } from "react";
import {
  FileText, FolderOpen, Grid3X3, List, Upload, Download,
  MoreHorizontal, Eye,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { useFiles } from "@/hooks/use-files";
import type { FileFeedQuery, UnifiedFile, FileType } from "@/types";

const FILE_TYPE_ICONS: Record<FileType, { icon: React.ElementType; color: string }> = {
  email: { icon: FileText, color: "text-red-400 bg-red-500/10" },
  document: { icon: FileText, color: "text-blue-400 bg-blue-500/10" },
  spreadsheet: { icon: Grid3X3, color: "text-green-400 bg-green-500/10" },
  presentation: { icon: FileText, color: "text-orange-400 bg-orange-500/10" },
  pdf: { icon: FileText, color: "text-red-400 bg-red-500/10" },
  image: { icon: FileText, color: "text-purple-400 bg-purple-500/10" },
  video: { icon: FileText, color: "text-pink-400 bg-pink-500/10" },
  audio: { icon: FileText, color: "text-yellow-400 bg-yellow-500/10" },
  archive: { icon: FileText, color: "text-gray-400 bg-gray-500/10" },
  other: { icon: FileText, color: "text-secondary-400 bg-white/5" },
};

function formatSize(bytes: number | null): string {
  if (!bytes) return "--";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0, s = bytes;
  while (s >= 1024 && i < units.length - 1) { s /= 1024; i++; }
  return `${s.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export default function FilesPage() {
  const [viewMode, setViewMode] = useState<"grid" | "list">("list");
  const query: FileFeedQuery = useMemo(() => ({
    sortBy: "date", sortOrder: "desc", limit: 50,
  }), []);
  const { data, isLoading } = useFiles(query);

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-white">Files</h1>
          <p className="text-xs text-secondary-500 mt-1">All documents across your connected accounts</p>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex rounded-lg border border-white/10 overflow-hidden">
            <button onClick={() => setViewMode("list")}
              className={`p-2 transition-colors ${viewMode === "list" ? "bg-primary-500/20 text-primary-300" : "text-secondary-500 hover:text-secondary-300"}`}>
              <List className="h-4 w-4" />
            </button>
            <button onClick={() => setViewMode("grid")}
              className={`p-2 transition-colors ${viewMode === "grid" ? "bg-primary-500/20 text-primary-300" : "text-secondary-500 hover:text-secondary-300"}`}>
              <FolderOpen className="h-4 w-4" />
            </button>
          </div>
          <Button className="bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold gap-2" size="sm">
            <Upload className="h-4 w-4" /> Upload
          </Button>
        </div>
      </div>

      {isLoading && (
        <div className="space-y-2">
          {Array.from({ length: 8 }).map((_, i) => (
            <div key={i} className="flex items-center gap-3 rounded-xl border border-white/5 bg-white/[0.02] p-3">
              <Skeleton className="h-8 w-8 rounded-lg bg-white/5" />
              <Skeleton className="h-4 flex-1 bg-white/5" />
              <Skeleton className="h-4 w-16 bg-white/5" />
            </div>
          ))}
        </div>
      )}

      {!isLoading && data && viewMode === "list" && (
        <div className="rounded-xl border border-white/5 overflow-hidden">
          <div className="grid grid-cols-[1fr_120px_100px_80px_40px] gap-4 px-4 py-2 text-[10px] font-semibold uppercase tracking-wider text-secondary-500 border-b border-white/5 bg-white/[0.02]">
            <span>Name</span>
            <span>Source</span>
            <span>Size</span>
            <span>Modified</span>
            <span></span>
          </div>
          {data.items.map((file) => {
            const typeConfig = FILE_TYPE_ICONS[file.fileType] || FILE_TYPE_ICONS.other;
            const Icon = typeConfig.icon;
            return (
              <div key={file.id} className="grid grid-cols-[1fr_120px_100px_80px_40px] gap-4 items-center px-4 py-3 border-b border-white/[0.03] hover:bg-white/[0.02] transition-colors cursor-pointer">
                <div className="flex items-center gap-3 min-w-0">
                  <div className={`flex h-8 w-8 items-center justify-center rounded-lg ${typeConfig.color}`}>
                    <Icon className="h-4 w-4" />
                  </div>
                  <div className="min-w-0">
                    <p className="text-sm font-medium text-secondary-200 truncate">{file.title}</p>
                    {file.from && <p className="text-[11px] text-secondary-500 truncate">{file.from}</p>}
                  </div>
                </div>
                <Badge variant="outline" className="text-[10px] border-white/10 text-secondary-400 w-fit">
                  {file.source.replace("_", " ")}
                </Badge>
                <span className="text-xs text-secondary-500">{formatSize(file.size)}</span>
                <span className="text-[11px] text-secondary-500">
                  {new Date(file.updatedAt).toLocaleDateString("en-US", { month: "short", day: "numeric" })}
                </span>
                <button className="text-secondary-500 hover:text-secondary-300">
                  <MoreHorizontal className="h-4 w-4" />
                </button>
              </div>
            );
          })}
        </div>
      )}

      {!isLoading && data && viewMode === "grid" && (
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 gap-3">
          {data.items.map((file) => {
            const typeConfig = FILE_TYPE_ICONS[file.fileType] || FILE_TYPE_ICONS.other;
            const Icon = typeConfig.icon;
            return (
              <div key={file.id} className="group rounded-xl border border-white/5 bg-white/[0.02] p-4 hover:border-primary-500/20 hover:bg-white/[0.04] transition-all cursor-pointer">
                <div className={`flex h-12 w-12 items-center justify-center rounded-xl mb-3 ${typeConfig.color}`}>
                  <Icon className="h-6 w-6" />
                </div>
                <p className="text-sm font-medium text-secondary-200 truncate">{file.title}</p>
                <p className="text-[11px] text-secondary-500 mt-1">{formatSize(file.size)}</p>
                <div className="flex items-center gap-2 mt-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button className="p-1 rounded bg-white/5 text-secondary-400 hover:text-white">
                    <Eye className="h-3 w-3" />
                  </button>
                  <button className="p-1 rounded bg-white/5 text-secondary-400 hover:text-white">
                    <Download className="h-3 w-3" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
