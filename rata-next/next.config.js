/** @type {import('next').NextConfig} */
const nextConfig = {
  /* RATA renders no <Image>, so the image optimizer is surface with nothing
     behind it — and an unauthenticated endpoint that decodes attacker-supplied
     images is the kind of surface that keeps growing advisories. It carried two
     of them (RCE through AVIF, and unbounded cache growth) on Next 14; the
     Next 16 upgrade fixed those, and turning the endpoint off keeps the next
     one from mattering either.

     formats and remotePatterns are pinned as well, so it stays harmless if a
     future page renders an image and switches the optimizer back on. */
  images: {
    unoptimized: true,
    formats: ['image/webp'],
    remotePatterns: [],
    dangerouslyAllowSVG: false,
  },

  /* Security headers on every response, pages and API alike.

     - HSTS for a year, subdomains included, not preloaded (preloading is
       hard to undo and is a decision for the owner, not a default).
     - No framing, two ways: X-Frame-Options for older browsers and
       frame-ancestors for the rest. Nothing here is meant to be embedded, and
       a framed account page is how a click on "Delete account" gets stolen.
       This does not touch the mail frame in /app, which is srcdoc.
     - No MIME sniffing, and a Referer that never carries a path off-site.

     CORS for the desktop app is separate (lib/cors.js, per route) and is
     unaffected: these headers say nothing about who may read an answer. */
  async headers() {
    return [{
      source: '/:path*',
      headers: [
        { key: 'Strict-Transport-Security', value: 'max-age=31536000; includeSubDomains' },
        { key: 'X-Frame-Options', value: 'DENY' },
        { key: 'Content-Security-Policy', value: "frame-ancestors 'none'" },
        { key: 'X-Content-Type-Options', value: 'nosniff' },
        { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
      ],
    }];
  },
};
export default nextConfig;
