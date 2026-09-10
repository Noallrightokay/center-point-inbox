import { create } from "zustand";

import { api } from "@/lib/api";
import { clearToken, getToken, setToken } from "@/lib/token";
import { parseRole, type Role } from "@/lib/enums";
import type { AuthResponse, AuthUser } from "@/types";

type AuthPhase = "loading" | "authed" | "anon";

interface AuthState {
  user: AuthUser | null;
  phase: AuthPhase;
  /** Restore the session from a persisted token on app boot. */
  bootstrap: () => Promise<void>;
  setSession: (res: AuthResponse) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  phase: "loading",

  bootstrap: async () => {
    if (!getToken()) {
      set({ phase: "anon", user: null });
      return;
    }
    try {
      const user = await api.me();
      set({ user, phase: "authed" });
    } catch {
      clearToken();
      set({ user: null, phase: "anon" });
    }
  },

  setSession: (res) => {
    setToken(res.token);
    set({ user: res.user, phase: "authed" });
  },

  logout: () => {
    clearToken();
    set({ user: null, phase: "anon" });
  },
}));

/** Role helpers derived from the current user. */
export function useRole(): Role | null {
  const user = useAuthStore((s) => s.user);
  return user ? parseRole(user.role) : null;
}

export function roleOf(user: AuthUser | null): Role | null {
  return user ? parseRole(user.role) : null;
}

export function canSeeCompliance(role: Role | null): boolean {
  return role === "Admin" || role === "ComplianceOfficer";
}

export function isAdmin(role: Role | null): boolean {
  return role === "Admin";
}
