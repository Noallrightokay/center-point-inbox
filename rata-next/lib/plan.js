/* ---------------------------------------------------------------------------
   What each plan includes.

   There is no free tier. An account without an active subscription is not on a
   cheaper plan — it is unsubscribed, and the difference matters: it should be
   told what RATA costs, not handed a stripped-down product and left to think
   that is what RATA is.

   Three plans, and each one is defined by the thing it makes possible:

     Base        up to two mailboxes, arriving in one Center Point inbox,
                 translation when asked, with the Format Bridge
     Pro         more than two mailboxes, and the option to put them side by
                 side, with summaries and the briefing
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
    price: 12.99,
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
    /* RATA-hosted addresses, which do not exist: RATA hosts no mail. Every
       plan connects the mailboxes you already have. */
    ratamail: 0,
    domains: 0,
    sellable: true,
    blurb: 'Up to two mailboxes in one inbox, translation of any message when you ask, and documents and attachments converted between formats.',
  },
  pro: {
    label: 'RATA Pro',
    price: 23.99,
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
    /* A flag for addresses at mailrata.org, which RATA does not host and
       nothing reads. Kept as it was so no behaviour changes; no sentence may
       offer it (tests/app.test.mjs checks the blurb). */
    ratamail: UNLIMITED,
    domains: 0,
    sellable: true,
    blurb: 'Everything in Base, with as many mailboxes as you have, the option to view them side by side, summaries, and a briefing of what needs you.',
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
    /* Not on sale.

       Enterprise is the only tier whose headline features — the CRM, texts and
       automations — are flags nothing in the app reads yet. Selling it would
       mean taking $72 a month for three things that do not exist, so it stays
       defined (the shape is settled, and a webhook still has to understand the
       price if one is ever created) and stays out of every checkout, pricing
       page and upgrade prompt until the features are real.

       Flipping this to true is the whole change when they are. */
    sellable: false,
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
  blurb: 'Choose a plan to connect a mailbox. RATA Base is $12.99 a month.',
};

/* Mail on your own domain, for a plan that does not include it.

   RATA hosts no mail, at mailrata.org or anywhere else. Hosting mail on a
   customer's own domain would be a different amount of work — their MX would
   have to point here, and their mail breaking would become RATA's support
   call — so it would be charged for rather than folded in. Enterprise, not on
   sale, would include as many as they like.

   Priced per domain and per month, so somebody with three domains pays for
   three. Stripe carries it as a quantity on the same subscription. */
/* $12.99 and $72, not $12.99000001 and $72.00 — and never $12.9, which is
   what raw interpolation gives for a price ending in a zero. Written once so
   no price in the product can be quoted with a digit missing. */
export function money(n) {
  return Number.isInteger(n) ? `$${n}` : `$${n.toFixed(2)}`;
}

/* Priced, and not on sale. RATA-hosted mail does not exist, so
   STRIPE_PRICE_DOMAIN stays unset (LAUNCH.md, STRIPE-SETUP.md), and nothing
   may quote this price while it is: every sentence that would offer the add-on
   asks domainAddonOnSale() first. */
export const DOMAIN_ADDON = {
  price: 1.5,
  label: 'Your own domain',
  blurb: 'Host mail on a domain you own, you@yourcompany.com, read in RATA beside your other mailboxes.',
};

export const ORDER = ['base', 'pro', 'enterprise'];

/* The plans somebody can actually buy today, in order. Every price list,
   checkout button and "upgrade to…" sentence reads this rather than ORDER —
   pointing a customer at a plan with no way to buy it is worse than not
   mentioning it. */
export const SELLABLE = ORDER.filter(p => PLANS[p].sellable);
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
  /* Sellable only: "upgrade to Enterprise" is not advice while Enterprise
     cannot be bought — it is a dead end with a price on it. When nothing
     available lifts the limit, the refusal says what the limit is and stops,
     which is the honest version. */
  return ORDER.slice(from + 1).find(p => PLANS[p].sellable && PLANS[p][bucket] > cap) || null;
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
    return `Choose a plan to connect a ${one}. RATA Base is ${money(PLANS.base.price)} a month and includes up to ${PLANS.base.mail} ${many} in one RATA inbox.`;
  }

  const up = nextFor(plan, bucket);
  const lift = up
    ? ` ${PLANS[up].label} (${money(PLANS[up].price)}/month) includes ${PLANS[up][bucket] === UNLIMITED ? 'more than that' : PLANS[up][bucket]} — upgrade in Settings.`
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

