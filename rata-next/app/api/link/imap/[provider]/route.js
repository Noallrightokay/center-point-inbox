import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../../lib/server';
import { imapProvider, verifyImap } from '../../../../../lib/imap';

export const dynamic = 'force-dynamic';

/* Link a mail account with an app-specific password — a revocable per-app
   token, which is the sanctioned mechanism where OAuth is unavailable (Apple)
   or gated behind restricted-scope verification (Gmail). The credentials are
   proved against the real IMAP server before anything is written. */
export async function POST(req, { params }) {
  const def = imapProvider(params.provider);
  if (!def) return NextResponse.json({ error: 'Unknown mail provider' }, { status: 400 });

  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  let body;
  try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }
  const email = String(body.email || '').trim().toLowerCase();
  const pass = String(body.appPassword || '').replace(/\s+/g, '');
  if (!/.+@.+\..+/.test(email) || pass.length < 8)
    return NextResponse.json({ error: `Enter your ${def.label} address and its app-specific password` });

  const check = await verifyImap(def, email, pass);
  if (!check.ok) return NextResponse.json({ error: check.error });

  const { error: e2 } = await sb.from('provider_tokens').upsert({
    user_id: user.id, provider: def.token, label: email,
    access: pass, refresh: null, expires_at: null,
    updated_at: new Date().toISOString(),
  });
  if (e2) return NextResponse.json({ error: 'Could not save the link — ' + e2.message });

  return NextResponse.json({ ok: true, label: email, provider: params.provider });
}
