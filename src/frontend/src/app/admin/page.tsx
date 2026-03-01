"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import {
  Users,
  FileText,
  Languages,
  ShieldAlert,
  Activity,
  CheckCircle,
  Loader2,
  AlertTriangle,
  RotateCcw,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Separator } from "@/components/ui/separator";
import { useAuthStore } from "@/stores/auth-store";
import {
  useAdminDashboard,
  useAuditLogs,
  useDlpViolations,
  useResolveViolation,
  useUnquarantineFile,
} from "@/hooks/use-admin";
import type { DlpSeverity, DlpStatus, AuditAction } from "@/types";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const SEVERITY_STYLES: Record<DlpSeverity, string> = {
  low: "bg-blue-100 text-blue-800 border-blue-200",
  medium: "bg-yellow-100 text-yellow-800 border-yellow-200",
  high: "bg-orange-100 text-orange-800 border-orange-200",
  critical: "bg-red-100 text-red-800 border-red-200",
};

const STATUS_STYLES: Record<DlpStatus, string> = {
  open: "bg-red-100 text-red-800",
  acknowledged: "bg-yellow-100 text-yellow-800",
  resolved: "bg-green-100 text-green-800",
  false_positive: "bg-gray-100 text-gray-800",
};

const ACTION_LABELS: Partial<Record<AuditAction, string>> = {
  login: "User Login",
  logout: "User Logout",
  file_access: "File Access",
  file_download: "File Download",
  file_translate: "Translation",
  ai_query: "AI Query",
  ai_summarize: "AI Summary",
  admin_action: "Admin Action",
  dlp_violation: "DLP Violation",
  export: "Export",
  share: "Share",
};

function formatDateTime(dateStr: string): string {
  return new Date(dateStr).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatStorage(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let unitIndex = 0;
  let size = bytes;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex++;
  }
  return `${size.toFixed(1)} ${units[unitIndex]}`;
}

// ---------------------------------------------------------------------------
// Stats card component
// ---------------------------------------------------------------------------

function StatsCard({
  title,
  value,
  icon: Icon,
  description,
}: {
  title: string;
  value: string | number;
  icon: React.ElementType;
  description?: string;
}) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <CardTitle className="text-sm font-medium text-muted-foreground">
          {title}
        </CardTitle>
        <Icon className="h-4 w-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">{value}</div>
        {description && (
          <p className="text-xs text-muted-foreground mt-1">{description}</p>
        )}
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Page component
// ---------------------------------------------------------------------------

