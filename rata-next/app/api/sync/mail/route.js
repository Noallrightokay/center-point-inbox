import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../../lib/server';
import { fetchInbox, isMailKey, mailKey, describe, candidateHosts } from '../../../../lib/mail';

export const dynamic = 'force-dynamic';

/* Pull every linked mailbox into one stream.

   The combined inbox is the product, so the default is all of them, merged and
   newest first. ?email= narrows it to one account for a per-inbox refresh.

   One mailbox failing must not lose the others: each account reports its own
   error alongside whatever did arrive. */
export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const only = String(new URL(req.url).searchParams.get('email') || '').trim().toLowerCase();

  let q = sb.from('provider_tokens').select('provider,label,access,extra').eq('user_id', user.id);
  if (only) q = q.eq('provider', mailKey(only));
  const { data: rows } = await q;

  const mailboxes = (rows || []).filter(r => isMailKey(r.provider));
  if (!mailboxes.length) {
    return NextResponse.json({ error: only ? 'That mailbox is not linked' : 'No mailbox linked yet' });
  }

  const results = await Promise.all(mailboxes.map(async r => {
    const host = r.extra?.host || candidateHosts(r.label)[0];
    const label = r.extra?.provider_label || describe(r.label)?.label || host;
    const out = await fetchInbox({ host, email: r.label, pass: r.access, label });
    return { email: r.label, label, ...out };
  }));

  const messages = results.flatMap(r => r.messages || []).sort((a, b) => b.ts - a.ts);
  const failures = results.filter(r => r.error).map(r => ({ email: r.email, error: r.error }));

  return NextResponse.json({
    messages,
    accounts: results.map(r => ({ email: r.email, label: r.label, count: (r.messages || []).length, error: r.error || null })),
    ...(failures.length ? { partial: failures } : {}),
    ...(failures.length === results.length ? { error: failures[0].error } : {}),
  });
}
