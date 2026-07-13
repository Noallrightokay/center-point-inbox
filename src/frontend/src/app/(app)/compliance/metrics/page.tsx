"use client";

import { useQuery } from "@tanstack/react-query";
import { Cpu, Activity, AlertTriangle, Timer, Boxes, ShieldAlert } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { api } from "@/lib/api";
import { formatBytes, formatNumber } from "@/lib/utils";
import { isAdmin, roleOf, useAuthStore } from "@/stores/auth-store";
import { EmptyState } from "@/components/empty-state";
import { Skeleton } from "@/components/ui/skeleton";

function Metric({
  icon: Icon,
  label,
  value,
  sub,
  tone = "default",
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  sub?: string;
  tone?: "default" | "warning";
}) {
  return (
    <div className="rounded-md border border-border bg-card p-4">
      <div className="flex items-center gap-2 text-muted-foreground">
        <Icon className={tone === "warning" ? "h-4 w-4 text-warning" : "h-4 w-4"} />
        <span className="text-xs font-medium uppercase tracking-wide">{label}</span>
      </div>
      <p className="mt-2 text-2xl font-semibold tabular-nums text-foreground">{value}</p>
      {sub && <p className="mt-0.5 text-xs text-muted-foreground">{sub}</p>}
    </div>
  );
}

export default function MetricsPage() {
  const user = useAuthStore((s) => s.user);
  const admin = isAdmin(roleOf(user));

  const { data, isLoading } = useQuery({
    queryKey: ["admin-metrics"],
    queryFn: api.adminMetrics,
    refetchInterval: 10000,
    enabled: admin,
  });

  if (!admin) {
    return (
      <EmptyState
        icon={ShieldAlert}
        title="Administrators only"
        description="Instance metrics are restricted to administrators."
      />
    );
  }

  if (isLoading) {
    return (
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <Skeleton key={i} className="h-24 w-full" />
        ))}
      </div>
    );
  }

  if (!data) return <p className="text-sm text-muted-foreground">Metrics unavailable.</p>;

  const memPct =
    data.memoryTotalBytes && data.memoryTotalBytes > 0
      ? Math.round((data.memoryUsedBytes / data.memoryTotalBytes) * 100)
      : null;
  const errorPct = (data.errorRate <= 1 ? data.errorRate * 100 : data.errorRate).toFixed(2);
  const entityEntries = Object.entries(data.entityCounts ?? {});

  return (
    <div className="space-y-4">
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <Metric
          icon={Cpu}
          label="Memory"
          value={formatBytes(data.memoryUsedBytes)}
          sub={
            data.memoryTotalBytes
              ? `${memPct}% of ${formatBytes(data.memoryTotalBytes)}`
              : undefined
          }
        />
        <Metric icon={Boxes} label="Threads" value={formatNumber(data.threads)} />
        <Metric icon={Activity} label="Requests" value={formatNumber(data.requestCount)} />
        <Metric icon={Timer} label="Avg response" value={`${Math.round(data.avgResponseMs)}ms`} />
        <Metric
          icon={AlertTriangle}
          label="Error rate"
          value={`${errorPct}%`}
          tone={Number(errorPct) >= 1 ? "warning" : "default"}
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <div className="rounded-md border border-border bg-card p-4">
          <h3 className="mb-3 text-sm font-medium text-foreground">Top endpoints</h3>
          {data.topEndpoints.length === 0 ? (
            <p className="text-sm text-muted-foreground">No endpoint data.</p>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border text-left text-xs text-muted-foreground">
                  <th className="py-1.5 font-medium">Endpoint</th>
                  <th className="py-1.5 text-right font-medium">Calls</th>
                  <th className="py-1.5 text-right font-medium">Avg</th>
                </tr>
              </thead>
              <tbody>
                {data.topEndpoints.map((e) => (
                  <tr key={e.endpoint} className="border-b border-border last:border-0">
                    <td className="py-1.5 font-mono text-xs text-foreground">{e.endpoint}</td>
                    <td className="py-1.5 text-right tabular-nums text-muted-foreground">
                      {formatNumber(e.count)}
                    </td>
                    <td className="py-1.5 text-right tabular-nums text-muted-foreground">
                      {typeof e.avgMs === "number" ? `${Math.round(e.avgMs)}ms` : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        <div className="rounded-md border border-border bg-card p-4">
          <h3 className="mb-3 text-sm font-medium text-foreground">Entity counts</h3>
          {entityEntries.length === 0 ? (
            <p className="text-sm text-muted-foreground">No entities tracked.</p>
          ) : (
            <ul className="space-y-2">
              {entityEntries.map(([name, count]) => (
                <li key={name} className="flex items-center justify-between text-sm">
                  <span className="capitalize text-foreground">{name}</span>
                  <span className="font-mono tabular-nums text-muted-foreground">
                    {formatNumber(count)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
