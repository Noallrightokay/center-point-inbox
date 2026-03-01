"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { Suspense } from "react";
import { AlertCircle, Inbox } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { useAuthStore } from "@/stores/auth-store";

// ---------------------------------------------------------------------------
// Inner component that uses useSearchParams (must be inside Suspense)
// ---------------------------------------------------------------------------

function CallbackHandler() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const login = useAuthStore((s) => s.login);

  const [error, setError] = useState<string | null>(null);

  // Prevent double-execution in React 18 Strict Mode
  const hasRun = useRef(false);

  useEffect(() => {
    if (hasRun.current) return;
    hasRun.current = true;

    const code = searchParams.get("code");
    const state = searchParams.get("state");
    const errorParam = searchParams.get("error");
    const errorDescription = searchParams.get("error_description");

    // Handle explicit OAuth error
    if (errorParam) {
      setError(
        errorDescription ?? `OAuth error: ${errorParam}`,
      );
      return;
    }

    if (!code) {
      setError(
        "No authorization code received. Please try signing in again.",
      );
      return;
    }

    // Determine provider from state parameter or fallback
    let provider: "google" | "microsoft" = "google";
    if (state) {
      try {
        const decoded = JSON.parse(atob(state));
        if (decoded.provider === "microsoft") {
          provider = "microsoft";
        }
      } catch {
        // If state is not JSON-encoded, check the raw value
        if (state.includes("microsoft")) {
          provider = "microsoft";
        }
      }
    }

    const redirectUri = `${window.location.origin}/login/callback`;

    async function exchangeCode() {
      try {
        await login(provider, code!, redirectUri);
        router.replace("/dashboard");
      } catch (err) {
        const message =
          err instanceof Error ? err.message : "Authentication failed";
        setError(message);
      }
    }

    exchangeCode();
  }, [searchParams, login, router]);

  // Error state
  if (error) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-background px-4">
        <Card className="w-full max-w-sm shadow-lg">
          <CardHeader className="text-center">
            <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-red-100 text-red-600">
              <AlertCircle className="h-6 w-6" />
            </div>
            <CardTitle className="text-lg">Authentication Failed</CardTitle>
            <CardDescription className="text-sm">{error}</CardDescription>
          </CardHeader>
          <CardContent className="flex justify-center">
            <Button onClick={() => router.replace("/login")}>
              Back to Sign In
            </Button>
          </CardContent>
        </Card>
      </div>
    );
  }

  // Loading state
  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4">
      <Card className="w-full max-w-sm shadow-lg">
        <CardHeader className="text-center">
          <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-xl bg-primary text-primary-foreground">
            <Inbox className="h-6 w-6" />
          </div>
          <CardTitle className="text-lg">Signing you in...</CardTitle>
          <CardDescription className="text-sm">
            Please wait while we complete the authentication process.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex justify-center pb-8">
          <Spinner size={32} className="text-primary" />
        </CardContent>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Page component (wraps handler in Suspense for useSearchParams)
// ---------------------------------------------------------------------------

export default function OAuthCallbackPage() {
  return (
    <Suspense
      fallback={
        <div className="flex min-h-screen items-center justify-center">
          <Spinner size={32} />
        </div>
      }
    >
      <CallbackHandler />
    </Suspense>
  );
}
