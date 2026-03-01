"use client";

import { useState } from "react";
import {
  Mail, Plus, Trash2, Shield, Globe, Bell, Palette, User,
  CheckCircle, AlertCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { useAuthStore } from "@/stores/auth-store";

interface ConnectedAccount {
  id: string;
  email: string;
  provider: string;
  icon: string;
  color: string;
  status: "active" | "error";
  lastSync: string;
}

const MOCK_ACCOUNTS: ConnectedAccount[] = [
  { id: "1", email: "user@gmail.com", provider: "Gmail", icon: "G", color: "bg-red-500/20 text-red-400", status: "active", lastSync: "2 min ago" },
  { id: "2", email: "user@outlook.com", provider: "Outlook", icon: "O", color: "bg-blue-500/20 text-blue-400", status: "active", lastSync: "5 min ago" },
];

type SettingsTab = "accounts" | "security" | "notifications" | "appearance";

export default function SettingsPage() {
  const { user } = useAuthStore();
  const [activeTab, setActiveTab] = useState<SettingsTab>("accounts");
  const [accounts] = useState<ConnectedAccount[]>(MOCK_ACCOUNTS);

  const tabs = [
    { id: "accounts" as const, label: "Accounts", icon: Mail },
    { id: "security" as const, label: "Security", icon: Shield },
    { id: "notifications" as const, label: "Notifications", icon: Bell },
    { id: "appearance" as const, label: "Appearance", icon: Palette },
  ];

  return (
    <div className="max-w-4xl mx-auto px-6 py-8">
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-white">Settings</h1>
        <p className="text-sm text-secondary-400 mt-1">Manage your account and preferences</p>
      </div>

      <div className="flex gap-8">
        {/* Sidebar tabs */}
        <div className="w-48 shrink-0">
          <nav className="space-y-1">
            {tabs.map((tab) => (
              <button key={tab.id} onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2.5 w-full rounded-lg px-3 py-2.5 text-sm transition-all ${
                  activeTab === tab.id
                    ? "bg-primary-500/15 text-primary-300 font-medium border border-primary-500/20"
                    : "text-secondary-400 hover:bg-white/5 hover:text-secondary-200"
                }`}>
                <tab.icon className="h-4 w-4" />
                {tab.label}
              </button>
            ))}
          </nav>
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          {/* Accounts Tab */}
          {activeTab === "accounts" && (
            <div className="space-y-6">
              <div className="flex items-center justify-between">
                <div>
                  <h2 className="text-lg font-semibold text-white">Connected Accounts</h2>
                  <p className="text-xs text-secondary-500 mt-0.5">Manage your email accounts and cloud storage</p>
                </div>
                <Button asChild className="bg-amex-gold hover:bg-amex-gold/90 text-amex-black font-semibold gap-2" size="sm">
                  <a href="/connect-email">
                    <Plus className="h-4 w-4" /> Add Account
                  </a>
                </Button>
              </div>

              <div className="space-y-3">
                {accounts.map((account) => (
                  <div key={account.id} className="flex items-center gap-3 rounded-xl border border-white/5 bg-white/[0.02] p-4 hover:bg-white/[0.04] transition-colors">
                    <div className={`flex h-10 w-10 items-center justify-center rounded-lg text-sm font-bold ${account.color}`}>
                      {account.icon}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <p className="text-sm font-medium text-white truncate">{account.email}</p>
                        {account.status === "active" ? (
                          <Badge className="text-[10px] bg-green-500/10 text-green-400 border-green-500/20">Active</Badge>
                        ) : (
                          <Badge className="text-[10px] bg-red-500/10 text-red-400 border-red-500/20">Error</Badge>
                        )}
                      </div>
                      <p className="text-xs text-secondary-500 mt-0.5">{account.provider} — Last sync: {account.lastSync}</p>
                    </div>
                    <Button variant="ghost" size="icon" className="text-secondary-500 hover:text-red-400">
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Security Tab */}
          {activeTab === "security" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-white">Security</h2>
                <p className="text-xs text-secondary-500 mt-0.5">Manage security and compliance settings</p>
              </div>

              {[
                { title: "DLP Scanning", desc: "Automatically scan files for sensitive data (SSN, credit cards, PHI)", enabled: true },
                { title: "PHI Detection", desc: "Flag files containing protected health information", enabled: true },
                { title: "Audit Logging", desc: "Track all user actions for compliance", enabled: true },
                { title: "Two-Factor Auth", desc: "Require 2FA for all team members", enabled: false },
              ].map((setting) => (
                <div key={setting.title} className="flex items-center justify-between rounded-xl border border-white/5 bg-white/[0.02] p-4">
                  <div>
                    <p className="text-sm font-medium text-white">{setting.title}</p>
                    <p className="text-xs text-secondary-500 mt-0.5">{setting.desc}</p>
                  </div>
                  <button className={`relative h-6 w-11 rounded-full transition-colors ${setting.enabled ? "bg-primary-500" : "bg-white/10"}`}>
                    <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow-md transition-transform ${setting.enabled ? "left-[22px]" : "left-0.5"}`} />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* Notifications Tab */}
          {activeTab === "notifications" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-white">Notifications</h2>
                <p className="text-xs text-secondary-500 mt-0.5">Control how you receive notifications</p>
              </div>

              {[
                { title: "New emails", desc: "Get notified when new emails arrive" },
                { title: "DLP alerts", desc: "Critical security violations" },
                { title: "Translation complete", desc: "When document translations finish" },
                { title: "Weekly digest", desc: "Summary of inbox activity" },
              ].map((n) => (
                <div key={n.title} className="flex items-center justify-between rounded-xl border border-white/5 bg-white/[0.02] p-4">
                  <div>
                    <p className="text-sm font-medium text-white">{n.title}</p>
                    <p className="text-xs text-secondary-500 mt-0.5">{n.desc}</p>
                  </div>
                  <button className="relative h-6 w-11 rounded-full bg-primary-500 transition-colors">
                    <span className="absolute top-0.5 left-[22px] h-5 w-5 rounded-full bg-white shadow-md transition-transform" />
                  </button>
                </div>
              ))}
            </div>
          )}

          {/* Appearance Tab */}
          {activeTab === "appearance" && (
            <div className="space-y-6">
              <div>
                <h2 className="text-lg font-semibold text-white">Appearance</h2>
                <p className="text-xs text-secondary-500 mt-0.5">Customize the look and feel</p>
              </div>

              <div className="rounded-xl border border-white/5 bg-white/[0.02] p-4">
                <p className="text-sm font-medium text-white mb-3">Theme</p>
                <div className="flex gap-3">
                  {[
                    { name: "Dark", bg: "bg-amex-charcoal", border: "border-primary-500/40", active: true },
                    { name: "Light", bg: "bg-white", border: "border-white/10", active: false },
                    { name: "System", bg: "bg-gradient-to-r from-amex-charcoal to-white", border: "border-white/10", active: false },
                  ].map((theme) => (
                    <button key={theme.name} className={`flex flex-col items-center gap-2 rounded-xl border p-3 transition-all ${
                      theme.active ? theme.border + " bg-primary-500/5" : "border-white/5 hover:border-white/15"
                    }`}>
                      <div className={`h-12 w-20 rounded-lg ${theme.bg} border border-white/10`} />
                      <span className="text-xs text-secondary-300">{theme.name}</span>
                    </button>
                  ))}
                </div>
              </div>

              <div className="rounded-xl border border-white/5 bg-white/[0.02] p-4">
                <p className="text-sm font-medium text-white mb-3">Accent Color</p>
                <div className="flex gap-2">
                  {[
                    { color: "bg-amex-gold", active: true },
                    { color: "bg-blue-500", active: false },
                    { color: "bg-emerald-500", active: false },
                    { color: "bg-purple-500", active: false },
                    { color: "bg-rose-500", active: false },
                  ].map((c, i) => (
                    <button key={i} className={`h-8 w-8 rounded-full ${c.color} ${c.active ? "ring-2 ring-white ring-offset-2 ring-offset-amex-charcoal" : "opacity-60 hover:opacity-100"} transition-all`} />
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