export default function AdminPage() {
  const router = useRouter();
  const user = useAuthStore((s) => s.user);
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);
  const isAdmin = user?.roles?.includes("admin") ?? false;

  // Redirect non-admin users
  useEffect(() => {
    if (!isAuthenticated) {
      router.replace("/login");
      return;
    }
    if (!isAdmin) {
      router.replace("/dashboard");
    }
  }, [isAuthenticated, isAdmin, router]);

  const {
    data: dashboard,
    isLoading: dashboardLoading,
    isError: dashboardError,
  } = useAdminDashboard();

  const { data: auditLogs, isLoading: auditLoading } = useAuditLogs({
    page: 1,
    pageSize: 10,
    sortBy: "timestamp",
    sortOrder: "desc",
  });

  const {
    data: violations,
    isLoading: violationsLoading,
  } = useDlpViolations();

  const resolveViolation = useResolveViolation();
  const unquarantineFile = useUnquarantineFile();

  function handleResolve(violationId: string) {
    resolveViolation.mutate({ violationId, status: "resolved" });
  }

  function handleUnquarantine(fileId: string) {
    unquarantineFile.mutate({ fileId });
  }

  if (!isAdmin) {
    return null;
  }

  return (
    <div className="p-6 max-w-7xl mx-auto space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Admin Dashboard</h1>
        <p className="text-sm text-muted-foreground">
          Monitor usage, compliance, and manage your organization.
        </p>
      </div>

      {/* Stats cards */}
      {dashboardLoading ? (
        <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
          {Array.from({ length: 4 }).map((_, i) => (
            <Card key={i}>
              <CardHeader className="pb-2">
                <Skeleton className="h-4 w-24" />
              </CardHeader>
              <CardContent>
                <Skeleton className="h-8 w-16" />
                <Skeleton className="h-3 w-32 mt-2" />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : dashboardError ? (
        <Card className="border-red-200 bg-red-50/50">
          <CardContent className="py-4 flex items-center gap-2 text-red-700">
            <AlertTriangle className="h-4 w-4" />
            <span className="text-sm">Failed to load dashboard data.</span>
          </CardContent>
        </Card>
      ) : dashboard ? (
        <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
          <StatsCard
            title="Total Users"
            value={dashboard.totalUsers.toLocaleString()}
            icon={Users}
            description={`${dashboard.activeUsers} active`}
          />
          <StatsCard
            title="Total Files"
            value={dashboard.totalFiles.toLocaleString()}
            icon={FileText}
            description={formatStorage(dashboard.storageUsedBytes)}
          />
          <StatsCard
            title="Total Translations"
            value={dashboard.totalTranslations.toLocaleString()}
            icon={Languages}
            description={`${dashboard.totalAiQueries} AI queries`}
          />
          <StatsCard
            title="Active Violations"
            value={dashboard.dlpViolationsOpen}
            icon={ShieldAlert}
            description="Requires attention"
          />
        </div>
      ) : null}

      <div className="grid gap-6 lg:grid-cols-2">
        {/* Recent activity table */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="text-base">Recent Activity</CardTitle>
                <CardDescription>
                  Latest audit log entries
                </CardDescription>
              </div>
              <Activity className="h-4 w-4 text-muted-foreground" />
            </div>
          </CardHeader>
          <CardContent>
            {auditLoading ? (
              <div className="space-y-3">
                {Array.from({ length: 5 }).map((_, i) => (
                  <div key={i} className="flex items-center gap-3">
                    <Skeleton className="h-8 w-8 rounded-full" />
                    <div className="flex-1 space-y-1">
                      <Skeleton className="h-3 w-3/4" />
                      <Skeleton className="h-3 w-1/2" />
                    </div>
                  </div>
                ))}
              </div>
            ) : auditLogs && auditLogs.items.length > 0 ? (
              <div className="space-y-0">
                {auditLogs.items.map((log, index) => (
                  <div key={log.id}>
                    <div className="flex items-start gap-3 py-2.5">
                      <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-medium">
                        {log.userEmail.charAt(0).toUpperCase()}
                      </div>
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <p className="text-sm font-medium truncate">
                            {log.userEmail}
                          </p>
                          <Badge
                            variant="outline"
                            className="text-[10px] px-1.5 py-0 shrink-0"
                          >
                            {ACTION_LABELS[log.action] ?? log.action}
                          </Badge>
                        </div>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          {log.resourceType}
                          {log.resourceId && ` (${log.resourceId.slice(0, 8)}...)`}
                          {" "}&middot;{" "}
                          {formatDateTime(log.timestamp)}
                        </p>
                      </div>
                    </div>
                    {index < auditLogs.items.length - 1 && <Separator />}
                  </div>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground text-center py-8">
                No recent activity
              </p>
            )}
          </CardContent>
        </Card>

        {/* DLP violations table */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="text-base">DLP Violations</CardTitle>
                <CardDescription>
                  Data loss prevention alerts
                </CardDescription>
              </div>
              <ShieldAlert className="h-4 w-4 text-muted-foreground" />
            </div>
          </CardHeader>
          <CardContent>
            {violationsLoading ? (
              <div className="space-y-3">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div key={i} className="flex items-center gap-3">
                    <Skeleton className="h-5 w-16 rounded-full" />
                    <div className="flex-1 space-y-1">
                      <Skeleton className="h-3 w-3/4" />
                      <Skeleton className="h-3 w-1/2" />
                    </div>
                    <Skeleton className="h-8 w-20" />
                  </div>
                ))}
              </div>
            ) : violations && violations.items.length > 0 ? (
              <div className="space-y-0">
                {violations.items.map((violation, index) => (
                  <div key={violation.id}>
                    <div className="flex items-start gap-3 py-2.5">
                      <div className="mt-0.5 shrink-0">
                        <Badge
                          variant="outline"
                          className={`text-[10px] px-1.5 py-0 ${
                            SEVERITY_STYLES[violation.severity]
                          }`}
                        >
                          {violation.severity.toUpperCase()}
                        </Badge>
                      </div>
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium truncate">
                          {violation.fileTitle}
                        </p>
                        <p className="text-xs text-muted-foreground mt-0.5">
                          {violation.violationType} &middot;{" "}
                          {violation.userEmail}
                        </p>
                        <p className="text-xs text-muted-foreground">
                          {formatDateTime(violation.detectedAt)}
                        </p>
                        {violation.detectedPatterns.length > 0 && (
                          <div className="flex flex-wrap gap-1 mt-1">
                            {violation.detectedPatterns
                              .slice(0, 3)
                              .map((pattern) => (
                                <Badge
                                  key={pattern}
                                  variant="secondary"
                                  className="text-[10px] px-1 py-0"
                                >
                                  {pattern}
                                </Badge>
                              ))}
                            {violation.detectedPatterns.length > 3 && (
                              <Badge
                                variant="secondary"
                                className="text-[10px] px-1 py-0"
                              >
                                +{violation.detectedPatterns.length - 3} more
                              </Badge>
                            )}
                          </div>
                        )}
                      </div>
                      <div className="flex flex-col gap-1 shrink-0">
                        <Badge
                          variant="outline"
                          className={`text-[10px] px-1.5 py-0 ${
                            STATUS_STYLES[violation.status]
                          }`}
                        >
                          {violation.status.replace("_", " ")}
                        </Badge>
                        {violation.status === "open" && (
                          <>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 text-xs"
                              disabled={resolveViolation.isPending}
                              onClick={() => handleResolve(violation.id)}
                            >
                              {resolveViolation.isPending ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                              ) : (
                                <CheckCircle className="h-3 w-3 mr-1" />
                              )}
                              Resolve
                            </Button>
                            <Button
                              variant="outline"
                              size="sm"
                              className="h-7 text-xs"
                              disabled={unquarantineFile.isPending}
                              onClick={() =>
                                handleUnquarantine(violation.fileId)
                              }
                            >
                              {unquarantineFile.isPending ? (
                                <Loader2 className="h-3 w-3 animate-spin" />
                              ) : (
                                <RotateCcw className="h-3 w-3 mr-1" />
                              )}
                              Unquarantine
                            </Button>
                          </>
                        )}
                      </div>
                    </div>
                    {index < violations.items.length - 1 && <Separator />}
                  </div>
                ))}
              </div>
            ) : (
              <div className="flex flex-col items-center justify-center py-8 text-center">
                <CheckCircle className="h-8 w-8 text-green-500 mb-2" />
                <p className="text-sm font-medium">No active violations</p>
                <p className="text-xs text-muted-foreground">
                  All clear. No DLP violations detected.
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
