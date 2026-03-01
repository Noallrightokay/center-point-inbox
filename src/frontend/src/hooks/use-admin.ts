import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { get, post, patch } from "@/lib/api";
import type {
  AdminDashboard,
  AuditLogList,
  DlpViolation,
  PaginatedResponse,
  PaginationParams,
} from "@/types";

// ---------------------------------------------------------------------------
// Query keys
// ---------------------------------------------------------------------------

export const adminKeys = {
  all: ["admin"] as const,
  dashboard: () => [...adminKeys.all, "dashboard"] as const,
  auditLogs: (params?: PaginationParams) =>
    [...adminKeys.all, "audit-logs", params] as const,
  violations: () => [...adminKeys.all, "dlp-violations"] as const,
};

// ---------------------------------------------------------------------------
// useAdminDashboard – top-level admin statistics
// ---------------------------------------------------------------------------

export function useAdminDashboard() {
  return useQuery<AdminDashboard>({
    queryKey: adminKeys.dashboard(),
    queryFn: () => get<AdminDashboard>("/admin/dashboard"),
    staleTime: 30_000,
  });
}

// ---------------------------------------------------------------------------
// useAuditLogs – paginated audit log list
// ---------------------------------------------------------------------------

export function useAuditLogs(params?: PaginationParams) {
  return useQuery<AuditLogList>({
    queryKey: adminKeys.auditLogs(params),
    queryFn: () => {
      const searchParams = new URLSearchParams();
      if (params?.page) searchParams.set("page", String(params.page));
      if (params?.pageSize)
        searchParams.set("pageSize", String(params.pageSize));
      if (params?.sortBy) searchParams.set("sortBy", params.sortBy);
      if (params?.sortOrder) searchParams.set("sortOrder", params.sortOrder);

      return get<AuditLogList>(`/admin/audit-logs?${searchParams.toString()}`);
    },
  });
}

// ---------------------------------------------------------------------------
// useDlpViolations – all open DLP violations
// ---------------------------------------------------------------------------

export function useDlpViolations() {
  return useQuery<PaginatedResponse<DlpViolation>>({
    queryKey: adminKeys.violations(),
    queryFn: () =>
      get<PaginatedResponse<DlpViolation>>("/admin/dlp-violations"),
  });
}

// ---------------------------------------------------------------------------
// useResolveViolation – resolve (or mark false positive) a DLP violation
// ---------------------------------------------------------------------------

export function useResolveViolation() {
  const queryClient = useQueryClient();

  return useMutation<
    DlpViolation,
    Error,
    { violationId: string; status: "resolved" | "false_positive" }
  >({
    mutationFn: ({ violationId, status }) =>
      patch<DlpViolation>(`/admin/dlp-violations/${violationId}`, { status }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.violations() });
      queryClient.invalidateQueries({ queryKey: adminKeys.dashboard() });
    },
  });
}

// ---------------------------------------------------------------------------
// useUnquarantineFile – remove quarantine from a file
// ---------------------------------------------------------------------------

export function useUnquarantineFile() {
  const queryClient = useQueryClient();

  return useMutation<void, Error, { fileId: string }>({
    mutationFn: ({ fileId }) =>
      post<void>(`/admin/files/${fileId}/unquarantine`, {}),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.violations() });
      queryClient.invalidateQueries({ queryKey: adminKeys.dashboard() });
    },
  });
}
