"use client";

import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Sparkles,
  Send,
  Square,
  FileText,
  X,
  User,
  ScanText,
  Loader2,
  KeyRound,
} from "lucide-react";

import { api, ApiError, streamChat } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ENTITY_TYPES, type EntityType } from "@/lib/enums";
import type { ChatMessage, ExtractedEntity } from "@/types";
import { useToast } from "@/app/toast-provider";
import { PageHeader } from "@/components/page-header";
import { EmptyState } from "@/components/empty-state";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export default function AssistantPage() {
  const { data: status, isLoading } = useQuery({ queryKey: ["ai-status"], queryFn: api.aiStatus });

  return (
    <div className="space-y-6">
      <PageHeader
        title="AI Assistant"
        description={
          status?.configured && status.model
            ? `Grounded chat and extraction · ${status.model}`
            : "Grounded chat and entity extraction."
        }
      />

      {isLoading ? (
        <Skeleton className="h-96 w-full" />
      ) : !status?.configured ? (
        <EmptyState
          icon={KeyRound}
          title="AI is not configured"
          description="Add an ANTHROPIC_API_KEY to this instance's environment to enable chat and entity extraction, then restart the service."
        />
      ) : (
        <Tabs defaultValue="chat">
          <TabsList>
            <TabsTrigger value="chat">
              <Sparkles className="h-4 w-4" /> Chat
            </TabsTrigger>
            <TabsTrigger value="entities">
              <ScanText className="h-4 w-4" /> Entity extraction
            </TabsTrigger>
          </TabsList>
          <TabsContent value="chat">
            <ChatPanel />
          </TabsContent>
          <TabsContent value="entities">
            <EntityPanel />
          </TabsContent>
        </Tabs>
      )}
    </div>
  );
}

