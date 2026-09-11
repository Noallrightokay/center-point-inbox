import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../lib/server';
import { isMailKey } from '../../../lib/mail';
import { planForUser, countLinks, planDef, bucketOf } from '../../../lib/plan';

export const dynamic = 'force-dynamic';

/* What this account has linked, and what its plan still allows.

   The client needs the allowance before the user tries to exceed it — a cap
   discovered only when the "Connect" button fails is a cap that feels like a
   bug. Passwords never leave the server; only the address and the server name
   are returned. */
export async function GET(req) {
  const { user, sb, error } = await userFromRequest(req);
  if (error) return NextResponse.json({ error });

  const { data: rows } = await sb.from('provider_tokens')
    .select('provider,label,extra,updated_at').eq('user_id', user.id);

  const plan = await planForUser(sb, user.email);
  const def = planDef(plan);
  const used = countLinks(rows);

  return NextResponse.json({
    plan,
    planLabel: def.label,
    limits: { mail: def.mail, chat: def.chat, files: def.files },
    used,
    remaining: { mail: Math.max(0, def.mail - used.mail), chat: Math.max(0, def.chat - used.chat) },
    links: (rows || []).map(r => ({
      provider: r.provider,
      bucket: bucketOf(r.provider),
      kind: isMailKey(r.provider) ? 'mail' : r.provider,
      label: r.label,
      host: r.extra?.host || null,
      providerLabel: r.extra?.provider_label || null,
      updatedAt: r.updated_at,
    })),
  });
}
