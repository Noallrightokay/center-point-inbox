/* The desktop app's pages are served from its own origin — tauri://localhost
   on macOS and Linux, http(s)://tauri.localhost on Windows — so every call it
   makes to mailrata.org is cross-origin, and a browser engine refuses to hand
   the answer to the page unless the server says that origin may read it. A
   JSON POST also sends an OPTIONS request first and gives up if it fails.

   Without this, licence renewal never worked from the app: it read as
   "offline" every time, and a licence would simply run out. Only the app's
   origins are named; any other website calling these endpoints gets no
   permission to read the answer. */
const APP_ORIGINS = new Set(['tauri://localhost', 'http://tauri.localhost', 'https://tauri.localhost']);

export function corsHeaders(req) {
  const origin = req.headers.get('origin') || '';
  if (!APP_ORIGINS.has(origin)) return {};
  return {
    'Access-Control-Allow-Origin': origin,
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type',
    'Access-Control-Max-Age': '86400',
    Vary: 'Origin',
  };
}

export function preflight(req) {
  return new Response(null, { status: 204, headers: corsHeaders(req) });
}
