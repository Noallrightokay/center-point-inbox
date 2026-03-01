import { NextRequest, NextResponse } from "next/server";

/**
 * Next.js Edge Middleware for Center Point Inbox.
 *
 * Responsibilities:
 * 1. Protect /dashboard and /file routes – redirect to /login when no auth
 *    token cookie is present.
 * 2. Protect /admin routes – additionally verify that the token contains an
 *    admin role claim.
 *
 * NOTE: This is a *client-side heuristic* only. The backend **must** verify
 * every request independently. We inspect the JWT payload without signature
 * verification to keep the middleware lightweight (Edge Runtime compatible).
 */

// ---------------------------------------------------------------------------
// Configuration – routes this middleware should intercept
// ---------------------------------------------------------------------------

export const config = {
  matcher: ["/dashboard/:path*", "/file/:path*", "/admin/:path*"],
};

// ---------------------------------------------------------------------------
// Lightweight JWT payload extraction (no signature verification)
// ---------------------------------------------------------------------------

interface JwtPayload {
  sub?: string;
  roles?: string[];
  exp?: number;
  [key: string]: unknown;
}

function parseJwtPayload(token: string): JwtPayload | null {
  try {
    const parts = token.split(".");
    if (parts.length !== 3) return null;

    const base64Url = parts[1];
    const base64 = base64Url.replace(/-/g, "+").replace(/_/g, "/");

    // Edge Runtime supports atob
    const jsonPayload = atob(base64);
    return JSON.parse(jsonPayload) as JwtPayload;
  } catch {
    return null;
  }
}

function isTokenExpired(payload: JwtPayload): boolean {
  if (!payload.exp) return false;
  const nowSeconds = Math.floor(Date.now() / 1000);
  // 30-second buffer for network latency
  return payload.exp <= nowSeconds + 30;
}

// ---------------------------------------------------------------------------
// Middleware handler
// ---------------------------------------------------------------------------

export default function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;

  // Read the auth token from the cookie set by the backend.
  // We check multiple cookie names to accommodate different backend
  // configurations.
  const token =
    request.cookies.get("access_token")?.value ??
    request.cookies.get("auth_token")?.value ??
    request.cookies.get("token")?.value ??
    null;

  const loginUrl = new URL("/login", request.url);
  loginUrl.searchParams.set("redirect", pathname);

  // -------------------------------------------------------------------------
  // No token present – redirect to login
  // -------------------------------------------------------------------------
  if (!token) {
    return NextResponse.redirect(loginUrl);
  }

  // -------------------------------------------------------------------------
  // Token present – verify basic validity
  // -------------------------------------------------------------------------
  const payload = parseJwtPayload(token);

  if (!payload) {
    // Malformed token – redirect to login
    const response = NextResponse.redirect(loginUrl);
    // Clear the invalid cookie
    response.cookies.delete("access_token");
    response.cookies.delete("auth_token");
    response.cookies.delete("token");
    return response;
  }

  if (isTokenExpired(payload)) {
    // Expired token – redirect to login. The client-side refresh logic in
    // the API interceptor may handle this, but we redirect as a safety net.
    return NextResponse.redirect(loginUrl);
  }

  // -------------------------------------------------------------------------
  // Admin route protection
  // -------------------------------------------------------------------------
  if (pathname.startsWith("/admin")) {
    const roles = payload.roles ?? [];
    if (!roles.includes("admin")) {
      // Non-admin user trying to access admin routes – redirect to dashboard
      const dashboardUrl = new URL("/dashboard", request.url);
      return NextResponse.redirect(dashboardUrl);
    }
  }

  // -------------------------------------------------------------------------
  // All checks passed – allow the request
  // -------------------------------------------------------------------------
  return NextResponse.next();
}
