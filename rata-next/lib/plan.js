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
    /* RATA-hosted addresses. Base connects the mailboxes you already have; it
       does not hand out new ones. */
    ratamail: 0,
    domains: 0,
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
    /* Addresses at mailrata.org, hosted by RATA rather than connected from
       somewhere else. Their own domain is the paid add-on below. */
    ratamail: UNLIMITED,
    domains: 0,
    blurb: 'Everything in Base, with more than two mailboxes, the option to view them side by side, summaries of what arrived, and your own @mailrata.org addresses.',
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
    ratamail: UNLIMITED,
    domains: UNLIMITED,
    blurb: 'Everything in Pro, plus mail on as many of your own domains as you like, the CRM, texts, automations and a shared view across the team.',
  },
};

/* Not a plan. What an account is before it has one. */
export const NO_PLAN = {
  label: 'No plan yet',
  price: 0,
  mail: 0, chat: 0,
  split: false, translate: false, convert: false, ai: false,
  files: false, crm: false, sms: false, automations: false,
  ratamail: 0, domains: 0,
  blurb: 'Choose a plan to connect a mailbox. RATA Base is $8 a month.',
};

/* Mail on your own domain, for a plan that does not include it.

   Pro gets addresses at mailrata.org. Hosting mail on a customer's own domain
   is a different amount of work — their MX has to point here, and their mail
   breaking becomes RATA's support call — so it is charged for rather than
   folded in. Enterprise includes as many as they like.

   Priced per domain and per month, so somebody with three domains pays for
   three. Stripe carries it as a quantity on the same subscription. */
/* $8 and $1.50, not $8.00 and $1.5. Written once so no price in the product
   can be quoted with a stray digit missing. */
export function money(n) {
  return Number.isInteger(n) ? `$${n}` : `$${n.toFixed(2)}`;
}

export const DOMAIN_ADDON = {
  price: 1.5,
  label: 'Your own domain',
  blurb: 'Host mail on a domain you own — you@yourcompany.com, arriving in the same Center Point inbox.',
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

/* How many domains this account may host mail on: what the plan includes, plus
   what it has bought. */
export function domainsAllowed(plan, purchased = 0) {
  const included = planDef(plan).domains;
  if (included === UNLIMITED) return UNLIMITED;
  return included + Math.max(0, Number(purchased) || 0);
}

/* Null when another domain is allowed, or the sentence to show when it is not.

   This is deliberately not `refusal()`. Every other limit in RATA is lifted by
   moving up a plan, and saying "upgrade to Enterprise" to a Pro member who
   wants one custom domain would be both wrong and expensive — the answer there
   is $1.50, not $56. */
export function domainRefusal(plan, used, purchased = 0) {
  const cap = domainsAllowed(plan, purchased);
  if (used < cap) return null;

  if (!PLANS[plan]) {
    return `Choose a plan to host mail on your own domain. ${PLANS.pro.label} (${money(PLANS.pro.price)} a month) can add one for ${money(DOMAIN_ADDON.price)} a month.`;
  }
  if (plan === 'enterprise') return null;   // unlimited; the cap is never reached

  const have = purchased
    ? `You are hosting ${used} domain${used === 1 ? '' : 's'}.`
    : `${planDef(plan).label} includes addresses at mailrata.org rather than mail on your own domain.`;
  return `${have} Add ${purchased ? 'another' : 'one'} for ${money(DOMAIN_ADDON.price)} a month, or ${PLANS.enterprise.label} (${money(PLANS.enterprise.price)}/month) includes as many as you like.`;
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

/* The plan and everything bought alongside it. Separate from planForUser so the
   existing callers, which only ask "which tier", keep their shape. */
export async function entitlementsForUser(sb, email) {
  if (!sb || !email) return { plan: null, domainAddons: 0 };
  const { data } = await sb.from('subscriptions')
    .select('plan,status,domain_addons').eq('email', String(email).toLowerCase()).maybeSingle();
  if (!data) return { plan: null, domainAddons: 0 };
  const live = ['active', 'trialing'].includes(String(data.status || '').toLowerCase());
  if (!live) return { plan: null, domainAddons: 0 };
  return {
    plan: PLANS[data.plan] ? data.plan : 'base',
    domainAddons: Math.max(0, Number(data.domain_addons) || 0),
  };
}
