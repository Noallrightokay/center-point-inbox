"use client";

import { Suspense, useEffect, useMemo, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import {
  Search,
  Sparkles,
  ExternalLink,
  ChevronLeft,
  ChevronRight,
  SearchX,
  History,
  Loader2,
} from "lucide-react";

import { api } from "@/lib/api";
import { cn, highlightTerms, relativeTime } from "@/lib/utils";
import { PROVIDER_TYPES, PROVIDER_LABEL, ITEM_TYPES } from "@/lib/enums";
import type { SearchRequest } from "@/types";
import { ProviderIcon } from "@/components/provider-icon";
import { ItemTypeChip } from "@/components/chips";
import { EmptyState } from "@/components/empty-state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";

const PAGE_SIZE = 20;
const ALL = "all";

interface Committed extends SearchRequest {
  ai: boolean;
}

function CommandCenterInner() {
  const params = useSearchParams();
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState<string>(ALL);
  const [itemType, setItemType] = useState<string>(ALL);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [ai, setAi] = useState(false);
  const [committed, setCommitted] = useState<Committed | null>(null);

  const [suggestions, setSuggestions] = useState<string[]>([]);
  const [recent, setRecent] = useState<string[]>([]);
  const [focused, setFocused] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  function commit(next?: Partial<Committed>) {
    const q = (next?.query ?? query).trim();
    if (!q) return;
    setCommitted({
      query: q,
      provider: provider !== ALL ? provider : undefined,
      itemType: itemType !== ALL ? itemType : undefined,
      from: from || undefined,
      to: to || undefined,
      page: next?.page ?? 1,
      pageSize: PAGE_SIZE,
      ai: next?.ai ?? ai,
    });
    setFocused(false);
  }

  // Prefill + auto-run from ?q= (command palette hand-off).
  useEffect(() => {
    const q = params.get("q");
    if (q) {
      setQuery(q);
      setCommitted({ query: q, page: 1, pageSize: PAGE_SIZE, ai: false });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [params]);

  // Recent searches for the empty-input state.
  useEffect(() => {
    api
      .searchHistory()
      .then((h) => setRecent(h.map((e) => e.query).slice(0, 8)))
      .catch(() => setRecent([]));
  }, []);

  // Typeahead.
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setSuggestions([]);
      return;
    }
    const t = setTimeout(() => {
      api.suggest(q).then((s) => setSuggestions(s.slice(0, 6))).catch(() => setSuggestions([]));
    }, 160);
    return () => clearTimeout(t);
  }, [query]);

  const { data, isFetching, isError } = useQuery({
    queryKey: ["search", committed],
    queryFn: () =>
      api.search(
        {
          query: committed!.query,
          provider: committed!.provider,
          itemType: committed!.itemType,
          from: committed!.from,
          to: committed!.to,
          page: committed!.page,
          pageSize: committed!.pageSize,
        },
        committed!.ai
      ),
    enabled: !!committed?.query,
  });

  const totalPages = data ? Math.max(1, Math.ceil(data.total / (committed?.pageSize ?? PAGE_SIZE))) : 1;
  const showDropdown =
    focused && (query.trim().length >= 2 ? suggestions.length > 0 : recent.length > 0);

  const dropdownItems = useMemo(
    () => (query.trim().length >= 2 ? suggestions : recent),
    [query, suggestions, recent]
  );

  return (
    <div className="space-y-6">
      {/* Hero */}
      <div className="pt-4 text-center">
        <h1 className="text-2xl font-semibold tracking-tight text-foreground">Command Center</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Search across everything you&apos;ve connected.
        </p>
      </div>

      <div className="relative mx-auto max-w-2xl">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            commit();
          }}
        >
          <div className="relative">
            <Search className="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onFocus={() => setFocused(true)}
              onBlur={() => setTimeout(() => setFocused(false), 120)}
              placeholder="Search emails, documents, spreadsheets…"
              className="h-12 w-full rounded-md border border-border bg-surface pl-10 pr-28 text-sm text-foreground shadow-sm outline-none transition-colors focus:border-ring focus:ring-1 focus:ring-ring"
            />
            <div className="absolute right-2 top-1/2 -translate-y-1/2">
              <Button type="submit" size="sm">
                Search
              </Button>
            </div>
          </div>
        </form>

        {showDropdown && (
          <div className="absolute z-20 mt-1.5 w-full overflow-hidden rounded-md border border-border bg-popover p-1 shadow-lg">
            {query.trim().length < 2 && (
              <p className="px-2.5 py-1.5 text-xs font-medium text-muted-foreground">
                Recent searches
              </p>
            )}
            {dropdownItems.map((s) => (
              <button
                key={s}
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                  setQuery(s);
                  commit({ query: s });
                }}
                className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm text-foreground hover:bg-muted"
              >
                {query.trim().length < 2 ? (
                  <History className="h-3.5 w-3.5 text-muted-foreground" />
                ) : (
                  <Search className="h-3.5 w-3.5 text-muted-foreground" />
                )}
                <span className="truncate">{s}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {/* Filter row */}
      <div className="mx-auto flex max-w-3xl flex-wrap items-center justify-center gap-2">
        <FilterSelect
          value={provider}
          onChange={(v) => {
            setProvider(v);
            if (committed) commit({});
          }}
          options={[
            { value: ALL, label: "All providers" },
            ...PROVIDER_TYPES.map((p) => ({ value: p, label: PROVIDER_LABEL[p] })),
          ]}
        />
        <FilterSelect
          value={itemType}
          onChange={(v) => {
            setItemType(v);
            if (committed) commit({});
          }}
          options={[
            { value: ALL, label: "All types" },
            ...ITEM_TYPES.map((t) => ({ value: t, label: t })),
          ]}
        />
        <DateField label="From" value={from} onChange={setFrom} />
        <DateField label="To" value={to} onChange={setTo} />
        <label className="ml-1 flex cursor-pointer items-center gap-2 rounded-md border border-border bg-surface px-2.5 py-1.5 text-sm">
          <Sparkles className={cn("h-3.5 w-3.5", ai ? "text-primary" : "text-muted-foreground")} />
          <span className="text-muted-foreground">AI summary</span>
          <Switch
            checked={ai}
            onCheckedChange={(v) => {
              setAi(v);
              if (committed) commit({ ai: v });
            }}
            aria-label="Toggle AI summary"
          />
        </label>
      </div>

      {/* Results */}
      <div className="mx-auto max-w-3xl space-y-3">
        {!committed && (
          <p className="pt-6 text-center text-sm text-muted-foreground">
            Type a query above or press{" "}
            <kbd className="rounded border border-border bg-muted px-1 font-mono text-xs">⌘K</kbd>{" "}
            from anywhere.
          </p>
        )}

        {isError && (
          <p className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            Search failed. Please try again.
          </p>
        )}

        {isFetching && (
          <div className="space-y-3">
            {Array.from({ length: 5 }).map((_, i) => (
              <div key={i} className="rounded-md border border-border p-4">
                <Skeleton className="h-4 w-2/3" />
                <Skeleton className="mt-2 h-3 w-full" />
                <Skeleton className="mt-1.5 h-3 w-4/5" />
              </div>
            ))}
          </div>
        )}

        {!isFetching && data && committed?.ai && data.aiSummary && (
          <div className="rounded-md border border-primary/25 bg-primary/[0.06] p-4">
            <div className="mb-2 flex items-center gap-1.5">
              <span className="inline-flex items-center gap-1 rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-primary">
                <Sparkles className="h-3 w-3" /> AI
              </span>
              <span className="text-xs text-muted-foreground">Summary of results</span>
            </div>
            <p className="whitespace-pre-wrap text-sm leading-relaxed text-foreground">
              {data.aiSummary}
            </p>
          </div>
        )}

        {!isFetching && data && data.results.length === 0 && (
          <EmptyState
            icon={SearchX}
            title="No results"
            description="Try broadening your query, removing filters, or widening the date range."
          />
        )}

        {!isFetching &&
          data?.results.map((r) => (
            <a
              key={r.id}
              href={r.webUrl ?? undefined}
              target="_blank"
              rel="noopener noreferrer"
              className={cn(
                "group block rounded-md border border-border bg-card p-4 transition-colors hover:border-ring/50",
                !r.webUrl && "pointer-events-none"
              )}
            >
              <div className="flex items-start gap-3">
                <ProviderIcon provider={r.provider} size={22} className="mt-0.5" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <ItemTypeChip itemType={r.itemType} />
                    <span className="text-xs text-muted-foreground">
                      {relativeTime(r.modifiedAt)}
                    </span>
                  </div>
                  <h3 className="mt-1.5 truncate text-sm font-medium text-foreground">
                    {r.title || "Untitled"}
                  </h3>
                  {r.snippet && (
                    <p
                      className="mt-1 line-clamp-2 text-sm text-muted-foreground"
                      dangerouslySetInnerHTML={{
                        __html: highlightTerms(r.snippet, committed?.query ?? ""),
                      }}
                    />
                  )}
                </div>
                {r.webUrl && (
                  <ExternalLink className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
                )}
              </div>
            </a>
          ))}

        {/* Pagination */}
        {!isFetching && data && data.total > (committed?.pageSize ?? PAGE_SIZE) && (
          <div className="flex items-center justify-between pt-2">
            <span className="text-xs text-muted-foreground">
              {data.total.toLocaleString()} results
              {typeof data.tookMs === "number" && ` · ${data.tookMs}ms`}
            </span>
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={(committed?.page ?? 1) <= 1}
                onClick={() => commit({ page: (committed?.page ?? 1) - 1 })}
              >
                <ChevronLeft className="h-4 w-4" /> Prev
              </Button>
              <span className="text-xs text-muted-foreground">
                {committed?.page ?? 1} / {totalPages}
              </span>
              <Button
                variant="outline"
                size="sm"
                disabled={(committed?.page ?? 1) >= totalPages}
                onClick={() => commit({ page: (committed?.page ?? 1) + 1 })}
              >
                Next <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function FilterSelect({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="h-9 rounded-md border border-border bg-surface px-2.5 text-sm text-foreground outline-none transition-colors focus:border-ring"
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

function DateField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className="flex items-center gap-1.5 rounded-md border border-border bg-surface px-2.5 py-1 text-sm">
      <span className="text-xs text-muted-foreground">{label}</span>
      <input
        type="date"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="bg-transparent text-sm text-foreground outline-none [color-scheme:light] dark:[color-scheme:dark]"
      />
    </label>
  );
}

export default function CommandCenterPage() {
  return (
    <Suspense
      fallback={
        <div className="flex justify-center pt-20">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      }
    >
      <CommandCenterInner />
    </Suspense>
  );
}
