// ==========================================================================
// Centra API DTOs.
//
// Enum-typed fields arrive as integers OR strings on the wire, so they are
// typed as `Wire` and normalised with the parsers in lib/enums.ts at the
// point of use.
// ==========================================================================

/** An enum value as it arrives from the backend: integer index or string name. */
export type Wire = number | string;

// --- Auth ------------------------------------------------------------------
export interface AuthUser {
  id: string;
  email: string;
  displayName: string;
  role: Wire; // Admin | ComplianceOfficer | Member
  createdAt?: string;
}

export interface AuthResponse {
  token: string;
  expiresAt: string;
  user: AuthUser;
}

// --- Setup -----------------------------------------------------------------
export interface SetupCheck {
  name: string;
  ok: boolean;
  detail?: string | null;
  envHint?: string | null;
}

export interface SetupStatus {
  ready: boolean;
  adminExists: boolean;
  database: boolean;
  redis: boolean;
  ai: boolean;
  gotenberg: boolean;
  providers?: Record<string, boolean>;
  checks?: SetupCheck[];
}

// --- Search ----------------------------------------------------------------
export interface SearchRequest {
  query: string;
  provider?: Wire;
  itemType?: Wire;
  from?: string;
  to?: string;
  page?: number;
  pageSize?: number;
}

export interface SearchResultItem {
  id: string;
  provider: Wire;
  itemType: Wire;
  title: string;
  snippet?: string | null;
  webUrl?: string | null;
  modifiedAt?: string | null;
  connectionId?: string | null;
  externalId?: string | null;
}

export interface SearchResponse {
  results: SearchResultItem[];
  total: number;
  page: number;
  pageSize: number;
  aiSummary?: string | null;
  tookMs?: number;
}

export interface SearchHistoryEntry {
  query: string;
  at?: string;
}

// --- Connections -----------------------------------------------------------
export interface Connection {
  id: string;
  provider: Wire;
  accountEmail: string;
  status: Wire; // Active | Expired | Error | Revoked
  lastSyncAt?: string | null;
  lastError?: string | null;
  displayName?: string | null;
}

export interface ConnectionStart {
  authorizationUrl: string;
  state: string;
}

export interface SyncStatusEntry {
  connectionId: string;
  status: Wire; // Idle | Running | Completed | Failed
  itemsIndexed?: number;
  startedAt?: string | null;
  finishedAt?: string | null;
  error?: string | null;
}

// --- Documents -------------------------------------------------------------
export interface CentraDocument {
  id: string;
  title: string;
  format: Wire;
  content?: string;
  version: number;
  createdAt?: string;
  updatedAt?: string;
}

// --- Files -----------------------------------------------------------------
export interface RemoteFile {
  externalId: string;
  name: string;
  size?: number | null;
  modifiedAt?: string | null;
  mimeType?: string | null;
  isGoogleNative?: boolean;
  webUrl?: string | null;
}

// --- Email -----------------------------------------------------------------
export interface EmailMessage {
  id: string;
  from: string;
  subject: string;
  snippet?: string | null;
  receivedAt?: string | null;
  webUrl?: string | null;
}

// --- Translation -----------------------------------------------------------
export interface TranslationMatrix {
  sources: string[];
  targets: string[];
  /** matrix[sourceFormat][targetFormat] = supported */
  supported: Record<string, Record<string, boolean>>;
  gotenbergAvailable: boolean;
}

export interface TranslationJob {
  id: string;
  status: Wire; // Queued | Processing | Completed | Failed
  targetFormat: Wire;
  sourceLabel?: string | null;
  connectionId?: string | null;
  sourceExternalId?: string | null;
  nativeDocumentId?: string | null;
  outputDocumentId?: string | null;
  durationMs?: number | null;
  error?: string | null;
  createdAt?: string | null;
  completedAt?: string | null;
}

export interface CreateTranslationJob {
  connectionId?: string;
  sourceExternalId?: string;
  nativeDocumentId?: string;
  targetFormat: string;
}

export interface FormatPreference {
  id: string;
  sourceFormat: Wire;
  provider?: Wire | null; // null => "Anywhere"
  targetFormat: Wire;
  autoApply: boolean;
}

// --- Scheduled exports -----------------------------------------------------
export interface ScheduledExport {
  id: string;
  documentId: string;
  targetFormat: Wire;
  cadence: Wire;
  enabled: boolean;
  consecutiveFailures: number;
  lastRunAt?: string | null;
  nextRunAt?: string | null;
}

// --- AI --------------------------------------------------------------------
export interface AiStatus {
  configured: boolean;
  model?: string | null;
  provider?: string | null;
}

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

export interface ExtractedEntity {
  type: Wire; // person | organization | date | location | amount
  value: string;
}

// --- Compliance / DLP ------------------------------------------------------
export interface ComplianceDashboard {
  users: number;
  activeConnections: number;
  indexedItems: number;
  dlpViolations30d: number;
  auditChainIntact: boolean;
  activeProfile: string;
}

export interface DlpProfile {
  id: string;
  name: string;
}

export interface DlpMatch {
  rule: string;
  severity: Wire;
  action: string;
  count: number;
  redactedSample?: string | null;
}

export interface DlpScanResult {
  matches: DlpMatch[];
  redactedContent: string;
  profile: string;
}

export interface ComplianceReport {
  byRule: { rule: string; count: number }[];
  byAction: { action: string; count: number }[];
  chainIntact: boolean;
  from?: string;
  to?: string;
}

// --- Audit -----------------------------------------------------------------
export interface AuditEntry {
  id: string;
  timestamp: string;
  category: string;
  action: string;
  detail?: string | null;
  userId?: string | null;
  userEmail?: string | null;
  hash: string;
}

export interface AuditPage {
  entries: AuditEntry[];
  page: number;
  pageSize: number;
  total: number;
}

export interface AuditVerifyResult {
  intact: boolean;
  checkedCount?: number;
  brokenAt?: string | null;
  message?: string | null;
}

// --- Admin metrics ---------------------------------------------------------
export interface AdminMetrics {
  memoryUsedBytes: number;
  memoryTotalBytes?: number;
  threads: number;
  requestCount: number;
  avgResponseMs: number;
  errorRate: number;
  topEndpoints: { endpoint: string; count: number; avgMs?: number }[];
  entityCounts: Record<string, number>;
}

// --- Errors ----------------------------------------------------------------
export interface ApiErrorBody {
  error: string;
  status: number;
  traceId?: string;
}
