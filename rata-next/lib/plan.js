/* ---------------------------------------------------------------------------
   What a plan includes.

   A link is a live connection to somebody else's service — a mailbox, a Slack
   workspace — and each one costs RATA a sync. So links are what the plans are
   sold by, and the limits live here rather than being scattered through the UI:
   the server refuses an over-limit link, and the client reads the same numbers
   to explain the cap before the user hits it.

   Counting rule: mail accounts and chat links are counted separately, so
   connecting a second mailbox never uses up the Slack allowance.
   --------------------------------------------------------------------------- */

export const PLANS = {
  free: {
    label: 'Free preview',
    mail: 1,
    chat: 0,
    files: false,        // the business file library
    blurb: 'One mailbox, so you can see what RATA does with your own mail.',
  },
  base: {
    label: 'RATA Base',
    mail: 2,
    chat: 1,
    files: false,
    blurb: 'Two mailboxes and one Slack workspace.',
  },
  business: {
    label: 'RATA Business',
    mail: 10,
    chat: 5,
    files: true,
    blurb: 'Ten mailboxes, five chat workspaces, and the shared file library.',
  },
};

export const CHAT_TYPES = ['slack', 'discord', 'phone'];

export function planDef(plan) {
  return PLANS[plan] || PLANS.free;
}

/* Which allowance a link draws from. Mail is mail whichever provider serves
   it — the old per-provider tokens count too, or upgrading would silently
   drop somebody's existing mailbox. */
export function bucketOf(provider) {
  if (!provider) return null;
  if (provider.startsWith('mail:')) return 'mail';
  if (['gmail_imap', 'apple', 'ms', 'google'].includes(provider)) return 'mail';
  if (CHAT_TYPES.includes(provider)) return 'chat';
  return null;
}

export function countLinks(rows) {
  const n = { mail: 0, chat: 0 };
  for (const r of rows || []) {
    const b = bucketOf(r.provider);
    if (b) n[b]++;
  }
  return n;
}

/* Returns null when the link is allowed, or the sentence to show when it is
   not. The message names the limit and the plan that lifts it, because "limit
   reached" on its own tells the user nothing they can act on. */
export function refusal(plan, bucket, used) {
  const def = planDef(plan);
  const cap = def[bucket];
  if (used < cap) return null;

  const what = bucket === 'mail' ? 'mailbox' : 'chat workspace';
  const plural = bucket === 'mail' ? 'mailboxes' : 'chat workspaces';
  const next = Object.entries(PLANS).find(([name, p]) => p[bucket] > cap && name !== plan);

  if (cap === 0) {
    return `${def.label} does not include ${plural}.` + (next ? ` ${PLANS[next[0]].label} includes ${PLANS[next[0]][bucket]}.` : '');
  }
  return `${def.label} includes ${cap} ${cap === 1 ? what : plural}, and ${cap === 1 ? 'it is' : 'they are'} in use.`
    + (next ? ` ${PLANS[next[0]].label} includes ${PLANS[next[0]][bucket]} — upgrade in Settings to add another.` : '');
}

/* The user's plan, read from the subscriptions table the Stripe webhook
   maintains. Anything not actively paid for is the free preview. */
export async function planForUser(sb, email) {
  if (!sb || !email) return 'free';
  const { data } = await sb.from('subscriptions')
    .select('plan,status').eq('email', String(email).toLowerCase()).maybeSingle();
  if (!data) return 'free';
  const live = ['active', 'trialing'].includes(String(data.status || '').toLowerCase());
  if (!live) return 'free';
  return PLANS[data.plan] ? data.plan : 'base';
}
