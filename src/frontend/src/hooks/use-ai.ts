import { useMutation } from "@tanstack/react-query";
import { post } from "@/lib/api";
import type {
  AiQueryRequest,
  AiQueryResponse,
  SummarizeRequest,
  SummarizeResponse,
  SearchRequest,
  SearchResponse,
} from "@/types";

// ---------------------------------------------------------------------------
// useAiAsk – sends a natural-language question to the AI assistant
// ---------------------------------------------------------------------------

export function useAiAsk() {
  return useMutation<AiQueryResponse, Error, AiQueryRequest>({
    mutationFn: (request) => post<AiQueryResponse>("/ai/ask", request),
  });
}

// ---------------------------------------------------------------------------
// useAiSummarize – generates a summary for a specific file
// ---------------------------------------------------------------------------

export function useAiSummarize() {
  return useMutation<SummarizeResponse, Error, SummarizeRequest>({
    mutationFn: (request) =>
      post<SummarizeResponse>("/ai/summarize", request),
  });
}

// ---------------------------------------------------------------------------
// useAiSearch – semantic search across user files
// ---------------------------------------------------------------------------

export function useAiSearch() {
  return useMutation<SearchResponse, Error, SearchRequest>({
    mutationFn: (request) =>
      post<SearchResponse>("/ai/search", request),
  });
}
