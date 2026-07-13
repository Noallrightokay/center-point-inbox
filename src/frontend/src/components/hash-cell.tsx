"use client";

import { useState } from "react";
import { Check, Copy } from "lucide-react";

import { cn } from "@/lib/utils";
import { copyToClipboard, truncateMiddle } from "@/lib/utils";

export function HashCell({ hash, className }: { hash: string; className?: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    if (await copyToClipboard(hash)) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    }
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      title={hash}
      className={cn(
        "group inline-flex items-center gap-1.5 font-mono text-xs text-muted-foreground transition-colors hover:text-foreground",
        className
      )}
    >
      <span>{truncateMiddle(hash, 6)}</span>
      {copied ? (
        <Check className="h-3 w-3 text-success" />
      ) : (
        <Copy className="h-3 w-3 opacity-0 transition-opacity group-hover:opacity-100" />
      )}
    </button>
  );
}
