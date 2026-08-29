import { NextResponse } from 'next/server';

/* Never prerender: env vars are read per request, not baked in at build time.
   Without this Next would freeze whatever was set when the build ran. */
export const dynamic = 'force-dynamic';

/* ---------------------------------------------------------------------------
   /config.js — the public site configuration, emitted from environment
   variables instead of a hand-edited file.

   Everything here ships to every visitor's browser. Only publishable values
   belong in it: the Supabase URL and anon key (which RLS is designed to expose),
   the Google OAuth client ID, and Stripe payment links. SUPABASE_SERVICE_ROLE_KEY
   is deliberately not read anywhere in this file — it bypasses row-level
   security and is server-only.

   Unset variables become empty strings rather than errors, so an unconfigured
   deployment still runs: RATA falls back to on-device accounts.
   --------------------------------------------------------------------------- */

const PUBLIC_CONFIG = [
  ['supabaseUrl',    'SUPABASE_URL'],
  ['supabaseKey',    'SUPABASE_ANON_KEY'],
  ['googleClientId', 'GOOGLE_CLIENT_ID'],
  ['stripeMonthly',  'STRIPE_MONTHLY'],
  ['stripeAnnual',   'STRIPE_ANNUAL'],
  ['stripePortal',   'STRIPE_PORTAL'],
];

/* A service-role key in the public config would hand every visitor read/write
   over every user's data — including the stored mail credentials in
   provider_tokens. It is an easy paste to get wrong (the two keys sit next to
   each other on the same Supabase settings page and look alike), and the damage
   is silent, so refuse it outright rather than serve it. */
function isSecretKey(v) {
  if (!v) return false;
  if (v.startsWith('sb_secret_')) return true;          // current-style secret key
  const parts = v.split('.');                            // legacy JWT-style key
  if (parts.length !== 3) return false;
  try {
    const pad = parts[1].length % 4;
    const b64 = parts[1].replace(/-/g, '+').replace(/_/g, '/') + (pad ? '='.repeat(4 - pad) : '');
    return JSON.parse(Buffer.from(b64, 'base64').toString('utf8')).role === 'service_role';
  } catch { return false; }
}

export async function GET() {
  const cfg = {};
  const refused = [];

  for (const [key, envName] of PUBLIC_CONFIG) {
    const raw = (process.env[envName] || '').trim();
    if (envName === 'SUPABASE_ANON_KEY' && isSecretKey(raw)) {
      refused.push(envName);
      cfg[key] = '';
      continue;
    }
    cfg[key] = raw;
  }

  /* JSON.stringify handles escaping — a value with a quote or newline in it
     must not be able to break out of this script. */
  let body = `/* RATA public configuration — generated per request from the
   environment. Do not edit: change the variables in your host's settings
   instead. Secrets are never emitted here. */
window.RATA_CONFIG = ${JSON.stringify(cfg, null, 2)};\n`;

  if (refused.length) {
    const msg = `RATA: ${refused.join(', ')} looks like a Supabase SERVICE ROLE key, not the anon key. Refusing to publish it. Cloud accounts are disabled until it is corrected.`;
    console.error(msg);
    body += `console.error(${JSON.stringify(msg)});\n`;
  }

  return new NextResponse(body, {
    headers: {
      'Content-Type': 'application/javascript; charset=utf-8',
      /* Configuration must never be served stale — a rotated key has to take
         effect on the next load, not whenever a cache decides to expire. */
      'Cache-Control': 'no-store, must-revalidate',
    },
  });
}
