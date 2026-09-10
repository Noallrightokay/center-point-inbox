"use client";

import { Suspense, useEffect } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Search, Languages, ShieldCheck } from "lucide-react";

import { useAuthStore } from "@/stores/auth-store";
import { AuthForm } from "@/components/auth/auth-form";

const HIGHLIGHTS = [
  { icon: Search, text: "One search across every mailbox, drive, and document you connect." },
  { icon: Languages, text: "Translate and convert formats in place — no copy-paste, no exports." },
  { icon: ShieldCheck, text: "Tamper-evident audit trail and DLP, self-hosted on your terms." },
];

function LoginInner() {
  const router = useRouter();
  const params = useSearchParams();
  const phase = useAuthStore((s) => s.phase);
  const bootstrap = useAuthStore((s) => s.bootstrap);
  const mode = params.get("mode") === "register" ? "register" : "login";

  useEffect(() => {
    if (phase === "loading") void bootstrap();
  }, [phase, bootstrap]);

  useEffect(() => {
    if (phase === "authed") router.replace("/");
  }, [phase, router]);

  return (
    <div className="grid min-h-screen lg:grid-cols-2">
      {/* Brand panel */}
      <div className="relative hidden flex-col justify-between bg-rail p-12 text-rail-foreground lg:flex">
        <div className="flex items-center gap-2.5">
          <span className="flex h-9 w-9 items-center justify-center rounded-md bg-primary font-semibold text-primary-foreground">
            C
          </span>
          <span className="text-lg font-semibold text-white">Centra</span>
        </div>

        <div className="max-w-md space-y-8">
          <h1 className="text-balance text-3xl font-semibold leading-tight text-white">
            Everything you receive, in one calm place.
          </h1>
          <ul className="space-y-4">
            {HIGHLIGHTS.map(({ icon: Icon, text }) => (
              <li key={text} className="flex items-start gap-3">
                <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-white/5 text-primary">
                  <Icon className="h-4 w-4" />
                </span>
                <span className="text-sm leading-relaxed text-rail-foreground">{text}</span>
              </li>
            ))}
          </ul>
        </div>

        <p className="text-xs text-rail-foreground/70">
          Self-hosted universal email &amp; document management.
        </p>
      </div>

      {/* Form panel */}
      <div className="flex items-center justify-center px-6 py-12">
        <div className="w-full max-w-sm">
          <div className="mb-8 lg:hidden">
            <span className="flex h-9 w-9 items-center justify-center rounded-md bg-primary font-semibold text-primary-foreground">
              C
            </span>
          </div>
          <h2 className="text-xl font-semibold text-foreground">
            {mode === "register" ? "Create your account" : "Sign in to Centra"}
          </h2>
          <p className="mb-6 mt-1 text-sm text-muted-foreground">
            {mode === "register"
              ? "The first account becomes the instance administrator."
              : "Welcome back. Enter your credentials to continue."}
          </p>

          <AuthForm mode={mode} onAuthed={() => router.replace("/")} />

          <p className="mt-8 text-center text-xs text-muted-foreground">
            Setting up a new instance?{" "}
            <Link href="/setup" className="font-medium text-primary hover:underline">
              View setup status
            </Link>
          </p>
        </div>
      </div>
    </div>
  );
}

export default function LoginPage() {
  return (
    <Suspense fallback={null}>
      <LoginInner />
    </Suspense>
  );
}
