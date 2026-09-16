import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../../lib/server';
import { imapProvider, verifyImap } from '../../../../../lib/imap';
import { sealRow } from '../../../../../lib/secrets';
import { planForUser, countLinks, refusal } from '../../../../../lib/plan';
import { allowed, failed, succeeded, LINK_ATTEMPTS, ADDRESS_ATTEMPTS, waitPhrase } from '../../../../../lib/ratelimit';

export const dynamic = 'force-dynamic';

/* Link a mail account with an app-specific password — a revocable per-app
   token, which is the sanctioned mechanism where OAuth is unavailable (Apple)
   or gated behind restricted-scope verification (Gmail). The credentials are
   proved against the real IMAP server before anything is written. */
export async function POST(req, { params }) {
  const { provider } = await params;
  const def = imapProvider(provider);
  if (!def) return NextResponse.json({ error: 'Unknown mail provider' }, { status: 400 });

  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  let body;
  try { body = await req.json(); } catch { return NextResponse.json({ error: 'Bad request' }); }
  const email = String(body.email || '').trim().toLowerCase();
  const pass = String(body.appPassword || '').replace(/\s+/g, '');
  if (!/.+@.+\..+/.test(email) || pass.length < 8)
    return NextResponse.json({ error: `Enter your ${def.label} address and its app-specific password` });

  /* The same allowance the newer /api/link/mail enforces. Without it this
     older route is a way around the plan: a Base account capped at two
     mailboxes could link two here and two there and hold four. Both write to
     provider_tokens, so both have to count the rows already in it. */
  const { data: rows } = await sb.from('provider_tokens')
    .select('provider').eq('user_id', user.id);
  if (!(rows || []).some(r => r.provider === def.token)) {
    const plan = await planForUser(sb, user.email);
    const stop = refusal(plan, 'mail', countLinks(rows).mail);
    if (stop) return NextResponse.json({ error: stop, overLimit: true, plan }, { status: 402 });
  }

  /* And the same limit on guessing — see lib/ratelimit.js. This route reaches
     Gmail and iCloud, which are the two worth testing a leaked list against. */
  const byUser = `link:${user.id}`;
  const byAddr = `addr:${email}`;
  for (const [key, policy, who] of [[byUser, LINK_ATTEMPTS, 'this account'], [byAddr, ADDRESS_ATTEMPTS, email]]) {
    const gate = allowed(key, policy);
    if (!gate.ok) {
      return NextResponse.json({
        error: `Too many failed sign-ins for ${who}. Try again ${waitPhrase(gate.retryAfterMs)} — ${def.label} is counting these too.`,
        retryAfterMs: gate.retryAfterMs,
      }, { status: 429, headers: { 'Retry-After': String(Math.ceil(gate.retryAfterMs / 1000)) } });
    }
  }

  const check = await verifyImap(def, email, pass);
  if (!check.ok) {
    failed(byUser, LINK_ATTEMPTS);
    failed(byAddr, ADDRESS_ATTEMPTS);
    return NextResponse.json({ error: check.error });
  }
  succeeded(byUser); succeeded(byAddr);

  let sealed;
  try {
    sealed = sealRow({
      user_id: user.id, provider: def.token, label: email,
      access: pass, refresh: null, expires_at: null,
      updated_at: new Date().toISOString(),
    });
  } catch (e) { return NextResponse.json({ error: e.message }); }

  const { error: e2 } = await sb.from('provider_tokens').upsert(sealed);
  if (e2) return NextResponse.json({ error: 'Could not save the link — ' + e2.message });

  return NextResponse.json({ ok: true, label: email, provider });
}
