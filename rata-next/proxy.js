import { NextResponse } from 'next/server';

/* Send this whole deployment somewhere else.
 *
 * Set REDIRECT_TO on a deployment that should no longer serve the app — e.g.
 * mailrata.com pointing at https://mailrata.org — and every request moves,
 * path and query intact. Doing it here rather than at the host means it also
 * covers /api and clean URLs, and it is configuration rather than a domain
 * baked into the code, so the same build runs on both sites.
 *
 * 308 keeps the method and tells browsers and search engines the move is
 * permanent, which is what makes the second domain stop competing with the
 * first in search results. */
function redirectTarget(req) {
  const to = process.env.REDIRECT_TO;
  if (!to) return null;
  let target;
  try { target = new URL(to); } catch { return null; }
  const host = req.headers.get('host') || '';
  /* Never redirect a request that is already on the destination, or the site
     would bounce forever. */
  if (!host || host === target.host) return null;
  const url = new URL(req.url);
  url.protocol = target.protocol;
  url.host = target.host;
  url.port = '';
  return url;
}

// Serve the RATA static pages on clean URLs.
//
// /config.js is rewritten to an API route rather than being a route itself.
// Middleware runs before static file serving, so this wins even when a stale
// public/config.js is present on the host — which happens in practice: archive
// deploys are additive, so a file deleted from source keeps being served from
// the previous deployment and would otherwise shadow the route, silently
// reverting the site to a hand-edited config.
export default function proxy(req) {
  const moved = redirectTarget(req);
  if (moved) return NextResponse.redirect(moved, 308);

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

/* Broad matcher so REDIRECT_TO can move the entire site, not just the handful
   of paths that get rewritten. Next's own build output is excluded — those are
   served straight from disk and never need either behaviour. */
export const config = {
  matcher: ['/((?!_next/static|_next/image|favicon.ico).*)'],
};
