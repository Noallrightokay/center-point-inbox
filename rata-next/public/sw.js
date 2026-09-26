/* Rata service worker — app shell precache + offline fallback */
const V = 'rata-shell-v19';
const SHELL = [
  './', './index.html', './auth.html', './app.html', './account.html', './manifest.json', './config.js',
  './fonts/fonts.css',
  './icons/icon-192.png', './icons/icon-512.png', './icons/maskable-512.png', './icons/apple-touch-icon.png', './icons/mark-256.png'
];

/* Everything RATA runs on, served from our own origin — no third-party CDN at
   runtime. These come down during install, with the initial download, so the
   customer never waits on a fetch mid-conversion and conversion works offline.

   Kept separate from SHELL on purpose: addAll() is atomic, so bundling ~2MB of
   engines into the shell install would mean one flaky byte costs the app its
   entire offline shell. These are added individually and best-effort — a miss
   here is recoverable (the page fetches it on demand), a broken shell is not. */
/* The typefaces, precached the same way and for the same reason: the site
   should look like itself with the network off, and a page that falls back to
   the system font offline is a page that visibly changes shape the moment the
   wifi drops.

   Best-effort rather than part of the shell, deliberately. unicode-range means
   a browser fetches only what it needs at runtime, so most of these are never
   requested by any one reader — making the shell install atomic over all of
   them would risk the whole offline shell for bytes nobody was going to use. */
const FONTS = [
  './fonts/fredoka-500-hebrew.woff2',
  './fonts/fredoka-500-latin-ext.woff2',
  './fonts/fredoka-500-latin.woff2',
  './fonts/fredoka-600-hebrew.woff2',
  './fonts/fredoka-600-latin-ext.woff2',
  './fonts/fredoka-600-latin.woff2',
  './fonts/fredoka-700-hebrew.woff2',
  './fonts/fredoka-700-latin-ext.woff2',
  './fonts/fredoka-700-latin.woff2',
  './fonts/instrument-sans-400-latin-ext.woff2',
  './fonts/instrument-sans-400-latin.woff2',
  './fonts/instrument-sans-500-latin-ext.woff2',
  './fonts/instrument-sans-500-latin.woff2',
  './fonts/instrument-sans-600-latin-ext.woff2',
  './fonts/instrument-sans-600-latin.woff2',
  './fonts/instrument-sans-700-latin-ext.woff2',
  './fonts/instrument-sans-700-latin.woff2',
  './fonts/sora-400-latin-ext.woff2',
  './fonts/sora-400-latin.woff2',
  './fonts/sora-600-latin-ext.woff2',
  './fonts/sora-600-latin.woff2',
  './fonts/sora-700-latin-ext.woff2',
  './fonts/sora-700-latin.woff2'
];

const ENGINES = [
  './vendor/supabase-js-2.112.4.js',
  './vendor/mammoth-1.8.0.browser.min.js',
  './vendor/xlsx-0.20.3.full.min.js',
  './vendor/jspdf-2.5.1.umd.min.js'
];

self.addEventListener('install', e => {
  e.waitUntil(
    caches.open(V)
      .then(c => c.addAll(SHELL)
        .then(() => Promise.allSettled([...FONTS, ...ENGINES].map(u => c.add(u)))))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener('activate', e => {
  e.waitUntil(
    caches.keys().then(keys => Promise.all(keys.filter(k => k !== V).map(k => caches.delete(k))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', e => {
  const url = new URL(e.request.url);
  if (e.request.method !== 'GET') return;

  // Never intercept live APIs (Supabase handles itself)
  const passthrough = ['supabase.co'];
  if (passthrough.some(h => url.hostname.endsWith(h))) return;

  // Never intercept our own backend. These are authenticated, per-user, and
  // change on every call — caching them would serve one user's synced messages
  // back forever (and write them to disk). Always straight to the network.
  if (url.origin === location.origin && url.pathname.startsWith('/api/')) return;

  // Pages: network-first so updates land, cached shell when offline
  if (e.request.mode === 'navigate' || e.request.destination === 'document' || url.pathname.endsWith('config.js')) {
    e.respondWith(
      fetch(e.request)
        .then(r => { const cp = r.clone(); caches.open(V).then(c => c.put(e.request, cp)); return r; })
        .catch(() => caches.match(e.request).then(m => m || caches.match('./app.html')))
    );
    return;
  }

  // Same-origin assets + Google Fonts: cache-first with background fill
  if (url.origin === location.origin || url.hostname.endsWith('gstatic.com') || url.hostname.endsWith('fonts.googleapis.com')) {
    e.respondWith(
      caches.match(e.request).then(m => m || fetch(e.request).then(r => {
        const cp = r.clone(); caches.open(V).then(c => c.put(e.request, cp)); return r;
      }))
    );
  }
});
