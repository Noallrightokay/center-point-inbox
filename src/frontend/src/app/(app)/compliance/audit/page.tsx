"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ChevronLeft,
  ChevronRight,
  ShieldCheck,
  ShieldAlert,
  Loader2,
  ScrollText,
} from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { formatDateTime } from "@/lib/utils";
import type { AuditVerifyResult } from "@/types";
import { useToast } from "@/app/toast-provider";
import { HashCell } from "@/components/hash-cell";
import { EmptyState } from "@/components/empty-state";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";

const PAGE_SIZE = 25;

export default function AuditPage() {
  const { toast } = useToast();
  const [page, setPage] = useState(1);
  const [category, setCategory] = useState("");
  const [userId, setUserId] = useState("");
  const [applied, setApplied] = useState<{ category?: string; userId?: string }>({});
  const [verify, setVerify] = useState<AuditVerifyResult | null>(null);
  const [verifying, setVerifying] = useState(false);

  const { data, isFetching } = useQuery({
    queryKey: ["audit", page, applied],
    queryFn: () =>
      api.audit({
        page,
        pageSize: PAGE_SIZE,
        category: applied.category || undefined,
        userId: applied.userId || undefined,
      }),
  });

  const totalPages = data ? Math.max(1, Math.ceil(data.total / PAGE_SIZE)) : 1;

  function applyFilters() {
    setPage(1);
    setApplied({ category: category.trim() || undefined, userId: userId.trim() || undefined });
  }

  async function runVerify() {
    setVerifying(true);
    try {
      setVerify(await api.auditVerify());
    } catch (err) {
      toast({ title: "Verification failed", description: (err as ApiError).message, variant: "error" });
    } finally {
      setVerifying(false);
    }
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex flex-wrap items-end gap-2">
          <div className="w-40">
            <Input
              placeholder="Category"
              value={category}
              onChange={(e) => setCategory(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && applyFilters()}
            />
          </div>
          <div className="w-52">
            <Input
              placeholder="User ID"
              value={userId}
              onChange={(e) => setUserId(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && applyFilters()}
            />
          </div>
          <Button variant="secondary" size="sm" onClick={applyFilters}>
            Filter
          </Button>
        </div>
        <Button variant="outline" size="sm" onClick={runVerify} disabled={verifying}>
          {verifying ? <Loader2 className="h-4 w-4 animate-spin" /> : <ShieldCheck className="h-4 w-4" />}
          Verify chain
        </Button>
      </div>

      {verify && (
        <div
          className={
            verify.intact
              ? "flex items-center gap-3 rounded-md border border-success/30 bg-success/10 p-3"
              : "flex items-center gap-3 rounded-md border border-destructive/30 bg-destructive/10 p-3"
          }
        >
          {verify.intact ? (
            <ShieldCheck className="h-5 w-5 text-success" />
          ) : (
            <ShieldAlert className="h-5 w-5 text-destructive" />
          )}
          <p className="text-sm text-foreground">
            {verify.intact
              ? `Chain verified${typeof verify.checkedCount === "number" ? ` · ${verify.checkedCount} records` : ""}.`
              : verify.message || `Chain broken${verify.brokenAt ? ` at ${verify.brokenAt}` : ""}.`}
          </p>
        </div>
      )}

      {isFetching ? (
        <div className="space-y-2">
          {Array.from({ length: 8 }).map((_, i) => (
            <Skeleton key={i} className="h-11 w-full" />
          ))}
        </div>
      ) : !data || data.entries.length === 0 ? (
        <EmptyState icon={ScrollText} title="No audit records" description="Nothing matches these filters." />
      ) : (
        <div className="overflow-x-auto rounded-md border border-border">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                <th className="px-3 py-2 font-medium">Timestamp</th>
                <th className="px-3 py-2 font-medium">Category</th>
                <th className="px-3 py-2 font-medium">Action</th>
                <th className="px-3 py-2 font-medium">Detail</th>
                <th className="px-3 py-2 font-medium">Hash</th>
              </tr>
            </thead>
            <tbody>
              {data.entries.map((e) => (
                <tr key={e.id} className="border-b border-border last:border-0">
                  <td className="whitespace-nowrap px-3 py-2 text-xs text-muted-foreground">
                    {formatDateTime(e.timestamp)}
                  </td>
                  <td className="px-3 py-2">
                    <Badge variant="secondary">{e.category}</Badge>
                  </td>
                  <td className="px-3 py-2 text-foreground">{e.action}</td>
                  <td className="max-w-[280px] px-3 py-2">
                    <span className="block truncate text-muted-foreground" title={e.detail ?? ""}>
                      {e.detail || "—"}
                    </span>
                  </td>
                  <td className="px-3 py-2">
                    <HashCell hash={e.hash} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {data && data.total > PAGE_SIZE && (
        <div className="flex items-center justify-between">
          <span className="text-xs text-muted-foreground">{data.total.toLocaleString()} records</span>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" disabled={page <= 1} onClick={() => setPage((p) => p - 1)}>
              <ChevronLeft className="h-4 w-4" /> Prev
            </Button>
            <span className="text-xs text-muted-foreground">
              {page} / {totalPages}
            </span>
            <Button
              variant="outline"
              size="sm"
              disabled={page >= totalPages}
              onClick={() => setPage((p) => p + 1)}
            >
              Next <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
