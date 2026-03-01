// ==========================================================================
// Center Point Inbox – TypeScript type definitions
//
// These types mirror the backend DTOs for type-safe communication between
// the Next.js frontend and the Java/Spring Boot API.
// ==========================================================================

// ---------------------------------------------------------------------------
// Auth & User
// ---------------------------------------------------------------------------

export interface User {
  id: string;
  email: string;
  name: string;
  avatarUrl: string | null;
  provider: string;
  roles: string[];
  organizationId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface TokenResponse {
  accessToken: string;
  refreshToken?: string;
  expiresIn: number;
  tokenType: string;
  user?: User;
}

// ---------------------------------------------------------------------------
// Unified File / Inbox
// ---------------------------------------------------------------------------

export type FileSource = "gmail" | "outlook" | "google_drive" | "onedrive";

export type FileType =
  | "email"
  | "document"
  | "spreadsheet"
  | "presentation"
  | "pdf"
  | "image"
  | "video"
  | "audio"
  | "archive"
  | "other";

export interface UnifiedFile {
  id: string;
  externalId: string;
  source: FileSource;
  fileType: FileType;
  title: string;
  snippet: string | null;
  body: string | null;
  from: string | null;
  to: string[] | null;
  cc: string[] | null;
  bcc: string[] | null;
  labels: string[];
  isRead: boolean;
  isStarred: boolean;
  hasAttachments: boolean;
  attachments: FileAttachment[];
  detectedLanguage: string | null;
  translatedTitle: string | null;
  translatedBody: string | null;
  threadId: string | null;
  parentId: string | null;
  mimeType: string | null;
  size: number | null;
  webUrl: string | null;
  createdAt: string;
  updatedAt: string;
  receivedAt: string | null;
}

export interface FileAttachment {
  id: string;
  filename: string;
  mimeType: string;
  size: number;
  downloadUrl: string | null;
}

export interface UnifiedFileFeed {
  items: UnifiedFile[];
  nextCursor: string | null;
  hasMore: boolean;
  totalCount: number;
}

export interface FileFeedQuery {
  source?: FileSource;
  fileType?: FileType;
  search?: string;
  label?: string;
  isRead?: boolean;
  isStarred?: boolean;
  language?: string;
  from?: string;
  sortBy?: "date" | "relevance" | "size";
  sortOrder?: "asc" | "desc";
  cursor?: string;
  limit?: number;
}

// ---------------------------------------------------------------------------
// Translation
// ---------------------------------------------------------------------------

export type TranslationStatus =
  | "pending"
  | "processing"
  | "completed"
  | "failed";

export interface TranslationJob {
  id: string;
  fileId: string;
  sourceLanguage: string;
  targetLanguage: string;
  status: TranslationStatus;
  translatedContent: string | null;
  characterCount: number | null;
  provider: string | null;
  errorMessage: string | null;
  createdAt: string;
  completedAt: string | null;
}

export interface TranslationRequest {
  fileId: string;
  targetLanguage: string;
  sourceLanguage?: string;
  fieldsToTranslate?: string[];
}

// ---------------------------------------------------------------------------
// AI / LLM
// ---------------------------------------------------------------------------

export interface AiQueryRequest {
  query: string;
  fileIds?: string[];
  context?: string;
  model?: string;
  maxTokens?: number;
  temperature?: number;
}

export interface AiQueryResponse {
  id: string;
  query: string;
  response: string;
  model: string;
  tokensUsed: number;
  sourceReferences: AiSourceReference[];
  createdAt: string;
}

export interface AiSourceReference {
  fileId: string;
  fileTitle: string;
  relevanceScore: number;
  excerpt: string;
}

export interface SummarizeRequest {
  fileId: string;
  maxLength?: number;
  language?: string;
  format?: "brief" | "detailed" | "bullets";
}

export interface SummarizeResponse {
  id: string;
  fileId: string;
  summary: string;
  keyPoints: string[];
  language: string;
  model: string;
  tokensUsed: number;
  createdAt: string;
}

export interface SearchRequest {
  query: string;
  sources?: FileSource[];
  fileTypes?: FileType[];
  dateFrom?: string;
  dateTo?: string;
  language?: string;
  limit?: number;
  offset?: number;
}

export interface SearchResponse {
  results: SearchResult[];
  totalCount: number;
  query: string;
  searchTimeMs: number;
}

export interface SearchResult {
  file: UnifiedFile;
  relevanceScore: number;
  highlights: SearchHighlight[];
}

export interface SearchHighlight {
  field: string;
  fragments: string[];
}

// ---------------------------------------------------------------------------
// Audit & Compliance
// ---------------------------------------------------------------------------

export type AuditAction =
  | "login"
  | "logout"
  | "file_access"
  | "file_download"
  | "file_translate"
  | "ai_query"
  | "ai_summarize"
  | "admin_action"
  | "dlp_violation"
  | "export"
  | "share";

export interface AuditLog {
  id: string;
  userId: string;
  userEmail: string;
  action: AuditAction;
  resourceType: string;
  resourceId: string | null;
  details: Record<string, unknown>;
  ipAddress: string;
  userAgent: string;
  timestamp: string;
  organizationId: string | null;
}

export interface AuditLogList {
  items: AuditLog[];
  totalCount: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

export type DlpSeverity = "low" | "medium" | "high" | "critical";

export type DlpStatus = "open" | "acknowledged" | "resolved" | "false_positive";

export interface DlpViolation {
  id: string;
  fileId: string;
  fileTitle: string;
  userId: string;
  userEmail: string;
  violationType: string;
  severity: DlpSeverity;
  status: DlpStatus;
  description: string;
  detectedPatterns: string[];
  remediationAction: string | null;
  detectedAt: string;
  resolvedAt: string | null;
  resolvedBy: string | null;
  organizationId: string | null;
}

// ---------------------------------------------------------------------------
// Admin Dashboard
// ---------------------------------------------------------------------------

export interface AdminDashboard {
  totalUsers: number;
  activeUsers: number;
  totalFiles: number;
  totalTranslations: number;
  totalAiQueries: number;
  storageUsedBytes: number;
  dlpViolationsOpen: number;
  recentActivity: AuditLog[];
  usageBySource: Record<FileSource, number>;
  usageByDay: DailyUsage[];
  topLanguages: LanguageUsage[];
}

export interface DailyUsage {
  date: string;
  fileAccesses: number;
  translations: number;
  aiQueries: number;
}

export interface LanguageUsage {
  language: string;
  translationCount: number;
  percentage: number;
}

// ---------------------------------------------------------------------------
// Common / Shared
// ---------------------------------------------------------------------------

export interface PaginatedResponse<T> {
  items: T[];
  totalCount: number;
  page: number;
  pageSize: number;
  totalPages: number;
}

export interface ApiError {
  status: number;
  message: string;
  errors?: FieldError[];
  timestamp: string;
  path: string;
}

export interface FieldError {
  field: string;
  message: string;
  rejectedValue?: unknown;
}

export type SortOrder = "asc" | "desc";

export interface PaginationParams {
  page?: number;
  pageSize?: number;
  sortBy?: string;
  sortOrder?: SortOrder;
}
