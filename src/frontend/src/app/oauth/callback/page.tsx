"use client";

import { Suspense, useEffect, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import { CheckCircle2, Loader2, XCircle } from "lucide-react";

import { api, ApiError } from "@/lib/api";

const STORAGE_KEY = "centra-oauth-provider";

type Phase = "working" | "done" | "error";

function CallbackInner() {
  const params = useSearchParams();
  const [phase, setPhase] = useState<Phase>("working");
  const [message, setMessage] = useState("Completing connection…");
  const ran = useRef(false);

  useEffect(() => {
    if (ran.current) return;
    ran.current = true;

    const providerError = params.get("error");
    const code = params.get("code");
    const state = params.get("state") ?? "";
    let provider = "";
    try {
      provider = window.localStorage.getItem(STORAGE_KEY) ?? "";
    } catch {
      /* ignore */
    }

    function finish(ok: boolean, msg: string) {
      setPhase(ok ? "done" : "error");
      setMessage(msg);
      try {
        window.localStorage.removeItem(STORAGE_KEY);
      } catch {
        /* ignore */
      }
      window.opener?.postMessage({ type: "centra-oauth", ok, message: msg }, window.location.origin);
      if (ok) setTimeout(() => window.close(), 900);
    }

    if (providerError) {
      finish(false, `Provider returned an error: ${providerError}`);
      return;
    }
    if (!code || !provider) {
      finish(false, "Missing authorization code or provider context.");
      return;
    }

    api
      .completeConnection(provider, { code, state })
      .then(() => finish(true, "Connection established. You can close this window."))
      .catch((err) => {
        const e = err as ApiError;
        finish(false, e.message || "Failed to complete the connection.");
      });
  }, [params]);

  const Icon = phase === "working" ? Loader2 : phase === "done" ? CheckCircle2 : XCircle;
  const tone =
    phase === "working"
      ? "text-muted-foreground"
      : phase === "done"
        ? "text-success"
        : "text-destructive";

  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background px-6 text-center">
      <Icon className={`h-8 w-8 ${tone} ${phase === "working" ? "animate-spin" : ""}`} />
      <p className="max-w-sm text-sm text-foreground">{message}</p>
      {phase === "error" && (
        <button
          onClick={() => window.close()}
          className="text-sm font-medium text-primary hover:underline"
        >
          Close window
        </button>
      )}
    </div>
  );
}

export default function OAuthCallbackPage() {
  return (
    <Suspense fallback={null}>
      <CallbackInner />
    </Suspense>
  );
}