/* Whether the domain add-on can actually be bought: only once its Stripe price
   is configured. The same variable decides whether the webhook grants a domain
   at all (domainAddonsOf in lib/stripe.js), so an offer and the ability to
   honour it cannot come apart. */
export function domainAddonOnSale(env = typeof process === 'undefined' ? {} : process.env) {
  return !!String(env?.STRIPE_PRICE_DOMAIN || '').trim();
}

/* What to say while the add-on is not on sale: the fact, with no price and
   nothing to buy. Mail on your own domain that a provider hosts is a different
   thing, and RATA does connect it. */
export const NO_DOMAIN_HOSTING = 'RATA does not host mail on your own domain. A mailbox on your domain that your provider hosts can be linked like any other.';

/* Null when another domain is allowed, or the sentence to show when it is not.

   This is deliberately not `refusal()`. Every other limit in RATA is lifted by
   moving up a plan, and saying "upgrade to Enterprise" to a Pro member who
   wants one custom domain would be both wrong and expensive — the answer there
   is $1.50, not $56, once the add-on is on sale. Until then the answer is
   that RATA does not do it. */
export function domainRefusal(plan, used, purchased = 0, env) {
  const cap = domainsAllowed(plan, purchased);
  if (used < cap) return null;
  /* Never an offer of something that cannot be bought. */
  if (!domainAddonOnSale(env)) return NO_DOMAIN_HOSTING;

  if (!PLANS[plan]) {
    return `Choose a plan to host mail on your own domain. ${PLANS.pro.label} (${money(PLANS.pro.price)} a month) can add one for ${money(DOMAIN_ADDON.price)} a month.`;
  }
  if (plan === 'enterprise') return null;   // unlimited; the cap is never reached

  const have = purchased
    ? `You are hosting ${used} domain${used === 1 ? '' : 's'}.`
    : `${planDef(plan).label} does not include mail on your own domain.`;
  const alt = PLANS.enterprise.sellable
    ? ` Or ${PLANS.enterprise.label} (${money(PLANS.enterprise.price)}/month) includes as many as you like.`
    : '';
  return `${have} Add ${purchased ? 'another' : 'one'} for ${money(DOMAIN_ADDON.price)} a month.${alt}`;
}

/* Stripe's statuses, reduced to the question every route asks: is this
   person entitled right now? past_due is deliberately still entitled: a card
   that failed this morning should not lock someone out of their mail while
   Stripe retries it, which is Stripe's own grace period. It is safe because a
   licence lasts 30 days and is renewed only while this holds, so once Stripe
   gives up (canceled or unpaid) renewal stops and the licence runs out.

   Kept here, not in lib/stripe.js, so this file stays free of Node imports;
   the webhook re-exports it, and the two can no longer disagree (they did:
   the licence routes accepted only active and trialing). */
export const LIVE_STATUSES = ['active', 'trialing', 'past_due'];

const entitled = (status) => LIVE_STATUSES.includes(String(status || '').toLowerCase());

/* Whether a row is a purchase still being set up: `incomplete` (the webhook's
   PENDING) and tied to a Stripe customer, so a checkout event wrote it. Its
   plan has not been named yet, or its money has not cleared (a bank
   transfer). Not entitled, but not "no subscription" either: the licence
   page says "setting up" for it and offers no second checkout. A row with
   no customer was not written by a checkout and gets no such answer. */
export function settingUp(row) {
  return !!(row && row.stripe_customer && String(row.status || '').toLowerCase() === 'incomplete');
}

/* The plan and everything bought alongside it, read from the subscriptions
   table the Stripe webhook maintains. No live subscription is no plan — not a
   lesser one. `pending` is true for a purchase still being set up
   (settingUp), which is no plan yet. */
export async function entitlementsForUser(sb, email) {
  if (!sb || !email) return { plan: null, domainAddons: 0 };
  const { data } = await sb.from('subscriptions')
    .select('plan,status,domain_addons,stripe_customer').eq('email', String(email).toLowerCase()).maybeSingle();
  if (!data) return { plan: null, domainAddons: 0 };
  if (!entitled(data.status)) return { plan: null, domainAddons: 0, pending: settingUp(data) };
  return {
    plan: PLANS[data.plan] ? data.plan : 'base',
    domainAddons: Math.max(0, Number(data.domain_addons) || 0),
  };
}
