import { NextResponse } from 'next/server';
import { userFromRequest } from '../../../lib/server';
import { isMailKey } from '../../../lib/mail';
import { planForUser, countLinks, planDef, bucketOf, UNLIMITED } from '../../../lib/plan';

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

  /* JSON has no Infinity, so an unlimited allowance travels as null and the
     client reads that as "as many as you have" rather than as zero. */
  const cap = n => (n === UNLIMITED ? null : n);

  return NextResponse.json({
    plan,
    planLabel: def.label,
    price: def.price,
    limits: { mail: cap(def.mail), chat: cap(def.chat) },
    features: { split: def.split, translate: def.translate, convert: def.convert,
                ai: def.ai, files: def.files, crm: def.crm, sms: def.sms, automations: def.automations },
    used,
    remaining: {
      mail: def.mail === UNLIMITED ? null : Math.max(0, def.mail - used.mail),
      chat: def.chat === UNLIMITED ? null : Math.max(0, def.chat - used.chat),
    },
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
