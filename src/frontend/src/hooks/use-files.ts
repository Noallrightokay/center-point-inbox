import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { get, post } from "@/lib/api";
import type { UnifiedFileFeed, UnifiedFile, FileFeedQuery } from "@/types";

// ---------------------------------------------------------------------------
// Query keys
// ---------------------------------------------------------------------------

export const fileKeys = {
  all: ["files"] as const,
  lists: () => [...fileKeys.all, "list"] as const,
  list: (query: FileFeedQuery) => [...fileKeys.lists(), query] as const,
  details: () => [...fileKeys.all, "detail"] as const,
  detail: (id: string) => [...fileKeys.details(), id] as const,
};

// ---------------------------------------------------------------------------
// useFiles – fetches the unified file feed with filters and pagination
// ---------------------------------------------------------------------------

export function useFiles(query: FileFeedQuery) {
  return useQuery<UnifiedFileFeed>({
    queryKey: fileKeys.list(query),
    queryFn: async () => {
      const params = new URLSearchParams();

      if (query.source) params.set("source", query.source);
      if (query.fileType) params.set("fileType", query.fileType);
      if (query.search) params.set("search", query.search);
      if (query.label) params.set("label", query.label);
      if (query.isRead !== undefined) params.set("isRead", String(query.isRead));
      if (query.isStarred !== undefined) params.set("isStarred", String(query.isStarred));
      if (query.language) params.set("language", query.language);
      if (query.from) params.set("from", query.from);
      if (query.sortBy) params.set("sortBy", query.sortBy);
      if (query.sortOrder) params.set("sortOrder", query.sortOrder);
      if (query.cursor) params.set("cursor", query.cursor);
      if (query.limit) params.set("limit", String(query.limit));

      return get<UnifiedFileFeed>(`/files?${params.toString()}`);
    },
    placeholderData: (previousData) => previousData,
  });
}

// ---------------------------------------------------------------------------
// useFile – fetches a single file by ID
// ---------------------------------------------------------------------------

export function useFile(id: string) {
  return useQuery<UnifiedFile>({
    queryKey: fileKeys.detail(id),
    queryFn: () => get<UnifiedFile>(`/files/${id}`),
    enabled: !!id,
  });
}

// ---------------------------------------------------------------------------
// useDownloadFile – mutation that triggers a file download
// ---------------------------------------------------------------------------

export function useDownloadFile() {
  const queryClient = useQueryClient();

  return useMutation<Blob, Error, { fileId: string; filename: string }>({
    mutationFn: async ({ fileId }) => {
      const response = await post<Blob>(
        `/files/${fileId}/download`,
        undefined,
        { responseType: "blob" },
      );
      return response;
    },
    onSuccess: (_data, variables) => {
      // Trigger browser download via temporary anchor element.
      const url = window.URL.createObjectURL(_data);
      const link = document.createElement("a");
      link.href = url;
      link.download = variables.filename;
      document.body.appendChild(link);
      link.click();
      document.body.removeChild(link);
      window.URL.revokeObjectURL(url);

      // Invalidate file detail to refresh access timestamps.
      queryClient.invalidateQueries({
        queryKey: fileKeys.detail(variables.fileId),
      });
    },
  });
}
