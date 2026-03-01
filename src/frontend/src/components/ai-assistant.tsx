"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { Bot, Send, FileText, Loader2, Sparkles } from "lucide-react";
import {
  Sheet, SheetContent, SheetHeader, SheetTitle, SheetDescription,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Badge } from "@/components/ui/badge";
import { useAiAsk } from "@/hooks/use-ai";
import type { AiQueryResponse, AiSourceReference } from "@/types";

interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  sources?: AiSourceReference[];
  timestamp: Date;
}

interface AiAssistantProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const SUGGESTIONS = [
  "Summarize my unread emails",
  "What files were shared with me this week?",
  "Find documents with compliance issues",
  "Draft a reply to the latest email",
];

export function AiAssistant({ open, onOpenChange }: AiAssistantProps) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [inputValue, setInputValue] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const aiAsk = useAiAsk();

  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
  }, [messages, aiAsk.isPending]);

  useEffect(() => {
    if (open) setTimeout(() => inputRef.current?.focus(), 100);
  }, [open]);

  const handleSend = useCallback((query?: string) => {
    const text = query ?? inputValue.trim();
    if (!text) return;

    const userMessage: ChatMessage = {
      id: crypto.randomUUID(), role: "user", content: text, timestamp: new Date(),
    };
    setMessages((prev) => [...prev, userMessage]);
    setInputValue("");

    aiAsk.mutate({ query: text }, {
      onSuccess: (response: AiQueryResponse) => {
        setMessages((prev) => [...prev, {
          id: response.id, role: "assistant", content: response.response,
          sources: response.sourceReferences, timestamp: new Date(response.createdAt),
        }]);
      },
      onError: (error: Error) => {
        setMessages((prev) => [...prev, {
          id: crypto.randomUUID(), role: "assistant",
          content: `I encountered an error: ${error.message}. Please try again.`,
          timestamp: new Date(),
        }]);
      },
    });
  }, [inputValue, aiAsk]);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="flex w-full flex-col p-0 sm:max-w-lg bg-amex-charcoal border-l border-white/10">
        {/* Header */}
        <SheetHeader className="border-b border-white/5 px-6 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-amex-gold shadow-amex-glow">
                <Sparkles className="h-4 w-4 text-amex-black" />
              </div>
              <div>
                <SheetTitle className="text-base text-white">AI Assistant</SheetTitle>
                <SheetDescription className="text-xs text-secondary-500">
                  Powered by advanced AI
                </SheetDescription>
              </div>
            </div>
            {messages.length > 0 && (
              <Button variant="ghost" size="sm" onClick={() => setMessages([])} className="text-xs text-secondary-500 hover:text-secondary-300">
                Clear
              </Button>
            )}
          </div>
        </SheetHeader>

        {/* Messages */}
        <ScrollArea className="flex-1 px-6" ref={scrollRef}>
          <div className="py-4 space-y-4">
            {messages.length === 0 && (
              <div className="flex flex-col items-center justify-center py-12 text-center">
                <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-primary-500/10 border border-primary-500/20 mb-4">
                  <Bot className="h-8 w-8 text-primary-400" />
                </div>
                <h3 className="text-sm font-semibold mb-1 text-white">How can I help?</h3>
                <p className="text-xs text-secondary-500 mb-6 max-w-[280px]">
                  I can search, summarize, translate, and draft responses across all your connected accounts.
                </p>
                <div className="w-full space-y-2">
                  {SUGGESTIONS.map((q) => (
                    <button key={q} onClick={() => handleSend(q)}
                      className="w-full rounded-xl border border-white/5 bg-white/[0.02] px-4 py-3 text-left text-xs text-secondary-300 transition-all hover:bg-white/5 hover:border-primary-500/20 hover:text-primary-300">
                      {q}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {messages.map((msg) => (
              <div key={msg.id} className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
                <div className={`max-w-[85%] rounded-xl px-4 py-3 text-sm ${
                  msg.role === "user"
                    ? "bg-amex-gold text-amex-black font-medium"
                    : "bg-white/5 border border-white/5 text-secondary-200"
                }`}>
                  <p className="whitespace-pre-wrap">{msg.content}</p>
                  {msg.sources && msg.sources.length > 0 && (
                    <div className="mt-3 pt-2 border-t border-white/10 space-y-1">
                      <p className="text-[10px] font-semibold text-secondary-500 uppercase tracking-wider">Sources</p>
                      {msg.sources.map((src) => (
                        <a key={src.fileId} href={`/file/${src.fileId}`}
                          className="flex items-center gap-2 rounded-lg px-2 py-1.5 text-xs transition-colors hover:bg-white/5"
                          onClick={() => onOpenChange(false)}>
                          <FileText className="h-3 w-3 text-primary-400 shrink-0" />
                          <span className="truncate text-secondary-300">{src.fileTitle}</span>
                          <Badge className="ml-auto text-[9px] px-1 py-0 bg-primary-500/10 text-primary-400 border-primary-500/20">
                            {Math.round(src.relevanceScore * 100)}%
                          </Badge>
                        </a>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            ))}

            {aiAsk.isPending && (
              <div className="flex justify-start">
                <div className="flex items-center gap-2 rounded-xl bg-white/5 border border-white/5 px-4 py-3">
                  <div className="flex gap-1">
                    <div className="h-2 w-2 rounded-full bg-primary-500 animate-bounce" style={{ animationDelay: "0ms" }} />
                    <div className="h-2 w-2 rounded-full bg-primary-500 animate-bounce" style={{ animationDelay: "150ms" }} />
                    <div className="h-2 w-2 rounded-full bg-primary-500 animate-bounce" style={{ animationDelay: "300ms" }} />
                  </div>
                </div>
              </div>
            )}
          </div>
        </ScrollArea>

        {/* Input */}
        <div className="border-t border-white/5 px-6 py-4">
          <div className="flex items-center gap-2">
            <Input ref={inputRef} value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); handleSend(); } }}
              placeholder="Ask anything..."
              disabled={aiAsk.isPending}
              className="flex-1 bg-white/5 border-white/10 focus:border-primary-500 text-sm placeholder:text-secondary-500"
            />
            <Button size="icon" onClick={() => handleSend()} disabled={!inputValue.trim() || aiAsk.isPending}
              className="bg-amex-gold hover:bg-amex-gold/90 text-amex-black">
              {aiAsk.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Send className="h-4 w-4" />}
            </Button>
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}
