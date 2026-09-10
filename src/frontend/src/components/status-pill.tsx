import { Badge, type BadgeProps } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import {
  parseConnectionStatus,
  parseSyncStatus,
  parseTranslationStatus,
  parseDlpSeverity,
} from "@/lib/enums";
import type { Wire } from "@/types";

type Variant = NonNullable<BadgeProps["variant"]>;

function Dot({ className }: { className?: string }) {
  return <span className={cn("h-1.5 w-1.5 rounded-full", className)} />;
}

const CONNECTION: Record<string, { variant: Variant; dot: string }> = {
  Active: { variant: "success", dot: "bg-success" },
  Expired: { variant: "warning", dot: "bg-warning" },
  Error: { variant: "destructive", dot: "bg-destructive" },
  Revoked: { variant: "secondary", dot: "bg-muted-foreground" },
};

export function ConnectionStatusPill({ status }: { status: Wire }) {
  const key = parseConnectionStatus(status);
  const s = CONNECTION[key];
  return (
    <Badge variant={s.variant} className="gap-1.5">
      <Dot className={s.dot} />
      {key}
    </Badge>
  );
}

const SYNC: Record<string, Variant> = {
  Idle: "secondary",
  Running: "info",
  Completed: "success",
  Failed: "destructive",
};

export function SyncStatusChip({ status }: { status: Wire }) {
  const key = parseSyncStatus(status);
  return (
    <Badge variant={SYNC[key]} className="gap-1.5">
      {key === "Running" && <Dot className="animate-pulse bg-info" />}
      {key}
    </Badge>
  );
}

const JOB: Record<string, Variant> = {
  Queued: "secondary",
  Processing: "info",
  Completed: "success",
  Failed: "destructive",
};

export function JobStatusChip({ status }: { status: Wire }) {
  const key = parseTranslationStatus(status);
  return (
    <Badge variant={JOB[key]} className="gap-1.5">
      {key === "Processing" && <Dot className="animate-pulse bg-info" />}
      {key}
    </Badge>
  );
}

const SEVERITY: Record<string, Variant> = {
  Low: "secondary",
  Medium: "info",
  High: "warning",
  Critical: "destructive",
};

export function SeverityBadge({ severity }: { severity: Wire }) {
  const key = parseDlpSeverity(severity);
  return <Badge variant={SEVERITY[key]}>{key}</Badge>;
}
