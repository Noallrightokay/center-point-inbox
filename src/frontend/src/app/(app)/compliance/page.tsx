"use client";

import { useQuery } from "@tanstack/react-query";
import {
  Users,
  Cable,
  Database,
  ShieldAlert,
  ShieldCheck,
  Shield,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { api } from "@/lib/api";
import { formatNumber } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";

function StatCard({
  icon: Icon,
  label,
  value,
  tone = "default",
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  tone?: "default" | "warning";
}) {
  return (
    <div className="rounded-md border border-border bg-card p-4">
      <div className="flex items-center gap-2 text-muted-foreground">
        <Icon className={tone === "warning" ? "h-4 w-4 text-warning" : "h-4 w-4"} />
        <span className="text-xs font-medium uppercase tracking-wide">{label}</span>
      </div>
      <p className="mt-2 text-2xl font-semibold tabular-nums text-foreground">{value}</p>
    </div>
  );
}

export default function ComplianceDashboardPage() {
  const { data, isLoading } = useQuery({
    queryKey: ["compliance-dashboard"],
    queryFn: api.complianceDashboard,
  });

  if (isLoading) {
    return (
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <Skeleton key={i} className="h-24 w-full" />
        ))}
      </div>
    );
  }

  if (!data) return <p className="text-sm text-muted-foreground">Dashboard unavailable.</p>;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Shield className="h-4 w-4" /> Active profile
          <Badge variant="default">{data.activeProfile}</Badge>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard icon={Users} label="Users" value={formatNumber(data.users)} />
        <StatCard icon={Cable} label="Active connections" value={formatNumber(data.activeConnections)} />
        <StatCard icon={Database} label="Indexed items" value={formatNumber(data.indexedItems)} />
        <StatCard
          icon={ShieldAlert}
          label="DLP violations · 30d"
          value={formatNumber(data.dlpViolations30d)}
          tone={data.dlpViolations30d > 0 ? "warning" : "default"}
        />
      </div>

      {/* Audit chain integrity */}
      <div
        className={
          data.auditChainIntact
            ? "flex items-center gap-3 rounded-md border border-success/30 bg-success/10 p-4"
            : "flex items-center gap-3 rounded-md border border-destructive/30 bg-destructive/10 p-4"
        }
      >
        {data.auditChainIntact ? (
          <ShieldCheck className="h-6 w-6 text-success" />
        ) : (
          <ShieldAlert className="h-6 w-6 text-destructive" />
        )}
        <div>
          <p className="text-sm font-medium text-foreground">
            {data.auditChainIntact ? "Audit chain intact" : "Audit chain compromised"}
          </p>
          <p className="text-xs text-muted-foreground">
            {data.auditChainIntact
              ? "Every audit record hashes correctly against its predecessor."
              : "The audit hash chain failed verification. Investigate immediately."}
          </p>
        </div>
      </div>
    </div>
  );
}
