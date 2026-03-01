import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { get, post } from "@/lib/api";
import type {
  TranslationJob,
  TranslationRequest,
  PaginatedResponse,
} from "@/types";

// ---------------------------------------------------------------------------
// Query keys
// ---------------------------------------------------------------------------

export const translationKeys = {
  all: ["translations"] as const,
  lists: () => [...translationKeys.all, "list"] as const,
  list: (params?: Record<string, unknown>) =>
    [...translationKeys.lists(), params] as const,
  details: () => [...translationKeys.all, "detail"] as const,
  detail: (id: string) => [...translationKeys.details(), id] as const,
};

// ---------------------------------------------------------------------------
// useRequestTranslation – submits a new translation job
// ---------------------------------------------------------------------------

export function useRequestTranslation() {
  const queryClient = useQueryClient();

  return useMutation<TranslationJob, Error, TranslationRequest>({
    mutationFn: (request) =>
      post<TranslationJob>("/translate", request),
    onSuccess: () => {
      queryClient.invalidateQueries({
        queryKey: translationKeys.lists(),
      });
    },
  });
}

// ---------------------------------------------------------------------------
// useTranslationStatus – polls a translation job until terminal state
// ---------------------------------------------------------------------------

export function useTranslationStatus(jobId: string | null) {
  return useQuery<TranslationJob>({
    queryKey: translationKeys.detail(jobId ?? ""),
    queryFn: () => get<TranslationJob>(`/translate/${jobId}`),
    enabled: !!jobId,
    refetchInterval: (query) => {
      const data = query.state.data;
      if (!data) return 2000;
      if (data.status === "completed" || data.status === "failed") {
        return false;
      }
      return 2000;
    },
  });
}

// ---------------------------------------------------------------------------
// useUserTranslations – lists translation jobs for the current user
// ---------------------------------------------------------------------------

export function useUserTranslations(params?: {
  page?: number;
  pageSize?: number;
}) {
  return useQuery<PaginatedResponse<TranslationJob>>({
    queryKey: translationKeys.list(params),
    queryFn: () => {
      const searchParams = new URLSearchParams();
      if (params?.page) searchParams.set("page", String(params.page));
      if (params?.pageSize)
        searchParams.set("pageSize", String(params.pageSize));

      return get<PaginatedResponse<TranslationJob>>(
        `/translate?${searchParams.toString()}`,
      );
    },
  });
}
