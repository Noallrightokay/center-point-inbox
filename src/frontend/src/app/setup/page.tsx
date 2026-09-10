"use client";

import { useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Check, X, Loader2, Copy, CheckCheck } from "lucide-react";

import { api } from "@/lib/api";
import { cn, copyToClipboard } from "@/lib/utils";
import type { SetupCheck, SetupStatus } from "@/types";
import { AuthForm } from "@/components/auth/auth-form";
import { ThemeToggle } from "@/components/theme-toggle";

function deriveChecks(status: SetupStatus): SetupCheck[] {
  if (status.checks && status.checks.length > 0) return status.checks;
  const rows: SetupCheck[] = [
    { name: "Database", ok: status.database },
    { name: "Redis", ok: status.redis },
  ];
  if (status.providers) {
    for (const [name, ok] of Object.entries(status.providers)) {
      rows.push({ name: `Provider · ${name}`, ok });
    }
  }
  rows.push({ name: "AI (Anthropic)", ok: status.ai });
  rows.push({ name: "Gotenberg", ok: status.gotenberg });
  rows.push({ name: "Administrator account", ok: status.adminExists });
  return rows;
}

function EnvHint({ hint }: { hint: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="mt-2 flex items-start justify-between gap-2 rounded-md border border-border bg-muted/50 p-2.5">
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-pre font-mono text-xs text-muted-foreground">
        {hint}
      </code>
      <button
        onClick={async () => {
          if (await copyToClipboard(hint)) {
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
          }
        }}
        className="shrink-0 text-muted-foreground hover:text-foreground"
        aria-label="Copy env hint"
      >
        {copied ? <CheckCheck className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
      </button>
    </div>
  );
}

export default function SetupPage() {
  const router = useRouter();
  const { data, isLoading } = useQuery({
    queryKey: ["setup-status"],
    queryFn: api.setupStatus,
    refetchInterval: 5000,
  });

  const checks = useMemo(() => (data ? deriveChecks(data) : []), [data]);
  const readyCount = checks.filter((c) => c.ok).length;

  return (
    <div className="min-h-screen bg-background">
      <header className="flex h-14 items-center justify-between border-b border-border px-6">
        <div className="flex items-center gap-2.5">
          <span className="flex h-8 w-8 items-center justify-center rounded-md bg-primary font-semibold text-primary-foreground">
            C
          </span>
          <span className="font-semibold text-foreground">Centra setup</span>
        </div>
        <ThemeToggle />
      </header>

      <div className="mx-auto grid max-w-5xl gap-8 px-6 py-10 lg:grid-cols-[1fr_360px]">
        {/* Checklist */}
        <section>
          <div className="mb-5 flex items-center justify-between">
            <div>
              <h1 className="text-xl font-semibold text-foreground">Instance readiness</h1>
              <p className="mt-1 text-sm text-muted-foreground">
                Live status, refreshed every 5 seconds.
              </p>
            </div>
            {data && (
              <span
                className={cn(
                  "rounded-md border px-2.5 py-1 text-xs font-medium",
                  data.ready
                    ? "border-success/30 bg-success/10 text-success"
                    : "border-warning/30 bg-warning/10 text-warning"
                )}
              >
                {readyCount}/{checks.length} ready
              </span>
            )}
          </div>

          {isLoading ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" /> Checking services…
            </div>
          ) : (
            <ul className="space-y-2">
              {checks.map((c) => (
                <li
                  key={c.name}
                  className="rounded-md border border-border bg-card p-3"
                >
                  <div className="flex items-center gap-3">
                    <span
                      className={cn(
                        "flex h-6 w-6 items-center justify-center rounded-full",
                        c.ok
                          ? "bg-success/15 text-success"
                          : "bg-destructive/15 text-destructive"
                      )}
                    >
                      {c.ok ? <Check className="h-3.5 w-3.5" /> : <X className="h-3.5 w-3.5" />}
                    </span>
                    <span className="flex-1 text-sm font-medium text-foreground">{c.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {c.ok ? "OK" : "Not configured"}
                    </span>
                  </div>
                  {c.detail && (
                    <p className="mt-1.5 pl-9 text-xs text-muted-foreground">{c.detail}</p>
                  )}
                  {!c.ok && c.envHint && (
                    <div className="pl-9">
                      <EnvHint hint={c.envHint} />
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* Admin registration or continue */}
        <aside className="lg:pt-1">
          <div className="rounded-md border border-border bg-card p-5">
            {data && !data.adminExists ? (
              <>
                <h2 className="text-base font-semibold text-foreground">
                  Create the administrator
                </h2>
                <p className="mb-4 mt-1 text-sm text-muted-foreground">
                  No admin exists yet. The first account owns this instance.
                </p>
                <AuthForm
                  mode="register"
                  allowModeSwitch={false}
                  onAuthed={() => router.replace("/")}
                />
              </>
            ) : (
              <>
                <h2 className="text-base font-semibold text-foreground">Administrator ready</h2>
                <p className="mb-4 mt-1 text-sm text-muted-foreground">
                  This instance already has an administrator. Sign in to continue.
                </p>
                <button
                  onClick={() => router.replace("/login")}
                  className="inline-flex h-9 w-full items-center justify-center rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground hover:bg-primary/90"
                >
                  Go to sign in
                </button>
              </>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
