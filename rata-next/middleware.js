import { NextResponse } from 'next/server';

// Serve the RATA static pages on clean URLs.
//
// /config.js is rewritten to an API route rather than being a route itself.
// Middleware runs before static file serving, so this wins even when a stale
// public/config.js is present on the host — which happens in practice: archive
// deploys are additive, so a file deleted from source keeps being served from
// the previous deployment and would otherwise shadow the route, silently
// reverting the site to a hand-edited config.
export function middleware(req) {
  const { pathname } = req.nextUrl;
  const map = {
    '/': '/index.html',
    '/auth': '/auth.html',
    '/app': '/app.html',
    '/config.js': '/api/config',
  };
  if (map[pathname]) return NextResponse.rewrite(new URL(map[pathname], req.url));
  return NextResponse.next();
}
export const config = { matcher: ['/', '/auth', '/app', '/config.js'] };
