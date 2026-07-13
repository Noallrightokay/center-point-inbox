"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { create } from "zustand";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import {
  Search,
  Cable,
  FileText,
  Languages,
  CalendarClock,
  Sparkles,
  FolderOpen,
  ShieldCheck,
  History,
  CornerDownLeft,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import { canSeeCompliance, useRole } from "@/stores/auth-store";

interface PaletteState {
  open: boolean;
  setOpen: (open: boolean) => void;
  toggle: () => void;
}

export const useCommandPalette = create<PaletteState>((set, get) => ({
  open: false,
  setOpen: (open) => set({ open }),
  toggle: () => set({ open: !get().open }),
}));

interface NavCommand {
  label: string;
  href: string;
  icon: LucideIcon;
  keywords?: string;
  gated?: boolean;
}

const NAV: NavCommand[] = [
  { label: "Command Center", href: "/", icon: Search, keywords: "home search" },
  { label: "Connections", href: "/connections", icon: Cable, keywords: "accounts providers oauth" },
  { label: "Documents", href: "/documents", icon: FileText, keywords: "docs notes markdown" },
  { label: "Files & Email", href: "/browser", icon: FolderOpen, keywords: "drive inbox messages" },
  { label: "Translation Studio", href: "/translation", icon: Languages, keywords: "convert format" },
  { label: "Scheduled Exports", href: "/exports", icon: CalendarClock, keywords: "cadence schedule" },
  { label: "AI Assistant", href: "/assistant", icon: Sparkles, keywords: "chat entities" },
  { label: "Compliance", href: "/compliance", icon: ShieldCheck, keywords: "dlp audit reports", gated: true },
];

export function CommandPalette() {
  const router = useRouter();
  const { open, setOpen } = useCommandPalette();
  const role = useRole();
  const [query, setQuery] = useState("");
  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [recent, setRecent] = useState<string[]>([]);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const navItems = useMemo(
    () => NAV.filter((n) => !n.gated || canSeeCompliance(role)),
    [role]
  );

  // Load recent searches when the palette opens with an empty query.
  useEffect(() => {
    if (!open) return;
    setQuery("");
    setActive(0);
    api
      .searchHistory()
      .then((h) => setRecent(h.map((e) => e.query).slice(0, 6)))
      .catch(() => setRecent([]));
  }, [open]);

  // Debounced typeahead.
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setSuggestions([]);
      return;
    }
    const t = setTimeout(() => {
      api
        .suggest(q)
        .then((s) => setSuggestions(s.slice(0, 6)))
        .catch(() => setSuggestions([]));
    }, 160);
    return () => clearTimeout(t);
  }, [query]);

  const filteredNav = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return navItems;
    return navItems.filter(
      (n) => n.label.toLowerCase().includes(q) || n.keywords?.includes(q)
    );
  }, [navItems, query]);

  const runSearch = useCallback(
    (q: string) => {
      const term = q.trim();
      if (!term) return;
      setOpen(false);
      router.push(`/?q=${encodeURIComponent(term)}`);
    },
    [router, setOpen]
  );

  // Flatten selectable rows for keyboard navigation.
  type Row =
    | { kind: "search"; value: string }
    | { kind: "suggest"; value: string }
    | { kind: "recent"; value: string }
    | { kind: "nav"; item: NavCommand };

  const rows: Row[] = useMemo(() => {
    const r: Row[] = [];
    if (query.trim()) r.push({ kind: "search", value: query.trim() });
    suggestions.forEach((s) => r.push({ kind: "suggest", value: s }));
    if (!query.trim()) recent.forEach((s) => r.push({ kind: "recent", value: s }));
    filteredNav.forEach((item) => r.push({ kind: "nav", item }));
    return r;
  }, [query, suggestions, recent, filteredNav]);

  useEffect(() => {
    setActive((a) => Math.min(a, Math.max(0, rows.length - 1)));
  }, [rows.length]);

  function selectRow(row: Row) {
    if (row.kind === "nav") {
      setOpen(false);
      router.push(row.item.href);
    } else {
      runSearch(row.value);
    }
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => (a + 1) % Math.max(1, rows.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => (a - 1 + rows.length) % Math.max(1, rows.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[active];
      if (row) selectRow(row);
      else runSearch(query);
    }
  }

  return (
    <DialogPrimitive.Root open={open} onOpenChange={setOpen}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-[80] bg-black/50 backdrop-blur-sm data-[state=open]:animate-fade-in" />
        <DialogPrimitive.Content
          onOpenAutoFocus={(e) => {
            e.preventDefault();
            inputRef.current?.focus();
          }}
          className="fixed left-1/2 top-[14%] z-[90] w-[calc(100vw-2rem)] max-w-xl -translate-x-1/2 overflow-hidden rounded-lg border border-border bg-popover shadow-2xl data-[state=open]:animate-fade-in-up"
        >
          <DialogPrimitive.Title className="sr-only">Search & commands</DialogPrimitive.Title>
          <div className="flex items-center gap-2.5 border-b border-border px-4">
            <Search className="h-4 w-4 shrink-0 text-muted-foreground" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={onKeyDown}
              placeholder="Search everything, or jump to…"
              className="h-12 w-full bg-transparent text-sm text-foreground outline-none placeholder:text-muted-foreground"
            />
          </div>

          <div className="max-h-[52vh] overflow-y-auto p-1.5">
            {rows.length === 0 && (
              <p className="px-3 py-6 text-center text-sm text-muted-foreground">
                No matches. Press Enter to search.
              </p>
            )}
            {rows.map((row, i) => {
              const isActive = i === active;
              const base =
                "flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm";
              const activeCls = isActive
                ? "bg-accent text-accent-foreground"
                : "text-foreground hover:bg-muted";
              if (row.kind === "nav") {
                const Icon = row.item.icon;
                return (
                  <button
                    key={`nav-${row.item.href}`}
                    className={cn(base, activeCls)}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => selectRow(row)}
                  >
                    <Icon className="h-4 w-4 text-muted-foreground" />
                    <span className="flex-1">{row.item.label}</span>
                    <span className="text-xs text-muted-foreground">Go</span>
                  </button>
                );
              }
              const Icon =
                row.kind === "recent" ? History : row.kind === "search" ? Search : Search;
              return (
                <button
                  key={`${row.kind}-${row.value}-${i}`}
                  className={cn(base, activeCls)}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => selectRow(row)}
                >
                  <Icon className="h-4 w-4 text-muted-foreground" />
                  <span className="flex-1 truncate">
                    {row.kind === "search" ? (
                      <>
                        Search for <span className="font-medium">“{row.value}”</span>
                      </>
                    ) : (
                      row.value
                    )}
                  </span>
                  {isActive && <CornerDownLeft className="h-3.5 w-3.5 text-muted-foreground" />}
                </button>
              );
            })}
          </div>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
