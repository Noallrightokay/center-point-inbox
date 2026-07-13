"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { Loader2 } from "lucide-react";

import { useAuthStore } from "@/stores/auth-store";
import { AppShell } from "@/components/app-shell";
import { CommandPalette } from "@/components/command-palette";

export default function AppLayout({ children }: { children: React.ReactNode }) {
  const router = useRouter();
  const phase = useAuthStore((s) => s.phase);
  const bootstrap = useAuthStore((s) => s.bootstrap);

  useEffect(() => {
    if (phase === "loading") void bootstrap();
  }, [phase, bootstrap]);

  useEffect(() => {
    if (phase === "anon") router.replace("/login");
  }, [phase, router]);

  if (phase !== "authed") {
    return (
      <div className="flex min-h-screen items-center justify-center bg-background">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <>
      <AppShell>{children}</AppShell>
      <CommandPalette />
    </>
  );
}
