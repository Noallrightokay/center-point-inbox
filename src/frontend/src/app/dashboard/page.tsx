"use client";

import { useState, useMemo, useCallback } from "react";
import {
  Search, SlidersHorizontal, Bot, ChevronLeft, ChevronRight,
  Mail, Star, Clock, AlertTriangle, Paperclip, Filter,
} from "lucide-react";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
import { FileCard } from "@/components/file-card";
import { useFiles } from "@/hooks/use-files";
import type { FileSource, FileFeedQuery } from "@/types";

type ProviderTab = "all" | "google" | "microsoft" | "imap";
type SortOption = "date" | "relevance" | "size";

const ITEMS_PER_PAGE = 20;

const QUICK_FILTERS = [
  { label: "Unread", icon: Mail, filter: { isRead: false } },
  { label: "Starred", icon: Star, filter: { isStarred: true } },
  { label: "Attachments", icon: Paperclip, filter: {} },
  { label: "Flagged", icon: AlertTriangle, filter: {} },
];

export default function DashboardPage() {
  const [providerTab, setProviderTab] = useState<ProviderTab>("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [sortBy, setSortBy] = useState<SortOption>("date");
  const [cursor, setCursor] = useState<string | undefined>(undefined);
  const [cursorHistory, setCursorHistory] = useState<string[]>([]);
  const [activeFilter, setActiveFilter] = useState<string | null>(null);

  const sourceFilter = useMemo((): FileSource | undefined => {
    switch (providerTab) {
      case "google": return "google_drive";
      case "microsoft": return "onedrive";
      default: return undefined;
    }
  }, [providerTab]);

  const query: FileFeedQuery = useMemo(() => ({
    source: sourceFilter,
    search: debouncedSearch || undefined,
    sortBy,
    sortOrder: "desc",
    cursor,
    limit: ITEMS_PER_PAGE,
  }), [sourceFilter, debouncedSearch, sortBy, cursor]);

  const { data, isLoading, isError, error, isFetching } = useFiles(query);

  const handleSearchChange = useCallback((value: string) => {
    setSearchQuery(value);
    setCursor(undefined);
    setCursorHistory([]);
    const timer = setTimeout(() => setDebouncedSearch(value), 300);
    return () => clearTimeout(timer);
  }, []);

  function handleNextPage() {
    if (data?.nextCursor) {
      setCursorHistory((prev) => [...prev, cursor ?? ""]);
      setCursor(data.nextCursor);
    }
  }

  function handlePrevPage() {
    if (cursorHistory.length > 0) {
      const prev = [...cursorHistory];
      const prevCursor = prev.pop();
      setCursorHistory(prev);
      setCursor(prevCursor || undefined);
    }
  }

  function handleProviderChange(tab: string) {
    setProviderTab(tab as ProviderTab);
    setCursor(undefined);
    setCursorHistory([]);
  }

  const currentPage = cursorHistory.length + 1;
  const totalPages = data ? Math.ceil(data.totalCount / ITEMS_PER_PAGE) : 0;

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="border-b border-white/5 bg-amex-card px-6 py-3">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <Tabs value={providerTab} onValueChange={handleProviderChange}>
            <TabsList className="bg-white/5 border border-white/10">
              <TabsTrigger value="all" className="data-[state=active]:bg-primary-500/20 data-[state=active]:text-primary-300">All</TabsTrigger>
              <TabsTrigger value="google" className="data-[state=active]:bg-primary-500/20 data-[state=active]:text-primary-300">Google</TabsTrigger>
              <TabsTrigger value="microsoft" className="data-[state=active]:bg-primary-500/20 data-[state=active]:text-primary-300">Microsoft</TabsTrigger>
              <TabsTrigger value="imap" className="data-[state=active]:bg-primary-500/20 data-[state=active]:text-primary-300">Other</TabsTrigger>
            </TabsList>
          </Tabs>

          <div className="flex items-center gap-2">
            <Select value={sortBy} onValueChange={(v) => { setSortBy(v as SortOption); setCursor(undefined); setCursorHistory([]); }}>
              <SelectTrigger className="w-[130px] h-9 bg-white/5 border-white/10 text-secondary-300">
                <SlidersHorizontal className="h-3.5 w-3.5 mr-1.5 text-secondary-500" />
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="bg-amex-charcoal-light border-white/10">
                <SelectItem value="date">Modified</SelectItem>
                <SelectItem value="relevance">Name</SelectItem>
                <SelectItem value="size">Size</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        {/* Quick filters */}
        <div className="flex items-center gap-2 mt-3">
          {QUICK_FILTERS.map((f) => (
            <button
              key={f.label}
              onClick={() => setActiveFilter(activeFilter === f.label ? null : f.label)}
              className={`inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium transition-all ${
                activeFilter === f.label
                  ? "bg-primary-500/20 text-primary-300 border border-primary-500/30"
                  : "bg-white/5 text-secondary-400 border border-white/5 hover:bg-white/10 hover:text-secondary-200"
              }`}
            >
              <f.icon className="h-3 w-3" />
              {f.label}
            </button>
          ))}
        </div>
      </div>

      {/* File feed */}
      <div className="flex-1 overflow-y-auto px-6 py-4 scrollbar-thin">
        {isLoading && (
          <div className="space-y-3">
            {Array.from({ length: 8 }).map((_, i) => (
              <div key={i} className="flex items-start gap-3 rounded-xl border border-white/5 bg-white/[0.02] p-4">
                <Skeleton className="h-10 w-10 rounded-lg bg-white/5" />
                <div className="flex-1 space-y-2">
                  <Skeleton className="h-4 w-3/4 bg-white/5" />
                  <Skeleton className="h-3 w-1/2 bg-white/5" />
                  <div className="flex gap-2">
                    <Skeleton className="h-5 w-16 rounded-full bg-white/5" />
                    <Skeleton className="h-5 w-12 rounded-full bg-white/5" />
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}

        {isError && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <div className="rounded-full bg-red-500/10 p-4 mb-4">
              <AlertTriangle className="h-8 w-8 text-red-400" />
            </div>
            <h3 className="text-lg font-medium mb-1">Failed to load files</h3>
            <p className="text-sm text-secondary-400 max-w-sm">
              {error?.message ?? "An error occurred while fetching your files. Please try again."}
            </p>
          </div>
        )}

        {!isLoading && !isError && data && data.items.length === 0 && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <div className="rounded-full bg-white/5 p-4 mb-4 border border-white/10">
              <Inbox className="h-8 w-8 text-secondary-500" />
            </div>
            <h3 className="text-lg font-medium mb-1">No files found</h3>
            <p className="text-sm text-secondary-400 max-w-sm">
              {debouncedSearch
                ? `No results for "${debouncedSearch}". Try a different search term.`
                : "Your inbox is empty. Connect an email account to get started."}
            </p>
            <Button className="mt-4 bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold" size="sm">
              Connect Account
            </Button>
          </div>
        )}

        {!isLoading && data && data.items.length > 0 && (
          <div className="space-y-2">
            <div className="flex items-center justify-between mb-2">
              <p className="text-xs text-secondary-500">
                {data.totalCount.toLocaleString()} item{data.totalCount !== 1 ? "s" : ""}
                {debouncedSearch && <span> matching &quot;{debouncedSearch}&quot;</span>}
              </p>
              {isFetching && !isLoading && (
                <Badge className="text-xs bg-primary-500/10 text-primary-400 border-primary-500/20">Updating...</Badge>
              )}
            </div>
            {data.items.map((file) => (
              <FileCard key={file.id} file={file} />
            ))}
          </div>
        )}
      </div>

      {/* Pagination */}
      {data && data.totalCount > ITEMS_PER_PAGE && (
        <div className="flex items-center justify-between border-t border-white/5 bg-amex-card px-6 py-3">
          <p className="text-sm text-secondary-500">
            Page {currentPage}{totalPages > 0 && <span> of {totalPages}</span>}
          </p>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={handlePrevPage} disabled={cursorHistory.length === 0} className="border-white/10 text-secondary-300 hover:bg-white/5">
              <ChevronLeft className="h-4 w-4 mr-1" /> Previous
            </Button>
            <Button variant="outline" size="sm" onClick={handleNextPage} disabled={!data.hasMore} className="border-white/10 text-secondary-300 hover:bg-white/5">
              Next <ChevronRight className="h-4 w-4 ml-1" />
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
