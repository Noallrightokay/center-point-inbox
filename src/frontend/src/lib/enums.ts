// ==========================================================================
// Centra enum helpers.
//
// The backend serialises enums as either integers (their declaration order)
// or as their string name. Every enum here ships an ordered value list plus a
// `parse` helper that normalises number | string | null into the canonical
// string union used throughout the UI.
// ==========================================================================

function makeParser<const T extends readonly string[]>(values: T, fallback: T[number]) {
  const byLower = new Map(values.map((v) => [v.toLowerCase(), v]));
  return (raw: unknown): T[number] => {
    if (typeof raw === "number" && raw >= 0 && raw < values.length) return values[raw];
    if (typeof raw === "string") {
      const trimmed = raw.trim();
      const asNum = Number(trimmed);
      if (trimmed !== "" && Number.isInteger(asNum) && asNum >= 0 && asNum < values.length) {
        return values[asNum];
      }
      const hit = byLower.get(trimmed.toLowerCase());
      if (hit) return hit;
    }
    return fallback;
  };
}

// --- ProviderType ----------------------------------------------------------
export const PROVIDER_TYPES = ["GoogleWorkspace", "Microsoft365", "Dropbox"] as const;
export type ProviderType = (typeof PROVIDER_TYPES)[number];
export const parseProvider = makeParser(PROVIDER_TYPES, "GoogleWorkspace");

export const PROVIDER_LABEL: Record<ProviderType, string> = {
  GoogleWorkspace: "Google Workspace",
  Microsoft365: "Microsoft 365",
  Dropbox: "Dropbox",
};

// --- ItemType --------------------------------------------------------------
export const ITEM_TYPES = [
  "Email",
  "Document",
  "Spreadsheet",
  "Presentation",
  "Pdf",
  "Image",
  "Folder",
  "Other",
] as const;
export type ItemType = (typeof ITEM_TYPES)[number];
export const parseItemType = makeParser(ITEM_TYPES, "Other");

// --- DocumentFormat --------------------------------------------------------
export const DOCUMENT_FORMATS = [
  "Unknown",
  "PlainText",
  "Markdown",
  "Html",
  "Word",
  "Excel",
  "PowerPoint",
  "Pdf",
  "Csv",
  "Json",
  "GoogleDoc",
  "GoogleSheet",
  "GoogleSlides",
] as const;
export type DocumentFormat = (typeof DOCUMENT_FORMATS)[number];
export const parseDocumentFormat = makeParser(DOCUMENT_FORMATS, "Unknown");

export const DOCUMENT_FORMAT_LABEL: Record<DocumentFormat, string> = {
  Unknown: "Unknown",
  PlainText: "Plain text",
  Markdown: "Markdown",
  Html: "HTML",
  Word: "Word",
  Excel: "Excel",
  PowerPoint: "PowerPoint",
  Pdf: "PDF",
  Csv: "CSV",
  Json: "JSON",
  GoogleDoc: "Google Doc",
  GoogleSheet: "Google Sheet",
  GoogleSlides: "Google Slides",
};

// Formats the document editor can author directly.
export const AUTHORABLE_FORMATS: DocumentFormat[] = [
  "Markdown",
  "PlainText",
  "Html",
  "Json",
  "Csv",
];

// --- TranslationStatus -----------------------------------------------------
export const TRANSLATION_STATUSES = ["Queued", "Processing", "Completed", "Failed"] as const;
export type TranslationStatus = (typeof TRANSLATION_STATUSES)[number];
export const parseTranslationStatus = makeParser(TRANSLATION_STATUSES, "Queued");

// --- Cadence ---------------------------------------------------------------
export const CADENCES = ["Hourly", "Daily", "Weekly", "Monthly", "OnChange"] as const;
export type Cadence = (typeof CADENCES)[number];
export const parseCadence = makeParser(CADENCES, "Daily");

// --- Connection status -----------------------------------------------------
export const CONNECTION_STATUSES = ["Active", "Expired", "Error", "Revoked"] as const;
export type ConnectionStatus = (typeof CONNECTION_STATUSES)[number];
export const parseConnectionStatus = makeParser(CONNECTION_STATUSES, "Error");

// --- Sync status -----------------------------------------------------------
export const SYNC_STATUSES = ["Idle", "Running", "Completed", "Failed"] as const;
export type SyncStatus = (typeof SYNC_STATUSES)[number];
export const parseSyncStatus = makeParser(SYNC_STATUSES, "Idle");

// --- Roles -----------------------------------------------------------------
export const ROLES = ["Admin", "ComplianceOfficer", "Member"] as const;
export type Role = (typeof ROLES)[number];
export const parseRole = makeParser(ROLES, "Member");

// --- DLP severity ----------------------------------------------------------
export const DLP_SEVERITIES = ["Low", "Medium", "High", "Critical"] as const;
export type DlpSeverity = (typeof DLP_SEVERITIES)[number];
export const parseDlpSeverity = makeParser(DLP_SEVERITIES, "Low");

// --- Entity types (AI) -----------------------------------------------------
export const ENTITY_TYPES = [
  "person",
  "organization",
  "date",
  "location",
  "amount",
] as const;
export type EntityType = (typeof ENTITY_TYPES)[number];
