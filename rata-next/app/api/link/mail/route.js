import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';
import { describe, verifyMail, mailKey } from '../../../../lib/mail';
import { planForUser, countLinks, refusal } from '../../../../lib/plan';

export const dynamic = 'force-dynamic';

/* Link any mailbox.

   The user supplies an address and an app password; RATA works out the server.
   `host` is accepted as an override for the addresses we cannot guess — a
   company mail server, a small host — and is echoed back so the client can
   show what it found.

   POST adds, DELETE removes. Both are per-account: a person may hold several
   mailboxes, up to what their plan includes. */

export async function POST(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  let body;
  try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }
  const email = String(body.email || '').trim().toLowerCase();
  const pass = String(body.appPassword || '').replace(/\s+/g, '');
  const host = String(body.host || '').trim().toLowerCase() || null;

  if (!/.+@.+\..+/.test(email)) return NextResponse.json({ error: 'Enter the full email address' });
  if (pass.length < 8) return NextResponse.json({ error: 'Enter the app password for that address' });

  const info = describe(email);
  if (info?.unsupported) return NextResponse.json({ error: info.why });

  const { data: rows } = await sb.from('provider_tokens')
    .select('provider,label').eq('user_id', user.id);
  const already = (rows || []).find(r => r.provider === mailKey(email));

  /* Relinking a mailbox you already have is a repair, not a new link, so it
     must not be refused for being over the limit. */
  if (!already) {
    const plan = await planForUser(sb, user.email);
    const stop = refusal(plan, 'mail', countLinks(rows).mail);
    if (stop) return NextResponse.json({ error: stop, overLimit: true, plan }, { status: 402 });
  }

  const check = await verifyMail(email, pass, host);
  /* Ask for the server address only when we were guessing it and the guess
     failed — a known provider that refuses is a password problem, and
     showing a server box there would send the user hunting for nothing. */
  if (!check.ok) return NextResponse.json({ error: check.error, needsHost: !host && !!info?.guessed });

  const { error: e2 } = await sb.from('provider_tokens').upsert({
    user_id: user.id,
    provider: mailKey(email),
    label: email,
    access: pass,
    refresh: null,
    extra: { kind: 'mail', host: check.host, port: 993, provider_label: check.label },
    expires_at: null,
    updated_at: new Date().toISOString(),
  });
  if (e2) return NextResponse.json({ error: 'Could not save the link — ' + e2.message });

  return NextResponse.json({ ok: true, email, host: check.host, label: check.label, relinked: !!already });
}

export async function DELETE(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const email = String(new URL(req.url).searchParams.get('email') || '').trim().toLowerCase();
  if (!email) return NextResponse.json({ error: 'Which mailbox?' });

  const { error: e2 } = await sb.from('provider_tokens')
    .delete().eq('user_id', user.id).eq('provider', mailKey(email));
  if (e2) return NextResponse.json({ error: 'Could not remove the link — ' + e2.message });
  return NextResponse.json({ ok: true, email });
}
