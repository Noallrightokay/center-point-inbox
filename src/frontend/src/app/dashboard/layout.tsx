"use client";

import { useState, useEffect } from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import {
  Inbox,
  LayoutDashboard,
  Shield,
  LogOut,
  ChevronLeft,
  ChevronRight,
  Bot,
  Settings,
  Mail,
  Plus,
  Search,
  Bell,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { AiAssistant } from "@/components/ai-assistant";
import { useAuthStore } from "@/stores/auth-store";

const NAV_ITEMS = [
  { href: "/dashboard", label: "Inbox", icon: Inbox },
  { href: "/dashboard/files", label: "Files", icon: LayoutDashboard },
  { href: "/dashboard/settings", label: "Settings", icon: Settings },
  { href: "/admin", label: "Admin", icon: Shield, requiresAdmin: true },
] as const;

const CONNECTED_PROVIDERS = [
  { name: "Gmail", icon: "G", color: "bg-red-500/20 text-red-400" },
  { name: "Outlook", icon: "O", color: "bg-blue-500/20 text-blue-400" },
];

export default function DashboardLayout({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const { user, isAuthenticated, logout } = useAuthStore();

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [aiAssistantOpen, setAiAssistantOpen] = useState(false);

  const isAdmin = user?.roles?.includes("admin") ?? false;

  useEffect(() => {
    if (!isAuthenticated) {
      router.replace("/login");
    }
  }, [isAuthenticated, router]);

  async function handleLogout() {
    await logout();
    router.replace("/login");
  }

  function getInitials(name: string | undefined): string {
    if (!name) return "?";
    return name.split(" ").map((n) => n[0]).join("").toUpperCase().slice(0, 2);
  }

  if (!isAuthenticated) return null;

  return (
    <div className="flex h-screen bg-[hsl(var(--background))]">
      {/* Sidebar */}
      <aside className={`flex flex-col border-r border-white/5 bg-amex-card transition-all duration-300 ${sidebarCollapsed ? "w-16" : "w-64"}`}>
        {/* Brand */}
        <div className="flex h-14 items-center gap-2.5 border-b border-white/5 px-4">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-amex-gold shadow-amex">
            <Mail className="h-4 w-4 text-amex-black" />
          </div>
          {!sidebarCollapsed && (
            <span className="text-sm font-bold tracking-tight gold-text">Center Point</span>
          )}
        </div>

        {/* Quick compose */}
        {!sidebarCollapsed && (
          <div className="p-3">
            <Button className="w-full bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold gap-2 shadow-amex-glow" size="sm">
              <Plus className="h-4 w-4" />
              Compose
            </Button>
          </div>
        )}

        {/* Navigation */}
        <nav className="flex-1 space-y-0.5 p-2">
          {NAV_ITEMS.map((item) => {
            if ("requiresAdmin" in item && item.requiresAdmin && !isAdmin) return null;
            const isActive = pathname === item.href || (item.href !== "/dashboard" && pathname.startsWith(item.href));
            const Icon = item.icon;
            return (
              <Link
                key={item.href}
                href={item.href}
                className={`flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm transition-all duration-200 ${
                  isActive
                    ? "bg-primary-500/15 text-primary-300 font-medium border border-primary-500/20"
                    : "text-secondary-400 hover:bg-white/5 hover:text-secondary-200"
                } ${sidebarCollapsed ? "justify-center" : ""}`}
                title={sidebarCollapsed ? item.label : undefined}
              >
                <Icon className={`h-4 w-4 shrink-0 ${isActive ? "text-primary-400" : ""}`} />
                {!sidebarCollapsed && <span>{item.label}</span>}
                {!sidebarCollapsed && item.label === "Inbox" && (
                  <Badge className="ml-auto bg-primary-500/20 text-primary-300 border-primary-500/30 text-[10px] px-1.5">
                    12
                  </Badge>
                )}
              </Link>
            );
          })}
        </nav>

        {/* Connected accounts */}
        {!sidebarCollapsed && (
          <div className="border-t border-white/5 px-4 py-3">
            <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-secondary-500">
              Connected
            </p>
            <div className="space-y-1.5">
              {CONNECTED_PROVIDERS.map((p) => (
                <div key={p.name} className="flex items-center gap-2">
                  <div className={`flex h-6 w-6 items-center justify-center rounded-md text-xs font-bold ${p.color}`}>
                    {p.icon}
                  </div>
                  <span className="text-xs text-secondary-300">{p.name}</span>
                  <div className="ml-auto h-1.5 w-1.5 rounded-full bg-green-500" />
                </div>
              ))}
              <Link href="/connect-email" className="flex items-center gap-2 mt-2 text-xs text-primary-400 hover:text-primary-300 transition-colors">
                <Plus className="h-3 w-3" />
                Add account
              </Link>
            </div>
          </div>
        )}

        {/* Collapse */}
        <div className="border-t border-white/5 p-2">
          <Button variant="ghost" size="sm" className="w-full justify-center text-secondary-500 hover:text-secondary-300" onClick={() => setSidebarCollapsed(!sidebarCollapsed)}>
            {sidebarCollapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronLeft className="h-4 w-4" />}
          </Button>
        </div>
      </aside>

      {/* Main area */}
      <div className="flex flex-1 flex-col overflow-hidden">
        {/* Top header */}
        <header className="flex h-14 items-center justify-between border-b border-white/5 bg-amex-card px-6">
          <div className="flex items-center gap-3">
            <div className="relative">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-secondary-500" />
              <input
                type="text"
                placeholder="Search everything..."
                className="h-9 w-64 rounded-lg bg-white/5 border border-white/10 pl-9 pr-4 text-sm text-secondary-200 placeholder:text-secondary-500 focus:outline-none focus:border-primary-500/50 focus:ring-1 focus:ring-primary-500/20 transition-all"
              />
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="ghost" size="icon" className="relative text-secondary-400 hover:text-secondary-200" onClick={() => setAiAssistantOpen(true)}>
              <Bot className="h-4 w-4" />
            </Button>
            <Button variant="ghost" size="icon" className="relative text-secondary-400 hover:text-secondary-200">
              <Bell className="h-4 w-4" />
              <span className="absolute top-1.5 right-1.5 h-2 w-2 rounded-full bg-primary-500 animate-pulse-gold" />
            </Button>

            <div className="w-px h-6 bg-white/10 mx-1" />

            <div className="flex items-center gap-2">
              <Avatar className="h-8 w-8 border border-white/10">
                {user?.avatarUrl && <AvatarImage src={user.avatarUrl} alt={user.name} />}
                <AvatarFallback className="bg-amex-gold/20 text-primary-300 text-xs font-bold">
                  {getInitials(user?.name)}
                </AvatarFallback>
              </Avatar>
              {!sidebarCollapsed && (
                <div className="hidden sm:block">
                  <p className="text-sm font-medium leading-none">{user?.name ?? "User"}</p>
                  <p className="text-[11px] text-secondary-500">{user?.email ?? ""}</p>
                </div>
              )}
            </div>

            <Button variant="ghost" size="icon" onClick={handleLogout} title="Sign out" className="text-secondary-500 hover:text-red-400">
              <LogOut className="h-4 w-4" />
            </Button>
          </div>
        </header>

        <main className="flex-1 overflow-y-auto scrollbar-thin">{children}</main>
      </div>

      <AiAssistant open={aiAssistantOpen} onOpenChange={setAiAssistantOpen} />
    </div>
  );
}
