/* ---------------------------------------------------------------------------
   What each plan includes.

   There is no free tier. An account without an active subscription is not on a
   cheaper plan — it is unsubscribed, and the difference matters: it should be
   told what RATA costs, not handed a stripped-down product and left to think
   that is what RATA is.

   Three plans, and each one is defined by the thing it makes possible:

     Base        up to two mailboxes, arriving in one Center Point inbox,
                 translated, with the Format Bridge
     Pro         more than two mailboxes, and the option to put them side by
                 side, with summaries
     Enterprise  the CRM, texts and automations on top

   The Center Point inbox is not the limitation at the bottom of the ladder —
   it is the product. Every mailbox lands in one place, on every plan. What
   Pro adds is the choice to pull them apart again when that helps.

   The limits live here and nowhere else. The server refuses an over-limit
   link, and the client reads the same numbers to explain a cap before anyone
   runs into it — so the two cannot drift apart.
   --------------------------------------------------------------------------- */

export const UNLIMITED = Infinity;

export const PLANS = {
  base: {
    label: 'RATA Base',
    price: 8,
    mail: 2,
    chat: 0,
    /* One Center Point inbox: both mailboxes in the same stream. Pulling them
       apart into columns is what Pro adds. */
    split: false,
    translate: true,
    convert: true,
    ai: false,
    files: false,
    crm: false,
    sms: false,
    automations: false,
    blurb: 'Up to two mailboxes in one Center Point inbox, translated as they arrive, and any file converted to any format.',
  },
  pro: {
    label: 'RATA Pro',
    price: 16,
    mail: UNLIMITED,
    chat: 3,
    split: true,
    translate: true,
    convert: true,
    ai: true,
    files: true,
    crm: false,
    sms: false,
    automations: false,
    blurb: 'Everything in Base, with more than two mailboxes, the option to view them side by side, and summaries of what arrived.',
  },
  enterprise: {
    label: 'RATA Enterprise',
    price: 72,
    mail: UNLIMITED,
    chat: UNLIMITED,
    split: true,
    translate: true,
    convert: true,
    ai: true,
    files: true,
    crm: true,
    sms: true,
    automations: true,
    blurb: 'Everything in Pro, plus the CRM, texts, automations and a shared view across the team.',
  },
};

/* Not a plan. What an account is before it has one. */
export const NO_PLAN = {
  label: 'No plan yet',
  price: 0,
  mail: 0, chat: 0,
  split: false, translate: false, convert: false, ai: false,
  files: false, crm: false, sms: false, automations: false,
  blurb: 'Choose a plan to connect a mailbox. RATA Base is $8 a month.',
};

export const ORDER = ['base', 'pro', 'enterprise'];
export const CHAT_TYPES = ['slack', 'discord', 'phone'];

export function planDef(plan) {
  return PLANS[plan] || NO_PLAN;
}

export function has(plan, feature) {
  return !!planDef(plan)[feature];
}

/* Which allowance a link draws from. Mail is mail whichever provider serves
   it — the older per-provider tokens count too, or changing plan would
   silently drop somebody's existing mailbox. */
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

/* The next plan up that lifts this particular limit, or null at the top. */
export function nextFor(plan, bucket) {
  const from = ORDER.indexOf(plan);
  const cap = planDef(plan)[bucket];
  return ORDER.slice(from + 1).find(p => PLANS[p][bucket] > cap) || null;
}

/* Returns null when the link is allowed, or the sentence to show when it is
   not — naming the limit and what lifts it, because "limit reached" on its own
   tells the user nothing they can act on. */
export function refusal(plan, bucket, used) {
  const def = planDef(plan);
  const cap = def[bucket];
  if (used < cap) return null;

  const one = bucket === 'mail' ? 'mailbox' : 'chat workspace';
  const many = bucket === 'mail' ? 'mailboxes' : 'chat workspaces';

  if (!PLANS[plan]) {
    return `Choose a plan to connect a ${one}. RATA Base is $${PLANS.base.price} a month and includes up to ${PLANS.base.mail} ${many} in one Center Point inbox.`;
  }

  const up = nextFor(plan, bucket);
  const lift = up
    ? ` ${PLANS[up].label} ($${PLANS[up].price}/month) includes ${PLANS[up][bucket] === UNLIMITED ? 'more than that' : PLANS[up][bucket]} — upgrade in Settings.`
    : '';

  if (cap === 0) return `${def.label} does not include ${many}.${lift}`;
  return `${def.label} includes up to ${cap} ${cap === 1 ? one : many}, and ${cap === 1 ? 'it is' : 'they are'} in use.${lift}`;
}

/* The user's plan, read from the subscriptions table the Stripe webhook
   maintains. No active subscription is no plan — not a lesser one. */
export async function planForUser(sb, email) {
  if (!sb || !email) return null;
  const { data } = await sb.from('subscriptions')
    .select('plan,status').eq('email', String(email).toLowerCase()).maybeSingle();
  if (!data) return null;
  const live = ['active', 'trialing'].includes(String(data.status || '').toLowerCase());
  if (!live) return null;
  return PLANS[data.plan] ? data.plan : 'base';
}
