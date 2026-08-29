import { createClient } from '@supabase/supabase-js';

export function admin() {
  const url = process.env.SUPABASE_URL || process.env.NEXT_PUBLIC_SUPABASE_URL;
  const key = process.env.SUPABASE_SERVICE_ROLE_KEY;
  if (!url || !key) return null;
  return createClient(url, key, { auth: { persistSession: false } });
}

/* Resolve the signed-in RATA user from the Authorization: Bearer <supabase jwt> header */
export async function userFromRequest(req) {
  const sb = admin();
  if (!sb) return { error: 'Backend not configured: set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY' };
  const auth = req.headers.get('authorization') || '';
  const jwt = auth.startsWith('Bearer ') ? auth.slice(7) : '';
  if (!jwt) return { error: 'Not signed in' };
  const { data, error } = await sb.auth.getUser(jwt);
  if (error || !data?.user) return { error: 'Session invalid — sign in again' };
  return { user: data.user, sb };
}

export function appUrl(req) {
  return process.env.APP_URL || new URL(req.url).origin;
}

export const PROVIDERS = {
  ms: {
    env: ['MS_CLIENT_ID', 'MS_CLIENT_SECRET'],
    scope: 'offline_access User.Read Mail.Read',
  },
  slack: {
    env: ['SLACK_CLIENT_ID', 'SLACK_CLIENT_SECRET'],
    userScope: 'channels:history,groups:history,im:history,mpim:history,users:read,team:read',
  },
};

export function providerReady(p) {
  const def = PROVIDERS[p];
  if (!def) return false;
  return def.env.every(k => !!process.env[k]);
}

/* ---------------------------------------------------------------------------
   OAuth link-state binding.

   The state row in `link_states` proves *a* RATA user started a link. It does
   not prove the browser finishing the flow is the same one — the state travels
   in a URL, so anyone who obtains it could complete consent with their own
   Microsoft/Slack account and have it attached to the victim's workspace.

   So we also drop the state in an HttpOnly cookie at issue time and require it
   to match at both /start and /callback. SameSite=Lax (not Strict) is required:
   the provider sends the user back via a top-level GET navigation, which Lax
   allows and Strict would drop.
   --------------------------------------------------------------------------- */
export const LINK_COOKIE = 'rata_link_state';
export const LINK_TTL_MS = 10 * 60 * 1000;

export function setLinkCookie(res, state) {
  res.cookies.set(LINK_COOKIE, state, {
    httpOnly: true,
    secure: true,
    sameSite: 'lax',
    path: '/',
    maxAge: LINK_TTL_MS / 1000,
  });
  return res;
}

export function clearLinkCookie(res) {
  res.cookies.set(LINK_COOKIE, '', { httpOnly: true, secure: true, sameSite: 'lax', path: '/', maxAge: 0 });
  return res;
}

/* Constant-time-ish compare so a mismatch can't be probed byte by byte. */
export function sameState(a, b) {
  if (!a || !b || a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export function stateExpired(row) {
  return !row.created_at || Date.now() - new Date(row.created_at).getTime() > LINK_TTL_MS;
}
