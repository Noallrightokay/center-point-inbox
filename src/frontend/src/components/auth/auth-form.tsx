"use client";

import { useState } from "react";
import { Eye, EyeOff, Loader2, ShieldCheck } from "lucide-react";

import { api, ApiError } from "@/lib/api";
import { parseRole } from "@/lib/enums";
import { useAuthStore } from "@/stores/auth-store";
import type { AuthResponse } from "@/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { MIN_PASSWORD, PasswordStrength } from "./password-strength";

type Mode = "login" | "register";

export function AuthForm({
  mode: controlledMode,
  allowModeSwitch = true,
  onAuthed,
}: {
  mode?: Mode;
  allowModeSwitch?: boolean;
  onAuthed: (res: AuthResponse) => void;
}) {
  const setSession = useAuthStore((s) => s.setSession);
  const [mode, setMode] = useState<Mode>(controlledMode ?? "login");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [showPw, setShowPw] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [adminRes, setAdminRes] = useState<AuthResponse | null>(null);

  const isRegister = mode === "register";
  const pwTooShort = isRegister && password.length > 0 && password.length < MIN_PASSWORD;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    if (isRegister && password.length < MIN_PASSWORD) {
      setError(`Password must be at least ${MIN_PASSWORD} characters.`);
      return;
    }
    setSubmitting(true);
    try {
      const res = isRegister
        ? await api.register({ email: email.trim(), displayName: displayName.trim(), password })
        : await api.login({ email: email.trim(), password });

      if (isRegister && parseRole(res.user.role) === "Admin") {
        // Persist the session but pause on the admin callout before continuing.
        setSession(res);
        setAdminRes(res);
        return;
      }
      setSession(res);
      onAuthed(res);
    } catch (err) {
      const e2 = err as ApiError;
      if (e2.status === 401) {
        setError("Incorrect email or password.");
      } else if (e2.status === 409) {
        setError("An account with that email already exists.");
      } else {
        setError(e2.message || "Something went wrong. Please try again.");
      }
      if (e2.traceId) console.error(`[centra] auth failed traceId=${e2.traceId}`);
    } finally {
      setSubmitting(false);
    }
  }

  if (adminRes) {
    return (
      <div className="space-y-5">
        <div className="flex items-start gap-3 rounded-md border border-primary/30 bg-primary/10 p-4">
          <ShieldCheck className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
          <div>
            <p className="text-sm font-medium text-foreground">
              You are the administrator of this instance
            </p>
            <p className="mt-1 text-sm text-muted-foreground">
              This first account has full administrative access — connections, compliance,
              and instance metrics are all yours to configure.
            </p>
          </div>
        </div>
        <Button className="w-full" onClick={() => onAuthed(adminRes)}>
          Continue to Centra
        </Button>
      </div>
    );
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4" noValidate>
      {isRegister && (
        <div className="space-y-1.5">
          <Label htmlFor="displayName">Display name</Label>
          <Input
            id="displayName"
            autoComplete="name"
            required
            value={displayName}
            onChange={(e) => setDisplayName(e.target.value)}
            placeholder="Ada Lovelace"
          />
        </div>
      )}

      <div className="space-y-1.5">
        <Label htmlFor="email">Email</Label>
        <Input
          id="email"
          type="email"
          autoComplete="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder="you@company.com"
        />
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="password">Password</Label>
        <div className="relative">
          <Input
            id="password"
            type={showPw ? "text" : "password"}
            autoComplete={isRegister ? "new-password" : "current-password"}
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="pr-9"
            aria-invalid={pwTooShort}
          />
          <button
            type="button"
            onClick={() => setShowPw((v) => !v)}
            className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            aria-label={showPw ? "Hide password" : "Show password"}
            tabIndex={-1}
          >
            {showPw ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
          </button>
        </div>
        {isRegister && password.length > 0 && <PasswordStrength password={password} />}
      </div>

      {error && (
        <p className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </p>
      )}

      <Button type="submit" className="w-full" disabled={submitting}>
        {submitting && <Loader2 className="h-4 w-4 animate-spin" />}
        {isRegister ? "Create account" : "Sign in"}
      </Button>

      {allowModeSwitch && (
        <p className="text-center text-sm text-muted-foreground">
          {isRegister ? "Already have an account?" : "Need an account?"}{" "}
          <button
            type="button"
            className="font-medium text-primary hover:underline"
            onClick={() => {
              setMode(isRegister ? "login" : "register");
              setError(null);
            }}
          >
            {isRegister ? "Sign in" : "Register"}
          </button>
        </p>
      )}
    </form>
  );
}
