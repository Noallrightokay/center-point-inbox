import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';
import { ImapFlow } from 'imapflow';

export const dynamic = 'force-dynamic';

/* Apple has no OAuth for Mail — the sanctioned mechanism is an app-specific
   password (appleid.apple.com), a revocable per-app token. We verify it by
   actually connecting to iCloud IMAP before storing anything. */
export async function POST(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });
  let body; try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }
  const email = String(body.email || '').trim().toLowerCase();
  const pass = String(body.appPassword || '').trim();
  if (!/.+@.+\..+/.test(email) || pass.length < 8)
    return NextResponse.json({ error: 'Enter your iCloud address and the app-specific password' });

  const client = new ImapFlow({
    host: 'imap.mail.me.com', port: 993, secure: true,
    auth: { user: email, pass }, logger: false,
  });
  try {
    await client.connect();
    await client.logout();
  } catch (e) {
    return NextResponse.json({ error: 'iCloud rejected the sign-in — check the address and app-specific password (not your Apple ID password)' });
  }
  await sb.from('provider_tokens').upsert({
    user_id: user.id, provider: 'apple', label: email,
    access: pass, refresh: null, expires_at: null,
    updated_at: new Date().toISOString(),
  });
  return NextResponse.json({ ok: true, label: email });
}
