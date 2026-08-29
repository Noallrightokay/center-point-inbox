import { NextResponse } from 'next/server';

// Serve the RATA static pages on clean URLs.
export function middleware(req) {
  const { pathname } = req.nextUrl;
  const map = { '/': '/index.html', '/auth': '/auth.html', '/app': '/app.html' };
  if (map[pathname]) return NextResponse.rewrite(new URL(map[pathname], req.url));
  return NextResponse.next();
}
export const config = { matcher: ['/', '/auth', '/app'] };
