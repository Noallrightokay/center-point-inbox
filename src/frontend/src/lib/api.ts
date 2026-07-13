import axios, { AxiosError, type AxiosRequestConfig } from "axios";

import { clearToken, getToken } from "./token";
import type {
  AdminMetrics,
  AiStatus,
  AuditPage,
  AuditVerifyResult,
  AuthResponse,
  AuthUser,
  ChatMessage,
  ComplianceDashboard,
  ComplianceReport,
  Connection,
  ConnectionStart,
  CentraDocument,
  CreateTranslationJob,
  DlpProfile,
  DlpScanResult,
  EmailMessage,
  ExtractedEntity,
  FormatPreference,
  RemoteFile,
  ScheduledExport,
  SearchHistoryEntry,
  SearchRequest,
  SearchResponse,
  SetupStatus,
  SyncStatusEntry,
  TranslationJob,
  TranslationMatrix,
} from "@/types";

export const API_ORIGIN =
  process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";
export const API_BASE = `${API_ORIGIN.replace(/\/$/, "")}/api`;

/** Normalised error surfaced to the UI. Carries the backend traceId when present. */
export class ApiError extends Error {
  status: number;
  traceId?: string;
  constructor(message: string, status: number, traceId?: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.traceId = traceId;
  }
}

const client = axios.create({ baseURL: API_BASE, timeout: 30_000 });

client.interceptors.request.use((config) => {
  const token = getToken();
  if (token) {
    config.headers = config.headers ?? {};
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

// Endpoints that surface their own 401 inline rather than bouncing to login.
const SILENT_AUTH = [/\/auth\/login$/, /\/auth\/register$/, /\/setup\/status$/];

client.interceptors.response.use(
  (res) => res,
  (error: AxiosError<{ error?: string; status?: number; traceId?: string }>) => {
    const status = error.response?.status ?? 0;
    const url = error.config?.url ?? "";
    const body = error.response?.data;
    const message =
      (body && typeof body === "object" && body.error) ||
      error.message ||
      "Request failed";
    const traceId = body && typeof body === "object" ? body.traceId : undefined;

    if (status === 401 && !SILENT_AUTH.some((re) => re.test(url))) {
      clearToken();
      if (typeof window !== "undefined") {
        const path = window.location.pathname;
        if (path !== "/login" && path !== "/setup") {
          window.location.assign("/login");
        }
      }
    }
    return Promise.reject(new ApiError(message, status, traceId));
  }
);

async function get<T>(url: string, config?: AxiosRequestConfig): Promise<T> {
  return (await client.get<T>(url, config)).data;
}
async function post<T>(url: string, data?: unknown, config?: AxiosRequestConfig): Promise<T> {
  return (await client.post<T>(url, data, config)).data;
}
async function put<T>(url: string, data?: unknown, config?: AxiosRequestConfig): Promise<T> {
  return (await client.put<T>(url, data, config)).data;
}
async function del<T>(url: string, config?: AxiosRequestConfig): Promise<T> {
  return (await client.delete<T>(url, config)).data;
}

/** Trigger a browser download for a blob response from `url`. */
async function download(
  url: string,
  params: Record<string, string | undefined>,
  fallbackName: string
): Promise<void> {
  const res = await client.get(url, { params, responseType: "blob" });
  const disposition = res.headers["content-disposition"] as string | undefined;
  const match = disposition?.match(/filename\*?=(?:UTF-8'')?"?([^";]+)"?/i);
  const name = match ? decodeURIComponent(match[1]) : fallbackName;
  saveBlob(res.data as Blob, name);
}

export function saveBlob(blob: Blob, filename: string): void {
  const href = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = href;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(href), 1000);
}

// ==========================================================================
// SSE chat streaming (fetch-based; axios can't stream in the browser).
// ==========================================================================
export interface StreamHandlers {
  onToken: (text: string) => void;
  onDone?: () => void;
  onError?: (err: ApiError) => void;
  signal?: AbortSignal;
}

export async function streamChat(
  body: { messages: ChatMessage[]; contextDocumentId?: string },
  handlers: StreamHandlers
): Promise<void> {
  const token = getToken();
  let res: Response;
  try {
    res = await fetch(`${API_BASE}/ai/chat`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Accept: "text/event-stream",
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: JSON.stringify(body),
      signal: handlers.signal,
    });
  } catch (e) {
    if ((e as Error).name === "AbortError") return;
    handlers.onError?.(new ApiError("Network error", 0));
    return;
  }

  if (!res.ok) {
    if (res.status === 401 && typeof window !== "undefined") {
      clearToken();
      window.location.assign("/login");
    }
    let msg = `Request failed (${res.status})`;
    let traceId: string | undefined;
    try {
      const j = await res.json();
      msg = j.error ?? msg;
      traceId = j.traceId;
    } catch {
      /* non-JSON body */
    }
    handlers.onError?.(new ApiError(msg, res.status, traceId));
    return;
  }

  const reader = res.body?.getReader();
  if (!reader) {
    handlers.onDone?.();
    return;
  }
  const decoder = new TextDecoder();
  let buffer = "";

  const handleData = (payload: string): boolean => {
    const data = payload.trim();
    if (data === "[DONE]") return true;
    if (!data) return false;
    try {
      const parsed = JSON.parse(data);
      if (typeof parsed.text === "string") handlers.onToken(parsed.text);
    } catch {
      // Tolerate raw text chunks that aren't JSON.
      handlers.onToken(data);
    }
    return false;
  };

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const frames = buffer.split(/\n\n/);
      buffer = frames.pop() ?? "";
      for (const frame of frames) {
        for (const line of frame.split(/\n/)) {
          if (line.startsWith("data:")) {
            if (handleData(line.slice(5))) {
              handlers.onDone?.();
              return;
            }
          }
        }
      }
    }
  } catch (e) {
    if ((e as Error).name !== "AbortError") {
      handlers.onError?.(new ApiError("Stream interrupted", 0));
      return;
    }
  }
  handlers.onDone?.();
}

