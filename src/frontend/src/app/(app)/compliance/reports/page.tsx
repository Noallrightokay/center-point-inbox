"use client";

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { BarChart3, ShieldCheck, ShieldAlert } from "lucide-react";

import { api } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/empty-state";

// Restrained categorical palette (theme tokens + two fixed accents).
const SERIES = [
  "hsl(var(--primary))",
  "hsl(var(--info))",
  "hsl(var(--warning))",
  "hsl(var(--success))",
  "hsl(262 60% 60%)",
  "hsl(199 70% 50%)",
];

interface Params {
  from?: string;
  to?: string;
  profile?: string;
}

function BarList({ data }: { data: { rule: string; count: number }[] }) {
  const max = Math.max(1, ...data.map((d) => d.count));
  return (
    <div className="space-y-2.5">
      {data.map((d) => (
        <div key={d.rule}>
          <div className="mb-1 flex items-center justify-between text-xs">
            <span className="truncate text-foreground">{d.rule}</span>
            <span className="tabular-nums text-muted-foreground">{d.count}</span>
          </div>
          <div className="h-2 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full bg-primary"
              style={{ width: `${(d.count / max) * 100}%` }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}

function Donut({ data }: { data: { action: string; count: number }[] }) {
  const total = data.reduce((s, d) => s + d.count, 0);
  const gradient = useMemo(() => {
    if (total === 0) return "hsl(var(--muted))";
    let acc = 0;
    const stops = data.map((d, i) => {
      const start = (acc / total) * 100;
      acc += d.count;
      const end = (acc / total) * 100;
      return `${SERIES[i % SERIES.length]} ${start}% ${end}%`;
    });
    return `conic-gradient(${stops.join(", ")})`;
  }, [data, total]);

  return (
    <div className="flex items-center gap-5">
      <div
        className="relative h-32 w-32 shrink-0 rounded-full"
        style={{ background: gradient }}
        role="img"
        aria-label="Violations by action"
      >
        <div className="absolute inset-[22%] flex flex-col items-center justify-center rounded-full bg-card">
          <span className="text-lg font-semibold tabular-nums text-foreground">{total}</span>
          <span className="text-[10px] text-muted-foreground">total</span>
        </div>
      </div>
      <ul className="space-y-1.5 text-sm">
        {data.map((d, i) => (
          <li key={d.action} className="flex items-center gap-2">
            <span
              className="h-2.5 w-2.5 rounded-sm"
              style={{ background: SERIES[i % SERIES.length] }}
            />
            <span className="text-foreground">{d.action}</span>
            <span className="tabular-nums text-muted-foreground">{d.count}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export default function ReportsPage() {
  const { data: profiles } = useQuery({ queryKey: ["dlp-profiles"], queryFn: api.dlpProfiles });
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [profile, setProfile] = useState("");
  const [params, setParams] = useState<Params | null>(null);

  const { data, isFetching } = useQuery({
    queryKey: ["compliance-report", params],
    queryFn: () =>
      api.complianceReport({
        from: params?.from,
        to: params?.to,
        profile: params?.profile,
      }),
    enabled: !!params,
  });

  const inputCls =
    "h-9 rounded-md border border-border bg-surface px-2.5 text-sm text-foreground outline-none focus:border-ring [color-scheme:light] dark:[color-scheme:dark]";

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-end gap-3 rounded-md border border-border bg-card p-3">
        <div className="space-y-1">
          <Label htmlFor="rep-from">From</Label>
          <input id="rep-from" type="date" value={from} onChange={(e) => setFrom(e.target.value)} className={inputCls} />
        </div>
        <div className="space-y-1">
          <Label htmlFor="rep-to">To</Label>
          <input id="rep-to" type="date" value={to} onChange={(e) => setTo(e.target.value)} className={inputCls} />
        </div>
        <div className="space-y-1">
          <Label htmlFor="rep-profile">Profile</Label>
          <select
            id="rep-profile"
            value={profile}
            onChange={(e) => setProfile(e.target.value)}
            className={inputCls}
          >
            <option value="">All profiles</option>
            {profiles?.map((p) => (
              <option key={p.id} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        <Button
          onClick={() =>
            setParams({ from: from || undefined, to: to || undefined, profile: profile || undefined })
          }
        >
          <BarChart3 className="h-4 w-4" /> Run report
        </Button>
      </div>

      {!params ? (
        <EmptyState
          icon={BarChart3}
          title="Run a report"
          description="Choose a date range and profile to see violations by rule and action."
        />
      ) : isFetching ? (
        <div className="grid gap-4 lg:grid-cols-2">
          <Skeleton className="h-56 w-full" />
          <Skeleton className="h-56 w-full" />
        </div>
      ) : !data ? (
        <p className="text-sm text-muted-foreground">No report data.</p>
      ) : (
        <div className="space-y-4">
          <div className="grid gap-4 lg:grid-cols-2">
            <div className="rounded-md border border-border bg-card p-4">
              <h3 className="mb-3 text-sm font-medium text-foreground">Violations by rule</h3>
              {data.byRule.length ? (
                <BarList data={data.byRule} />
              ) : (
                <p className="text-sm text-muted-foreground">No violations in range.</p>
              )}
            </div>
            <div className="rounded-md border border-border bg-card p-4">
              <h3 className="mb-3 text-sm font-medium text-foreground">Violations by action</h3>
              {data.byAction.length ? (
                <Donut data={data.byAction} />
              ) : (
                <p className="text-sm text-muted-foreground">No violations in range.</p>
              )}
            </div>
          </div>

          <div
            className={
              data.chainIntact
                ? "flex items-center gap-3 rounded-md border border-success/30 bg-success/10 p-4"
                : "flex items-center gap-3 rounded-md border border-destructive/30 bg-destructive/10 p-4"
            }
          >
            {data.chainIntact ? (
              <ShieldCheck className="h-5 w-5 text-success" />
            ) : (
              <ShieldAlert className="h-5 w-5 text-destructive" />
            )}
            <p className="text-sm text-foreground">
              Audit chain {data.chainIntact ? "verified intact" : "failed verification"} for this period.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