function ChatPanel() {
  const { toast } = useToast();
  const { data: documents } = useQuery({ queryKey: ["documents"], queryFn: api.documents });
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [contextDocId, setContextDocId] = useState<string>("");
  const abortRef = useRef<AbortController | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const contextDoc = documents?.find((d) => d.id === contextDocId);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [messages]);

  function stop() {
    abortRef.current?.abort();
    abortRef.current = null;
    setStreaming(false);
  }

  async function send() {
    const text = input.trim();
    if (!text || streaming) return;

    const next: ChatMessage[] = [...messages, { role: "user", content: text }];
    setMessages([...next, { role: "assistant", content: "" }]);
    setInput("");
    setStreaming(true);

    const controller = new AbortController();
    abortRef.current = controller;

    await streamChat(
      { messages: next, contextDocumentId: contextDocId || undefined },
      {
        onToken: (t) =>
          setMessages((prev) => {
            const copy = [...prev];
            const last = copy[copy.length - 1];
            if (last?.role === "assistant") copy[copy.length - 1] = { ...last, content: last.content + t };
            return copy;
          }),
        onError: (err: ApiError) => {
          setStreaming(false);
          abortRef.current = null;
          toast({ title: "Assistant error", description: err.message, variant: "error" });
          setMessages((prev) => {
            const copy = [...prev];
            const last = copy[copy.length - 1];
            if (last?.role === "assistant" && last.content === "") copy.pop();
            return copy;
          });
        },
        onDone: () => {
          setStreaming(false);
          abortRef.current = null;
        },
        signal: controller.signal,
      }
    );
  }

  return (
    <div className="flex h-[calc(100vh-16rem)] min-h-[420px] flex-col rounded-md border border-border bg-card">
      <div ref={scrollRef} className="flex-1 space-y-4 overflow-y-auto p-4">
        {messages.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <div className="text-center">
              <Sparkles className="mx-auto mb-2 h-6 w-6 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">
                Ask anything, or ground the conversation on a document below.
              </p>
            </div>
          </div>
        ) : (
          messages.map((m, i) => (
            <div key={i} className={cn("flex gap-3", m.role === "user" && "flex-row-reverse")}>
              <span
                className={cn(
                  "flex h-7 w-7 shrink-0 items-center justify-center rounded-md",
                  m.role === "user" ? "bg-secondary text-secondary-foreground" : "bg-primary/15 text-primary"
                )}
              >
                {m.role === "user" ? <User className="h-4 w-4" /> : <Sparkles className="h-4 w-4" />}
              </span>
              <div
                className={cn(
                  "max-w-[80%] whitespace-pre-wrap rounded-md px-3 py-2 text-sm leading-relaxed",
                  m.role === "user"
                    ? "bg-secondary text-secondary-foreground"
                    : "bg-muted text-foreground"
                )}
              >
                {m.content || (
                  <span className="inline-flex items-center gap-1 text-muted-foreground">
                    <span className="h-2 w-1.5 animate-caret-blink bg-current" />
                  </span>
                )}
              </div>
            </div>
          ))
        )}
      </div>

      <div className="border-t border-border p-3">
        {contextDoc && (
          <div className="mb-2 flex w-fit items-center gap-1.5 rounded-md border border-primary/30 bg-primary/10 px-2 py-1 text-xs text-primary">
            <FileText className="h-3.5 w-3.5" />
            <span className="max-w-[240px] truncate">Grounded on: {contextDoc.title}</span>
            <button onClick={() => setContextDocId("")} aria-label="Remove grounding">
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        )}
        <div className="flex items-end gap-2">
          <select
            value={contextDocId}
            onChange={(e) => setContextDocId(e.target.value)}
            className="h-9 max-w-[160px] shrink-0 rounded-md border border-border bg-surface px-2 text-xs text-foreground outline-none focus:border-ring"
            title="Ground on document"
          >
            <option value="">No grounding</option>
            {documents?.map((d) => (
              <option key={d.id} value={d.id}>
                {d.title}
              </option>
            ))}
          </select>
          <Textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder="Message the assistant…  (Enter to send, Shift+Enter for newline)"
            className="max-h-32 min-h-[38px] flex-1 resize-none py-2"
            rows={1}
          />
          {streaming ? (
            <Button variant="secondary" onClick={stop} title="Stop">
              <Square className="h-4 w-4" /> Stop
            </Button>
          ) : (
            <Button onClick={send} disabled={!input.trim()} title="Send">
              <Send className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

const ENTITY_STYLE: Record<EntityType, string> = {
  person: "border-info/25 bg-info/10 text-info",
  organization: "border-primary/25 bg-primary/10 text-primary",
  date: "border-warning/30 bg-warning/10 text-warning",
  location: "border-success/25 bg-success/10 text-success",
  amount: "border-destructive/25 bg-destructive/10 text-destructive",
};

function normalizeEntityType(raw: ExtractedEntity["type"]): EntityType {
  if (typeof raw === "number") return ENTITY_TYPES[raw] ?? "person";
  const lower = String(raw).toLowerCase();
  return (ENTITY_TYPES.find((t) => t === lower) as EntityType) ?? "person";
}

function EntityPanel() {
  const { toast } = useToast();
  const [text, setText] = useState("");
  const [entities, setEntities] = useState<ExtractedEntity[] | null>(null);
  const [loading, setLoading] = useState(false);

  async function extract() {
    if (!text.trim() || loading) return;
    setLoading(true);
    try {
      const result = await api.entities(text.trim());
      setEntities(result);
    } catch (err) {
      toast({ title: "Extraction failed", description: (err as ApiError).message, variant: "error" });
    } finally {
      setLoading(false);
    }
  }

  const grouped = ENTITY_TYPES.map((type) => ({
    type,
    items: (entities ?? []).filter((e) => normalizeEntityType(e.type) === type),
  })).filter((g) => g.items.length > 0);

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <div className="space-y-2">
        <Textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Paste text to extract people, organizations, dates, locations, and amounts…"
          className="min-h-[280px]"
        />
        <Button onClick={extract} disabled={!text.trim() || loading}>
          {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : <ScanText className="h-4 w-4" />}
          Extract entities
        </Button>
      </div>

      <div className="rounded-md border border-border bg-card p-4">
        {entities === null ? (
          <p className="pt-10 text-center text-sm text-muted-foreground">
            Extracted entities appear here, grouped by type.
          </p>
        ) : grouped.length === 0 ? (
          <p className="pt-10 text-center text-sm text-muted-foreground">No entities found.</p>
        ) : (
          <div className="space-y-4">
            {grouped.map((g) => (
              <div key={g.type}>
                <p className="mb-1.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {g.type} · {g.items.length}
                </p>
                <div className="flex flex-wrap gap-1.5">
                  {g.items.map((e, i) => (
                    <span
                      key={`${e.value}-${i}`}
                      className={cn("rounded-md border px-2 py-0.5 text-xs", ENTITY_STYLE[g.type])}
                    >
                      {e.value}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