// ==========================================================================
// API surface
// ==========================================================================
export const api = {
  // --- Auth ---
  register: (data: { email: string; displayName: string; password: string }) =>
    post<AuthResponse>("/auth/register", data),
  login: (data: { email: string; password: string }) =>
    post<AuthResponse>("/auth/login", data),
  me: () => get<AuthUser>("/auth/me"),

  // --- Setup ---
  setupStatus: () => get<SetupStatus>("/setup/status"),

  // --- Search ---
  search: (req: SearchRequest, ai = false) =>
    post<SearchResponse>("/search", req, { params: ai ? { ai: true } : undefined }),
  suggest: (prefix: string) =>
    get<string[]>("/search/suggest", { params: { prefix } }),
  searchHistory: () => get<SearchHistoryEntry[]>("/search/history"),

  // --- Connections ---
  connections: () => get<Connection[]>("/connections"),
  startConnection: (provider: string) =>
    post<ConnectionStart>(`/connections/${provider}/start`),
  completeConnection: (provider: string, data: { code: string; state: string }) =>
    post<Connection>(`/connections/${provider}/callback`, data),
  disconnect: (id: string) => del<void>(`/connections/${id}`),

  // --- Sync ---
  sync: (connectionId: string) => post<SyncStatusEntry>(`/sync/${connectionId}`),
  syncStatus: () => get<SyncStatusEntry[]>("/sync/status"),

  // --- Documents ---
  documents: () => get<CentraDocument[]>("/documents"),
  createDocument: (data: { title: string; format: string; content: string }) =>
    post<CentraDocument>("/documents", data),
  updateDocument: (id: string, data: { title?: string; content?: string }) =>
    put<CentraDocument>(`/documents/${id}`, data),
  deleteDocument: (id: string) => del<void>(`/documents/${id}`),

  // --- Files ---
  files: (connectionId: string, provider: string) =>
    get<RemoteFile[]>(`/files/${connectionId}`, { params: { provider } }),
  downloadFile: (
    connectionId: string,
    params: { externalId: string; provider: string; targetMime?: string },
    fallbackName: string
  ) => download(`/files/${connectionId}/download`, params, fallbackName),
  uploadFile: (connectionId: string, provider: string, file: File) => {
    const form = new FormData();
    form.append("file", file);
    return post<RemoteFile>(`/files/${connectionId}/upload`, form, {
      params: { provider },
      headers: { "Content-Type": "multipart/form-data" },
    });
  },

  // --- Email ---
  email: (connectionId: string, provider: string, max = 50) =>
    get<EmailMessage[]>(`/email/${connectionId}`, { params: { provider, max } }),

  // --- Translation ---
  translationMatrix: () => get<TranslationMatrix>("/translation/matrix"),
  convert: async (file: File, target: string): Promise<void> => {
    const form = new FormData();
    form.append("file", file);
    const res = await client.post(`/translation/convert`, form, {
      params: { target },
      headers: { "Content-Type": "multipart/form-data" },
      responseType: "blob",
    });
    const disposition = res.headers["content-disposition"] as string | undefined;
    const match = disposition?.match(/filename\*?=(?:UTF-8'')?"?([^";]+)"?/i);
    const base = file.name.replace(/\.[^.]+$/, "");
    saveBlob(res.data as Blob, match ? decodeURIComponent(match[1]) : `${base}.${target}`);
  },
  translationJobs: () => get<TranslationJob[]>("/translation/jobs"),
  createTranslationJob: (data: CreateTranslationJob) =>
    post<TranslationJob>("/translation/jobs", data),

  // --- Format preferences ---
  formatPreferences: () => get<FormatPreference[]>("/preferences/formats"),
  createFormatPreference: (data: {
    sourceFormat: string;
    provider?: string | null;
    targetFormat: string;
    autoApply: boolean;
  }) => post<FormatPreference>("/preferences/formats", data),
  deleteFormatPreference: (id: string) => del<void>(`/preferences/formats/${id}`),

  // --- Scheduled exports ---
  exports: () => get<ScheduledExport[]>("/exports"),
  createExport: (data: { documentId: string; targetFormat: string; cadence: string }) =>
    post<ScheduledExport>("/exports", data),
  deleteExport: (id: string) => del<void>(`/exports/${id}`),

  // --- AI ---
  aiStatus: () => get<AiStatus>("/ai/status"),
  entities: (text: string) => post<ExtractedEntity[]>("/ai/entities", { text }),

  // --- Compliance / DLP ---
  complianceDashboard: () => get<ComplianceDashboard>("/compliance/dashboard"),
  complianceReport: (params: { from?: string; to?: string; profile?: string }) =>
    get<ComplianceReport>("/compliance/report", { params }),
  dlpProfiles: () => get<DlpProfile[]>("/dlp/profiles"),
  dlpScan: (text: string, profile?: string) =>
    post<DlpScanResult>("/dlp/scan", { text }, { params: profile ? { profile } : undefined }),

  // --- Audit ---
  audit: (params: { page?: number; pageSize?: number; category?: string; userId?: string }) =>
    get<AuditPage>("/audit", { params }),
  auditVerify: () => get<AuditVerifyResult>("/audit/verify"),

  // --- Admin ---
  adminMetrics: () => get<AdminMetrics>("/admin/metrics"),
};

export type Api = typeof api;
