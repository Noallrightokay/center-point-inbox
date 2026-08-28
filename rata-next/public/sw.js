/* Rata service worker — app shell precache + offline fallback */
const V = 'rata-shell-v13';
const SHELL = [
  './', './index.html', './auth.html', './app.html', './manifest.json', './config.js',
  './icons/icon-192.png', './icons/icon-512.png', './icons/maskable-512.png', './icons/apple-touch-icon.png', './icons/mark-256.png'
];

/* Everything RATA runs on, served from our own origin — no third-party CDN at
   runtime. These come down during install, with the initial download, so the
   customer never waits on a fetch mid-conversion and conversion works offline.

   Kept separate from SHELL on purpose: addAll() is atomic, so bundling ~2MB of
   engines into the shell install would mean one flaky byte costs the app its
   entire offline shell. These are added individually and best-effort — a miss
   here is recoverable (the page fetches it on demand), a broken shell is not. */
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
        .then(() => Promise.allSettled(ENGINES.map(u => c.add(u)))))
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

  // Never intercept live APIs (Gmail, Google auth, Supabase, Anthropic, CDNs handle themselves)
  const passthrough = ['googleapis.com', 'accounts.google.com', 'supabase.co', 'anthropic.com', 'translate.googleapis.com'];
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
