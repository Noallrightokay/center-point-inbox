import { create } from "zustand";
import { post } from "@/lib/api";
import {
  setToken,
  clearToken,
  parseJwt,
  refreshToken as refreshAuthToken,
  isAuthenticated as checkAuth,
} from "@/lib/auth";
import type { User, TokenResponse } from "@/types";

// ---------------------------------------------------------------------------
// Store shape
// ---------------------------------------------------------------------------

export interface AuthState {
  /** The currently authenticated user, or null when logged out. */
  user: User | null;

  /** Whether the user holds a valid session. */
  isAuthenticated: boolean;

  /** True while an authentication operation (login, refresh) is in flight. */
  isLoading: boolean;

  /**
   * Authenticate via an OAuth provider.
   *
   * @param provider  - `"google"` or `"microsoft"`
   * @param code      - The authorization code returned by the OAuth flow
   * @param redirectUri - The redirect URI used in the OAuth flow
   */
  login: (
    provider: "google" | "microsoft",
    code: string,
    redirectUri: string,
  ) => Promise<void>;

  /** Log the user out, clearing all auth state. */
  logout: () => Promise<void>;

  /** Attempt a silent session refresh using the httpOnly refresh cookie. */
  refreshSession: () => Promise<void>;

  /** Manually set the user object (e.g. after fetching /me). */
  setUser: (user: User) => void;
}

// ---------------------------------------------------------------------------
// Store implementation
// ---------------------------------------------------------------------------

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  isAuthenticated: false,
  isLoading: false,

  login: async (
    provider: "google" | "microsoft",
    code: string,
    redirectUri: string,
  ) => {
    set({ isLoading: true });

    try {
      const response = await post<TokenResponse>(
        `/auth/oauth/${provider}/callback`,
        { code, redirectUri },
      );

      setToken(response.accessToken);

      const payload = parseJwt(response.accessToken);
      const user: User = {
        id: response.user?.id ?? payload?.sub ?? "",
        email: response.user?.email ?? payload?.email ?? "",
        name: response.user?.name ?? payload?.name ?? "",
        avatarUrl: response.user?.avatarUrl ?? null,
        provider: response.user?.provider ?? provider,
        roles: response.user?.roles ?? payload?.roles ?? [],
        organizationId: response.user?.organizationId ?? payload?.orgId ?? null,
        createdAt: response.user?.createdAt ?? new Date().toISOString(),
        updatedAt: response.user?.updatedAt ?? new Date().toISOString(),
      };

      set({ user, isAuthenticated: true, isLoading: false });
    } catch (error) {
      clearToken();
      set({ user: null, isAuthenticated: false, isLoading: false });
      throw error;
    }
  },

  logout: async () => {
    set({ isLoading: true });

    try {
      await post("/auth/logout", {});
    } catch {
      // Best-effort logout — clear client state regardless.
    } finally {
      clearToken();
      set({ user: null, isAuthenticated: false, isLoading: false });
    }
  },

  refreshSession: async () => {
    set({ isLoading: true });

    try {
      const newToken = await refreshAuthToken();

      if (newToken && checkAuth()) {
        const payload = parseJwt(newToken);
        const user: User = {
          id: payload?.sub ?? "",
          email: payload?.email ?? "",
          name: payload?.name ?? "",
          avatarUrl: null,
          provider: "google",
          roles: payload?.roles ?? [],
          organizationId: payload?.orgId ?? null,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        };

        set({ user, isAuthenticated: true, isLoading: false });
      } else {
        clearToken();
        set({ user: null, isAuthenticated: false, isLoading: false });
      }
    } catch {
      clearToken();
      set({ user: null, isAuthenticated: false, isLoading: false });
    }
  },

  setUser: (user: User) => {
    set({ user, isAuthenticated: true });
  },
}));
