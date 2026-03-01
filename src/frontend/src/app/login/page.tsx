"use client";

import { useState, useEffect } from "react";
import { useRouter } from "next/navigation";
import { Mail, ArrowRight, Shield, Zap, Globe } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useAuthStore } from "@/stores/auth-store";

const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";
const GOOGLE_AUTH_URL = `${API_BASE_URL}/api/auth/oauth/google`;
const MICROSOFT_AUTH_URL = `${API_BASE_URL}/api/auth/oauth/microsoft`;

function GoogleIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none">
      <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1Z" fill="#4285F4"/>
      <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23Z" fill="#34A853"/>
      <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18A10.96 10.96 0 0 0 1 12c0 1.77.42 3.45 1.18 4.93l3.66-2.84Z" fill="#FBBC05"/>
      <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53Z" fill="#EA4335"/>
    </svg>
  );
}

function MicrosoftIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none">
      <rect x="1" y="1" width="10" height="10" fill="#F25022"/>
      <rect x="13" y="1" width="10" height="10" fill="#7FBA00"/>
      <rect x="1" y="13" width="10" height="10" fill="#00A4EF"/>
      <rect x="13" y="13" width="10" height="10" fill="#FFB900"/>
    </svg>
  );
}

export default function LoginPage() {
  const router = useRouter();
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated);
  const [showImap, setShowImap] = useState(false);
  const [imapEmail, setImapEmail] = useState("");

  useEffect(() => {
    if (isAuthenticated) {
      router.replace("/dashboard");
    }
  }, [isAuthenticated, router]);

  function handleGoogleLogin() {
    const redirectUri = `${window.location.origin}/login/callback`;
    const params = new URLSearchParams({ redirect_uri: redirectUri, provider: "google" });
    window.location.href = `${GOOGLE_AUTH_URL}?${params.toString()}`;
  }

  function handleMicrosoftLogin() {
    const redirectUri = `${window.location.origin}/login/callback`;
    const params = new URLSearchParams({ redirect_uri: redirectUri, provider: "microsoft" });
    window.location.href = `${MICROSOFT_AUTH_URL}?${params.toString()}`;
  }

  function handleImapConnect() {
    if (!imapEmail) return;
    router.push(`/connect-email?email=${encodeURIComponent(imapEmail)}`);
  }

  return (
    <div className="flex min-h-screen bg-amex-dark">
      {/* Left Panel — Branding */}
      <div className="hidden lg:flex lg:w-1/2 flex-col justify-between p-12 relative overflow-hidden">
        <div className="absolute inset-0 bg-amex-gradient opacity-80" />
        <div className="absolute top-0 right-0 w-96 h-96 bg-amex-gold opacity-5 rounded-full blur-3xl -translate-y-1/2 translate-x-1/2" />
        <div className="absolute bottom-0 left-0 w-72 h-72 bg-primary-500 opacity-10 rounded-full blur-3xl translate-y-1/2 -translate-x-1/2" />

        <div className="relative z-10">
          <div className="flex items-center gap-3 mb-2">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-amex-gold">
              <Mail className="h-5 w-5 text-amex-black" />
            </div>
            <span className="text-lg font-semibold tracking-tight text-white">Center Point</span>
          </div>
        </div>

        <div className="relative z-10 space-y-8">
          <div>
            <h1 className="text-5xl font-display font-bold text-white leading-tight tracking-tight">
              Your inbox,<br />
              <span className="gold-text">elevated.</span>
            </h1>
            <p className="mt-4 text-lg text-secondary-300 max-w-md leading-relaxed">
              A premium unified inbox that connects every email account. AI-powered intelligence. Enterprise-grade security. Zero compromise.
            </p>
          </div>

          <div className="grid grid-cols-1 gap-4">
            {[
              { icon: Globe, title: "Any Email, One Inbox", desc: "Gmail, Outlook, Yahoo, iCloud, or any IMAP — all in one place" },
              { icon: Zap, title: "AI-Powered Intelligence", desc: "Smart search, instant summaries, and translation across 100+ languages" },
              { icon: Shield, title: "Enterprise Security", desc: "DLP scanning, PHI detection, full audit trail, SOC 2 ready" },
            ].map((feature) => (
              <div key={feature.title} className="flex items-start gap-3 rounded-xl border border-white/10 bg-white/5 p-4 backdrop-blur-sm">
                <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-primary-500/20">
                  <feature.icon className="h-4 w-4 text-primary-300" />
                </div>
                <div>
                  <p className="text-sm font-semibold text-white">{feature.title}</p>
                  <p className="text-xs text-secondary-400 mt-0.5">{feature.desc}</p>
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="relative z-10">
          <p className="text-xs text-secondary-500">Trusted by enterprises worldwide</p>
        </div>
      </div>

      {/* Right Panel — Auth */}
      <div className="flex flex-1 flex-col items-center justify-center p-6 sm:p-12">
        <div className="w-full max-w-md space-y-8">
          <div className="lg:hidden flex items-center gap-3 justify-center mb-4">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-amex-gold">
              <Mail className="h-5 w-5 text-amex-black" />
            </div>
            <span className="text-lg font-semibold tracking-tight">Center Point</span>
          </div>

          <div className="text-center lg:text-left">
            <h2 className="text-2xl font-bold tracking-tight">Sign in to your inbox</h2>
            <p className="mt-2 text-sm text-secondary-400">Connect with your email provider to get started</p>
          </div>

          <div className="space-y-3">
            <button onClick={handleGoogleLogin} className="relative flex w-full items-center gap-3 rounded-xl border border-white/10 bg-white/5 px-4 py-3.5 text-sm font-medium transition-all hover:bg-white/10 hover:border-primary-500/30 hover:shadow-amex-glow">
              <GoogleIcon className="h-5 w-5" />
              <span>Continue with Google</span>
              <ArrowRight className="ml-auto h-4 w-4 text-secondary-500" />
            </button>

            <button onClick={handleMicrosoftLogin} className="relative flex w-full items-center gap-3 rounded-xl border border-white/10 bg-white/5 px-4 py-3.5 text-sm font-medium transition-all hover:bg-white/10 hover:border-primary-500/30 hover:shadow-amex-glow">
              <MicrosoftIcon className="h-5 w-5" />
              <span>Continue with Microsoft</span>
              <ArrowRight className="ml-auto h-4 w-4 text-secondary-500" />
            </button>

            <div className="relative py-2">
              <div className="absolute inset-0 flex items-center">
                <div className="w-full border-t border-white/10" />
              </div>
              <div className="relative flex justify-center">
                <span className="bg-[hsl(230,25%,7%)] px-3 text-xs text-secondary-500">or connect any email</span>
              </div>
            </div>

            {!showImap ? (
              <button onClick={() => setShowImap(true)} className="relative flex w-full items-center gap-3 rounded-xl border border-primary-500/30 bg-primary-500/10 px-4 py-3.5 text-sm font-medium transition-all hover:bg-primary-500/20 hover:shadow-amex-glow">
                <Mail className="h-5 w-5 text-primary-400" />
                <span className="text-primary-300">Connect any email account</span>
                <ArrowRight className="ml-auto h-4 w-4 text-primary-500" />
              </button>
            ) : (
              <div className="space-y-3 animate-slide-in-up">
                <div className="rounded-xl border border-primary-500/30 bg-primary-500/5 p-4">
                  <p className="text-xs text-primary-400 mb-3 font-medium">Enter your email to auto-detect settings</p>
                  <div className="flex gap-2">
                    <Input
                      type="email"
                      placeholder="you@company.com"
                      value={imapEmail}
                      onChange={(e) => setImapEmail(e.target.value)}
                      onKeyDown={(e) => e.key === "Enter" && handleImapConnect()}
                      className="flex-1 bg-white/5 border-white/10 focus:border-primary-500 text-sm"
                    />
                    <Button onClick={handleImapConnect} disabled={!imapEmail} className="bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold">
                      Connect
                    </Button>
                  </div>
                  <div className="flex flex-wrap gap-2 mt-3">
                    {["Yahoo", "iCloud", "ProtonMail", "Zoho", "AOL", "Custom IMAP"].map((provider) => (
                      <span key={provider} className="inline-flex items-center rounded-md bg-white/5 px-2 py-0.5 text-[10px] text-secondary-400 border border-white/5">
                        {provider}
                      </span>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </div>

          <p className="text-center text-[11px] text-secondary-500 leading-relaxed">
            By signing in, you agree to our Terms of Service and acknowledge that your data will be processed in accordance with our Privacy Policy. HIPAA and SOC 2 compliant.
          </p>
        </div>
      </div>
    </div>
  );
}
