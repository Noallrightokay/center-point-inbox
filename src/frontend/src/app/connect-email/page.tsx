"use client";

import { useState, useEffect, Suspense } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import {
  Mail, ArrowLeft, ArrowRight, Eye, EyeOff, Loader2, CheckCircle, AlertTriangle,
  Server, Lock, Shield,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { post } from "@/lib/api";

interface ImapConfig {
  imapHost: string;
  imapPort: number;
  smtpHost: string;
  smtpPort: number;
  encryption: "ssl" | "tls" | "none";
}

const KNOWN_PROVIDERS: Record<string, ImapConfig & { name: string; icon: string; color: string }> = {
  "gmail.com": { name: "Gmail", icon: "G", color: "bg-red-500/20 text-red-400", imapHost: "imap.gmail.com", imapPort: 993, smtpHost: "smtp.gmail.com", smtpPort: 587, encryption: "tls" },
  "outlook.com": { name: "Outlook", icon: "O", color: "bg-blue-500/20 text-blue-400", imapHost: "outlook.office365.com", imapPort: 993, smtpHost: "smtp.office365.com", smtpPort: 587, encryption: "tls" },
  "hotmail.com": { name: "Outlook", icon: "O", color: "bg-blue-500/20 text-blue-400", imapHost: "outlook.office365.com", imapPort: 993, smtpHost: "smtp.office365.com", smtpPort: 587, encryption: "tls" },
  "yahoo.com": { name: "Yahoo Mail", icon: "Y", color: "bg-purple-500/20 text-purple-400", imapHost: "imap.mail.yahoo.com", imapPort: 993, smtpHost: "smtp.mail.yahoo.com", smtpPort: 587, encryption: "tls" },
  "icloud.com": { name: "iCloud", icon: "i", color: "bg-sky-500/20 text-sky-400", imapHost: "imap.mail.me.com", imapPort: 993, smtpHost: "smtp.mail.me.com", smtpPort: 587, encryption: "tls" },
  "me.com": { name: "iCloud", icon: "i", color: "bg-sky-500/20 text-sky-400", imapHost: "imap.mail.me.com", imapPort: 993, smtpHost: "smtp.mail.me.com", smtpPort: 587, encryption: "tls" },
  "protonmail.com": { name: "ProtonMail", icon: "P", color: "bg-violet-500/20 text-violet-400", imapHost: "127.0.0.1", imapPort: 1143, smtpHost: "127.0.0.1", smtpPort: 1025, encryption: "none" },
  "zoho.com": { name: "Zoho Mail", icon: "Z", color: "bg-green-500/20 text-green-400", imapHost: "imap.zoho.com", imapPort: 993, smtpHost: "smtp.zoho.com", smtpPort: 587, encryption: "tls" },
  "aol.com": { name: "AOL Mail", icon: "A", color: "bg-yellow-500/20 text-yellow-400", imapHost: "imap.aol.com", imapPort: 993, smtpHost: "smtp.aol.com", smtpPort: 587, encryption: "tls" },
};

function detectProvider(email: string) {
  const domain = email.split("@")[1]?.toLowerCase();
  return domain ? KNOWN_PROVIDERS[domain] || null : null;
}

function ConnectEmailForm() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const initialEmail = searchParams.get("email") || "";

  const [step, setStep] = useState<"email" | "config" | "connecting" | "success" | "error">("email");
  const [email, setEmail] = useState(initialEmail);
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [imapHost, setImapHost] = useState("");
  const [imapPort, setImapPort] = useState("993");
  const [smtpHost, setSmtpHost] = useState("");
  const [smtpPort, setSmtpPort] = useState("587");
  const [encryption, setEncryption] = useState<"ssl" | "tls" | "none">("tls");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [errorMessage, setErrorMessage] = useState("");

  const detectedProvider = detectProvider(email);

  useEffect(() => {
    if (initialEmail && detectProvider(initialEmail)) {
      setStep("config");
    }
  }, [initialEmail]);

  function handleEmailNext() {
    if (!email) return;
    const provider = detectProvider(email);
    if (provider) {
      setImapHost(provider.imapHost);
      setImapPort(String(provider.imapPort));
      setSmtpHost(provider.smtpHost);
      setSmtpPort(String(provider.smtpPort));
      setEncryption(provider.encryption);
    }
    setStep("config");
  }

  async function handleConnect() {
    setStep("connecting");
    try {
      await post("/email/connect", {
        email,
        password,
        imapHost: imapHost || detectedProvider?.imapHost,
        imapPort: parseInt(imapPort) || detectedProvider?.imapPort || 993,
        smtpHost: smtpHost || detectedProvider?.smtpHost,
        smtpPort: parseInt(smtpPort) || detectedProvider?.smtpPort || 587,
        encryption,
      });
      setStep("success");
      setTimeout(() => router.push("/dashboard"), 2000);
    } catch (err: unknown) {
      setErrorMessage(err instanceof Error ? err.message : "Connection failed. Check your credentials and try again.");
      setStep("error");
    }
  }

  return (
    <div className="min-h-screen bg-amex-dark flex items-center justify-center p-6">
      <div className="w-full max-w-lg">
        <button onClick={() => router.back()} className="flex items-center gap-2 text-sm text-secondary-400 hover:text-secondary-200 mb-8 transition-colors">
          <ArrowLeft className="h-4 w-4" /> Back
        </button>

        <div className="rounded-2xl border border-white/10 bg-amex-card p-8 shadow-amex-lg">
          {/* Header */}
          <div className="flex items-center gap-3 mb-6">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-amex-gold shadow-amex-glow">
              <Mail className="h-5 w-5 text-amex-black" />
            </div>
            <div>
              <h1 className="text-lg font-bold text-white">Connect Email</h1>
              <p className="text-xs text-secondary-500">Works with any IMAP email provider</p>
            </div>
          </div>

          {/* Step: Email */}
          {step === "email" && (
            <div className="space-y-4">
              <div>
                <label className="text-xs font-medium text-secondary-400 mb-1.5 block">Email Address</label>
                <Input value={email} onChange={(e) => setEmail(e.target.value)} type="email" placeholder="you@company.com"
                  onKeyDown={(e) => e.key === "Enter" && handleEmailNext()}
                  className="bg-white/5 border-white/10 focus:border-primary-500 h-11" />
              </div>
              {detectedProvider && (
                <div className="flex items-center gap-2 rounded-lg bg-green-500/10 border border-green-500/20 px-3 py-2">
                  <CheckCircle className="h-4 w-4 text-green-400" />
                  <span className="text-xs text-green-300">Detected: {detectedProvider.name} — settings will be auto-configured</span>
                </div>
              )}
              <Button onClick={handleEmailNext} disabled={!email} className="w-full bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold h-11">
                Continue <ArrowRight className="h-4 w-4 ml-2" />
              </Button>
            </div>
          )}

          {/* Step: Config */}
          {step === "config" && (
            <div className="space-y-4">
              <div className="flex items-center gap-2 rounded-lg bg-white/5 border border-white/5 px-3 py-2 mb-2">
                {detectedProvider && (
                  <div className={`flex h-7 w-7 items-center justify-center rounded-md text-xs font-bold ${detectedProvider.color}`}>
                    {detectedProvider.icon}
                  </div>
                )}
                <span className="text-sm text-secondary-200">{email}</span>
                <button onClick={() => setStep("email")} className="ml-auto text-xs text-primary-400 hover:text-primary-300">Change</button>
              </div>

              <div>
                <label className="text-xs font-medium text-secondary-400 mb-1.5 block">
                  {detectedProvider?.name === "Gmail" ? "App Password" : "Password"}
                </label>
                <div className="relative">
                  <Input type={showPassword ? "text" : "password"} value={password} onChange={(e) => setPassword(e.target.value)}
                    placeholder={detectedProvider?.name === "Gmail" ? "16-character app password" : "Your email password"}
                    className="bg-white/5 border-white/10 focus:border-primary-500 h-11 pr-10" />
                  <button onClick={() => setShowPassword(!showPassword)} className="absolute right-3 top-1/2 -translate-y-1/2 text-secondary-500 hover:text-secondary-300">
                    {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </button>
                </div>
                {detectedProvider?.name === "Gmail" && (
                  <p className="text-[11px] text-secondary-500 mt-1.5">
                    Gmail requires an App Password. Generate one in your Google Account settings.
                  </p>
                )}
              </div>

              <button onClick={() => setShowAdvanced(!showAdvanced)} className="text-xs text-primary-400 hover:text-primary-300 flex items-center gap-1">
                <Server className="h-3 w-3" /> {showAdvanced ? "Hide" : "Show"} server settings
              </button>

              {showAdvanced && (
                <div className="space-y-3 rounded-lg border border-white/5 bg-white/[0.02] p-4">
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="text-[10px] font-medium text-secondary-500 mb-1 block">IMAP Host</label>
                      <Input value={imapHost} onChange={(e) => setImapHost(e.target.value)} placeholder={detectedProvider?.imapHost || "imap.example.com"}
                        className="bg-white/5 border-white/10 text-xs h-9" />
                    </div>
                    <div>
                      <label className="text-[10px] font-medium text-secondary-500 mb-1 block">IMAP Port</label>
                      <Input value={imapPort} onChange={(e) => setImapPort(e.target.value)} placeholder="993"
                        className="bg-white/5 border-white/10 text-xs h-9" />
                    </div>
                    <div>
                      <label className="text-[10px] font-medium text-secondary-500 mb-1 block">SMTP Host</label>
                      <Input value={smtpHost} onChange={(e) => setSmtpHost(e.target.value)} placeholder={detectedProvider?.smtpHost || "smtp.example.com"}
                        className="bg-white/5 border-white/10 text-xs h-9" />
                    </div>
                    <div>
                      <label className="text-[10px] font-medium text-secondary-500 mb-1 block">SMTP Port</label>
                      <Input value={smtpPort} onChange={(e) => setSmtpPort(e.target.value)} placeholder="587"
                        className="bg-white/5 border-white/10 text-xs h-9" />
                    </div>
                  </div>
                  <div>
                    <label className="text-[10px] font-medium text-secondary-500 mb-1 block">Encryption</label>
                    <div className="flex gap-2">
                      {(["tls", "ssl", "none"] as const).map((e) => (
                        <button key={e} onClick={() => setEncryption(e)}
                          className={`flex-1 rounded-lg px-3 py-2 text-xs font-medium transition-all ${
                            encryption === e ? "bg-primary-500/20 text-primary-300 border border-primary-500/30" : "bg-white/5 text-secondary-400 border border-white/5 hover:bg-white/10"
                          }`}>{e.toUpperCase()}</button>
                      ))}
                    </div>
                  </div>
                </div>
              )}

              <div className="flex items-start gap-2 rounded-lg bg-white/[0.02] border border-white/5 px-3 py-2">
                <Lock className="h-4 w-4 text-primary-400 mt-0.5 shrink-0" />
                <p className="text-[11px] text-secondary-500">Your credentials are encrypted with AES-256 and never stored in plain text.</p>
              </div>

              <Button onClick={handleConnect} disabled={!password} className="w-full bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold h-11">
                Connect Account
              </Button>
            </div>
          )}

          {/* Step: Connecting */}
          {step === "connecting" && (
            <div className="flex flex-col items-center py-8 text-center">
              <Loader2 className="h-10 w-10 text-primary-400 animate-spin mb-4" />
              <h3 className="text-sm font-semibold text-white mb-1">Connecting...</h3>
              <p className="text-xs text-secondary-500">Verifying credentials and syncing your inbox</p>
            </div>
          )}

          {/* Step: Success */}
          {step === "success" && (
            <div className="flex flex-col items-center py-8 text-center">
              <div className="flex h-14 w-14 items-center justify-center rounded-full bg-green-500/15 border border-green-500/20 mb-4">
                <CheckCircle className="h-7 w-7 text-green-400" />
              </div>
              <h3 className="text-sm font-semibold text-white mb-1">Connected!</h3>
              <p className="text-xs text-secondary-500">Redirecting to your inbox...</p>
            </div>
          )}

          {/* Step: Error */}
          {step === "error" && (
            <div className="flex flex-col items-center py-8 text-center">
              <div className="flex h-14 w-14 items-center justify-center rounded-full bg-red-500/15 border border-red-500/20 mb-4">
                <AlertTriangle className="h-7 w-7 text-red-400" />
              </div>
              <h3 className="text-sm font-semibold text-white mb-1">Connection Failed</h3>
              <p className="text-xs text-secondary-400 mb-4 max-w-sm">{errorMessage}</p>
              <Button onClick={() => setStep("config")} variant="outline" className="border-white/10 text-secondary-300">
                Try Again
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default function ConnectEmailPage() {
  return (
    <Suspense fallback={
      <div className="min-h-screen bg-amex-dark flex items-center justify-center">
        <Loader2 className="h-8 w-8 text-primary-400 animate-spin" />
      </div>
    }>
      <ConnectEmailForm />
    </Suspense>
  );
}
