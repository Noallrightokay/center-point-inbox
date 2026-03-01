import axios from "axios";

/**
 * Authentication utilities for Center Point Inbox.
 *
 * Tokens are stored **in-memory only** (never in localStorage or sessionStorage)
 * to prevent PHI leakage from client-side storage that persists beyond the
 * browser session or is accessible to XSS attacks.
 */

const API_BASE_URL =
  process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";

// ---------------------------------------------------------------------------
// In-memory token store
// ---------------------------------------------------------------------------

let accessToken: string | null = null;

/**
 * Returns the current in-memory access token, or null if not set.
 */
export function getToken(): string | null {
  return accessToken;
}

/**
 * Sets the access token in the in-memory store.
 */
export function setToken(token: string): void {
  accessToken = token;
}

/**
 * Clears the access token from the in-memory store.
 */
export function clearToken(): void {
  accessToken = null;
}

// ---------------------------------------------------------------------------
// JWT parsing (client-side display only -- no signature verification)
// ---------------------------------------------------------------------------

export interface JwtPayload {
  sub: string;
  email?: string;
  name?: string;
  roles?: string[];
  orgId?: string;
  iat?: number;
  exp?: number;
  [key: string]: unknown;
}

/**
 * Decode a JWT payload without verifying its signature.
 *
 * This is suitable **only** for displaying user information on the client.
 * All authoritative checks must happen server-side.
 *
 * @returns The decoded payload, or `null` if the token is malformed.
 */
export function parseJwt(token: string): JwtPayload | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) {
      return null;
    }

    // Base64-url decode the payload section.
    const base64Url = parts[1];
    const base64 = base64Url.replace(/-/g, "+").replace(/_/g, "/");
    const jsonPayload = decodeURIComponent(
      atob(base64)
        .split("")
        .map((c) => "%" + ("00" + c.charCodeAt(0).toString(16)).slice(-2))
        .join(""),
    );

    return JSON.parse(jsonPayload) as JwtPayload;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Token refresh
// ---------------------------------------------------------------------------

/**
 * Attempt a silent token refresh by calling the backend refresh endpoint.
 *
 * The refresh token is carried in an httpOnly cookie set by the backend,
 * so we rely on `withCredentials: true` to send it automatically.
 *
 * @returns The new access token string, or `null` if refresh failed.
 */
export async function refreshToken(): Promise<string | null> {
  try {
    const response = await axios.post<{ accessToken: string }>(
      `${API_BASE_URL}/api/auth/refresh`,
      {},
      { withCredentials: true },
    );

    const newToken = response.data.accessToken;
    if (newToken) {
      setToken(newToken);
      return newToken;
    }
    return null;
  } catch {
    clearToken();
    return null;
  }
}

// ---------------------------------------------------------------------------
// Authentication status check
// ---------------------------------------------------------------------------

/**
 * Check whether the user currently holds a valid (non-expired) access token.
 *
 * Note: this is a **client-side heuristic** only. The backend must always
 * verify the token independently.
 */
export function isAuthenticated(): boolean {
  const token = getToken();
  if (!token) {
    return false;
  }

  const payload = parseJwt(token);
  if (!payload || !payload.exp) {
    return false;
  }

  // Consider the token expired 30 seconds early to give a buffer for
  // network latency when making API calls.
  const bufferSeconds = 30;
  const nowSeconds = Math.floor(Date.now() / 1000);
  return payload.exp > nowSeconds + bufferSeconds;
}
