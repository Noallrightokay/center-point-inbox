"use client";

import { useEffect } from "react";
import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import {
  Search,
  Cable,
  FileText,
  FolderOpen,
  Languages,
  CalendarClock,
  Sparkles,
  ShieldCheck,
  LogOut,
  Command as CommandIcon,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { cn, initials } from "@/lib/utils";
import { canSeeCompliance, roleOf, useAuthStore } from "@/stores/auth-store";
import { useCommandPalette } from "@/components/command-palette";
import { ThemeToggle } from "@/components/theme-toggle";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface NavItem {
  label: string;
  href: string;
  icon: LucideIcon;
  /** single-key jump target for the `g <key>` chord */
  chord?: string;
  gated?: boolean;
}

const NAV: NavItem[] = [
  { label: "Command Center", href: "/", icon: Search },
  { label: "Connections", href: "/connections", icon: Cable, chord: "c" },
  { label: "Documents", href: "/documents", icon: FileText, chord: "d" },
  { label: "Files & Email", href: "/browser", icon: FolderOpen, chord: "f" },
  { label: "Translation", href: "/translation", icon: Languages, chord: "t" },
  { label: "Exports", href: "/exports", icon: CalendarClock, chord: "e" },
  { label: "Assistant", href: "/assistant", icon: Sparkles, chord: "a" },
  { label: "Compliance", href: "/compliance", icon: ShieldCheck, gated: true },
];

function isActivePath(pathname: string, href: string): boolean {
  if (href === "/") return pathname === "/";
  return pathname === href || pathname.startsWith(`${href}/`);
}

export function AppShell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const router = useRouter();
  const user = useAuthStore((s) => s.user);
  const logout = useAuthStore((s) => s.logout);
  const role = roleOf(user);
  const togglePalette = useCommandPalette((s) => s.toggle);

  const items = NAV.filter((n) => !n.gated || canSeeCompliance(role));

  // Global keyboard shortcuts: Cmd/Ctrl+K palette, and `g <key>` jumps.
  useEffect(() => {
    let awaitingChord = false;
    let chordTimer: ReturnType<typeof setTimeout> | undefined;

    function inField(target: EventTarget | null): boolean {
      const el = target as HTMLElement | null;
      if (!el) return false;
      const tag = el.tagName;
      return (
        tag === "INPUT" ||
        tag === "TEXTAREA" ||
        tag === "SELECT" ||
        el.isContentEditable
      );
    }

    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        togglePalette();
        return;
      }
      if (inField(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;

      if (awaitingChord) {
        const target = items.find((n) => n.chord === e.key.toLowerCase());
        awaitingChord = false;
        if (chordTimer) clearTimeout(chordTimer);
        if (target) {
          e.preventDefault();
          router.push(target.href);
        }
        return;
      }
      if (e.key.toLowerCase() === "g") {
        awaitingChord = true;
        chordTimer = setTimeout(() => (awaitingChord = false), 1000);
      }
    }

    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      if (chordTimer) clearTimeout(chordTimer);
    };
  }, [items, router, togglePalette]);

  return (
    <div className="flex min-h-screen bg-background">
      {/* Left icon rail (md+) */}
      <aside className="fixed inset-y-0 left-0 z-40 hidden w-14 flex-col items-center border-r border-border/60 bg-rail py-3 md:flex">
        <Link
          href="/"
          className="mb-3 flex h-9 w-9 items-center justify-center rounded-md bg-primary font-semibold text-primary-foreground"
          title="Centra"
        >
          C
        </Link>
        <nav className="flex flex-1 flex-col items-center gap-1">
          {items.map((item) => {
            const active = isActivePath(pathname, item.href);
            const Icon = item.icon;
            return (
              <Link
                key={item.href}
                href={item.href}
                title={item.label}
                aria-label={item.label}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "group relative flex h-10 w-10 items-center justify-center rounded-md transition-colors",
                  active
                    ? "bg-primary/15 text-primary"
                    : "text-rail-foreground hover:bg-white/5 hover:text-foreground"
                )}
              >
                {active && (
                  <span className="absolute left-0 h-5 w-0.5 rounded-full bg-primary" />
                )}
                <Icon className="h-[18px] w-[18px]" />
              </Link>
            );
          })}
        </nav>
      </aside>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col md:pl-14">
        {/* Top bar with persistent global search */}
        <header className="sticky top-0 z-30 flex h-14 items-center gap-3 border-b border-border bg-background/85 px-4 backdrop-blur">
          <button
            onClick={togglePalette}
            className="mx-auto flex h-9 w-full max-w-md items-center gap-2 rounded-md border border-border bg-surface px-3 text-sm text-muted-foreground transition-colors hover:border-ring/60"
          >
            <Search className="h-4 w-4" />
            <span className="flex-1 text-left">Search everything…</span>
            <kbd className="hidden items-center gap-0.5 rounded border border-border bg-muted px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground sm:inline-flex">
              <CommandIcon className="h-3 w-3" />K
            </kbd>
          </button>

          <div className="flex items-center gap-1">
            <ThemeToggle />
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  className="flex h-8 w-8 items-center justify-center rounded-full bg-primary/15 text-xs font-semibold text-primary transition-colors hover:bg-primary/25"
                  aria-label="Account menu"
                >
                  {initials(user?.displayName || user?.email)}
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuLabel className="flex flex-col gap-0.5">
                  <span className="truncate text-sm font-medium normal-case text-foreground">
                    {user?.displayName || "Signed in"}
                  </span>
                  <span className="truncate text-xs font-normal normal-case text-muted-foreground">
                    {user?.email}
                  </span>
                  {role && (
                    <span className="mt-1 inline-flex w-fit rounded border border-border px-1.5 py-0.5 text-[10px] font-normal normal-case text-muted-foreground">
                      {role}
                    </span>
                  )}
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onClick={() => {
                    logout();
                    router.replace("/login");
                  }}
                  className="text-destructive focus:text-destructive"
                >
                  <LogOut className="h-4 w-4" />
                  Sign out
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>

        <main className="min-w-0 flex-1 px-4 pb-24 pt-6 md:px-8 md:pb-10">
          <div className="mx-auto w-full max-w-6xl">{children}</div>
        </main>
      </div>

      {/* Mobile bottom tab bar */}
      <nav className="fixed inset-x-0 bottom-0 z-40 flex h-16 items-center justify-around border-t border-border bg-rail px-1 md:hidden">
        {items.slice(0, 6).map((item) => {
          const active = isActivePath(pathname, item.href);
          const Icon = item.icon;
          return (
            <Link
              key={item.href}
              href={item.href}
              aria-label={item.label}
              className={cn(
                "flex h-12 w-12 flex-col items-center justify-center gap-0.5 rounded-md text-[10px]",
                active ? "text-primary" : "text-rail-foreground"
              )}
            >
              <Icon className="h-5 w-5" />
            </Link>
          );
        })}
      </nav>
    </div>
  );
}
