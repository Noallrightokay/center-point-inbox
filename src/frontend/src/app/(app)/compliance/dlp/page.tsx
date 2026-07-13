"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ScanLine, Loader2, ShieldCheck } from "lucide-react";

import { api, ApiError } from "@/lib/api";
import type { DlpScanResult } from "@/types";
import { useToast } from "@/app/toast-provider";
import { SeverityBadge } from "@/components/status-pill";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";

const SAMPLE =
  "Please wire the payment to account 4111 1111 1111 1111. Contact John Doe at john.doe@example.com or 555-011-2233. SSN 123-45-6789.";

export default function DlpTesterPage() {
  const { toast } = useToast();
  const { data: profiles } = useQuery({ queryKey: ["dlp-profiles"], queryFn: api.dlpProfiles });
  const [text, setText] = useState("");
  const [profile, setProfile] = useState<string>("");
  const [result, setResult] = useState<DlpScanResult | null>(null);
  const [scanning, setScanning] = useState(false);

  async function scan() {
    if (!text.trim() || scanning) return;
    setScanning(true);
    try {
      setResult(await api.dlpScan(text, profile || undefined));
    } catch (err) {
      toast({ title: "Scan failed", description: (err as ApiError).message, variant: "error" });
    } finally {
      setScanning(false);
    }
  }

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <div className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="dlp-profile">Profile</Label>
          <select
            id="dlp-profile"
            value={profile}
            onChange={(e) => setProfile(e.target.value)}
            className="h-8 rounded-md border border-border bg-surface px-2 text-sm text-foreground outline-none focus:border-ring"
          >
            <option value="">Instance default</option>
            {profiles?.map((p) => (
              <option key={p.id} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
        </div>

        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Paste content to scan for sensitive data…"
          className="min-h-[260px]"
        />
        <div className="flex items-center gap-2">
          <Button onClick={scan} disabled={!text.trim() || scanning}>
            {scanning ? <Loader2 className="h-4 w-4 animate-spin" /> : <ScanLine className="h-4 w-4" />}
            Scan
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setText(SAMPLE)}>
            Load sample
          </Button>
        </div>
      </div>

      <div className="space-y-4">
        {!result ? (
          <div className="rounded-md border border-border bg-card p-6 text-center text-sm text-muted-foreground">
            Run a scan to see matches and a redacted preview.
          </div>
        ) : (
          <>
            {result.matches.length === 0 ? (
              <div className="flex items-center gap-3 rounded-md border border-success/30 bg-success/10 p-4">
                <ShieldCheck className="h-5 w-5 text-success" />
                <p className="text-sm text-foreground">No sensitive data detected.</p>
              </div>
            ) : (
              <div className="overflow-hidden rounded-md border border-border">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                      <th className="px-3 py-2 font-medium">Rule</th>
                      <th className="px-3 py-2 font-medium">Severity</th>
                      <th className="px-3 py-2 font-medium">Action</th>
                      <th className="px-3 py-2 font-medium">Count</th>
                      <th className="px-3 py-2 font-medium">Sample</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.matches.map((m, i) => (
                      <tr key={`${m.rule}-${i}`} className="border-b border-border last:border-0">
                        <td className="px-3 py-2 font-medium text-foreground">{m.rule}</td>
                        <td className="px-3 py-2">
                          <SeverityBadge severity={m.severity} />
                        </td>
                        <td className="px-3 py-2 text-muted-foreground">{m.action}</td>
                        <td className="px-3 py-2 tabular-nums text-muted-foreground">{m.count}</td>
                        <td className="px-3 py-2 font-mono text-xs text-muted-foreground">
                          {m.redactedSample || "—"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            <div>
              <p className="mb-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                Redacted content · {result.profile}
              </p>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded-md border border-border bg-muted/40 p-3 font-mono text-xs text-foreground">
                {result.redactedContent}
              </pre>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
