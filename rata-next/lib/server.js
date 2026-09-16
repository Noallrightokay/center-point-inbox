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
