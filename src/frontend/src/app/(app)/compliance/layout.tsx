"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { ShieldAlert } from "lucide-react";

import { cn } from "@/lib/utils";
import { canSeeCompliance, isAdmin, roleOf, useAuthStore } from "@/stores/auth-store";
import { EmptyState } from "@/components/empty-state";

interface Tab {
  label: string;
  href: string;
  adminOnly?: boolean;
}

const TABS: Tab[] = [
  { label: "Dashboard", href: "/compliance" },
  { label: "DLP tester", href: "/compliance/dlp" },
  { label: "Reports", href: "/compliance/reports" },
  { label: "Audit log", href: "/compliance/audit" },
  { label: "Metrics", href: "/compliance/metrics", adminOnly: true },
];

export default function ComplianceLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const user = useAuthStore((s) => s.user);
  const role = roleOf(user);

  if (!canSeeCompliance(role)) {
    return (
      <EmptyState
        icon={ShieldAlert}
        title="Restricted area"
        description="Compliance and administration are available to Compliance Officers and Administrators only."
      />
    );
  }

  const tabs = TABS.filter((t) => !t.adminOnly || isAdmin(role));

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-semibold tracking-tight text-foreground">Compliance &amp; Admin</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Governance, data-loss prevention, and instance health.
        </p>
      </div>

      <div className="flex gap-1 overflow-x-auto border-b border-border">
        {tabs.map((t) => {
          const active = pathname === t.href;
          return (
            <Link
              key={t.href}
              href={t.href}
              className={cn(
                "-mb-px whitespace-nowrap border-b-2 px-3 py-2 text-sm font-medium transition-colors",
                active
                  ? "border-primary text-foreground"
                  : "border-transparent text-muted-foreground hover:text-foreground"
              )}
            >
              {t.label}
            </Link>
          );
        })}
      </div>

      {children}
    </div>
  );
}
