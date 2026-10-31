/* The privacy policy and the terms of service (MVP-PLAN card H1).

   They are visible promises, so what is checked here is that each one is
   still what the code does: the deletion rule and its 30 days, the licence's
   length and renewal window, the prices and what each plan includes, what
   the AI relay is sent, what the online account keeps. When one of those
   changes in the code, a check here fails and the page has to be changed
   with it. Also: both pages are served and linked from the three places a
   customer meets them, and the only unknowns left are the owner's four
   placeholders, which live-check.sh refuses on the live site. */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startServer, makeChecker } from './helpers.mjs';
import { LICENCE_DAYS, RENEW_GRACE_DAYS } from '../lib/licence.js';
import { blocksDeletion, DELETION_REMOVES, DELETION_KEEPS } from '../lib/account.js';
import { PLANS, SELLABLE, ORDER, UNLIMITED, LIVE_STATUSES, money } from '../lib/plan.js';
import { LIMITS, validate } from '../lib/ai.js';
import { APPLE_PRODUCTS } from '../lib/plan.js';
import * as apple from '../lib/apple.js';

const HERE = dirname(fileURLToPath(import.meta.url));
const SITE = join(HERE, '..');
const REPO = join(SITE, '..');

/* The four forms, exactly. Anything else in brackets starting OWNER is a
   placeholder nobody told the owner about. */
const OWNER_FORMS = [
  '[OWNER: legal entity name]',
  '[OWNER: postal address]',
  '[OWNER: contact email]',
  '[OWNER: governing law]',
];

const ENT = { rsquo: '’', lsquo: '‘', ldquo: '“', rdquo: '”', amp: '&', lt: '<', gt: '>', nbsp: ' ', quot: '"' };
/* What a reader sees: no styles, scripts, comments or tags, entities
   decoded, apostrophes made plain so a sentence written once in code and
   once in HTML can be compared. */
function visible(html) {
  return html
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<script[\s\S]*?<\/script>/gi, ' ')
    .replace(/<!--[\s\S]*?-->/g, ' ')
    .replace(/<[^>]+>/g, ' ')
    .replace(/&(\w+);/g, (m, n) => ENT[n] ?? m)
    .replace(/[‘’]/g, "'")
    .replace(/\s+/g, ' ');
}
const plain = s => String(s).replace(/[‘’]/g, "'");
const hrefs = html => [...html.matchAll(/<a\b[^>]*\bhref="([^"]*)"/gi)].map(m => m[1]);
/* A link to one of the two pages: relative on the website, or the whole
   mailrata.org address where the page is also shipped in the desktop app. */
const linksTo = (html, page) => hrefs(html).some(h => h === page || h === '/' + page || h === 'https://mailrata.org/' + page);
const WORDS = { 1: 'one', 2: 'two', 3: 'three', 4: 'four', 5: 'five' };
const thousands = n => n.toLocaleString('en-US');

export default async function run(state) {
  const check = makeChecker(state);
  const s = await startServer();
  let privacy, terms, index, auth, account;
  try {
    console.log('\n— both pages are served, each the page it says —');
    for (const [path, h1] of [['/privacy.html', 'Privacy policy'], ['/terms.html', 'Terms of service']]) {
      const r = await fetch(s.url + path);
      const html = await r.text();
      check(r.status === 200, `${path} -> ${r.status}`);
      check(html.includes(`<h1>${h1}</h1>`), `${path} is the ${h1}`);
      if (path === '/privacy.html') privacy = html; else terms = html;
    }
    index = await (await fetch(s.url + '/')).text();
    auth = await (await fetch(s.url + '/auth')).text();
    account = await (await fetch(s.url + '/account')).text();
  } finally { await s.stop(); }
  if (!privacy || !terms) return;
  const P = visible(privacy), T = visible(terms);

  console.log('\n— linked from the three places a customer meets them —');
  {
    const footer = (index.match(/<footer>[\s\S]*?<\/footer>/) || [''])[0];
    check(linksTo(footer, 'privacy.html') && linksTo(footer, 'terms.html'), 'the landing page footer links both');
    const signup = (auth.match(/<div id="signup">[\s\S]*?<div class="foot"/) || [''])[0];
    const agree = (signup.match(/<p class="agree"[\s\S]*?<\/p>/) || [''])[0];
    check(/By creating an account you agree to/.test(visible(agree)), `sign-up says "By creating an account you agree to…": "${visible(agree).trim()}"`);
    check(linksTo(agree, 'terms.html') && linksTo(agree, 'privacy.html'), 'beside the sign-up button, linking both');
    /* auth.html is copied into the desktop app, which has neither page. */
    check(hrefs(agree).every(h => h.startsWith('https://mailrata.org/')),
      `and by whole mailrata.org addresses, which the desktop app opens in the browser: ${hrefs(agree).join(', ')}`);
    const foot = (account.match(/<div class="foot">[\s\S]*?<\/div>/) || [''])[0];
    check(linksTo(foot, 'privacy.html') && linksTo(foot, 'terms.html'), 'the licence page links both');
    check(linksTo(privacy, 'terms.html') && linksTo(terms, 'privacy.html'), 'and each links the other');
  }

  console.log('\n— the only unknowns are the owner\'s four placeholders —');
  for (const [name, html] of [['privacy', privacy], ['terms', terms]]) {
    const all = html.match(/OWNER/g) || [];
    const forms = html.match(/\[OWNER:[^\]]*\]/g) || [];
    const strays = forms.filter(f => !OWNER_FORMS.includes(f));
    check(strays.length === 0, `${name}: no placeholder but the four (${strays.join(', ') || 'none other'})`);
    check(all.length === forms.length, `${name}: no half-written placeholder (${all.length} OWNER, ${forms.length} in the form)`);
  }
  {
    const used = new Set([...(privacy + terms).matchAll(/\[OWNER:[^\]]*\]/g)].map(m => m[0]));
    /* Once the owner has filled them in there are none, and that is fine;
       until then, each fact only the owner knows is marked, not guessed. */
    check(used.size === 0 || OWNER_FORMS.every(f => used.has(f)),
      `every owner fact is marked until filled in: ${[...used].join(', ') || 'all filled in'}`);
  }

  console.log('\n— in DESIGN.md\'s voice, and making no third-party request —');
  for (const [name, html, text] of [['privacy', privacy, P], ['terms', terms, T]]) {
    check(!/—/.test(text), `${name}: no em dash in visible copy`);
    check(!/!/.test(text), `${name}: no exclamation mark`);
    check(!/\b(seamless|elevate|unleash)/i.test(text), `${name}: no buzzwords`);
    const loads = [...html.matchAll(/<(?:script|img|link|source|iframe)\b[^>]*\b(?:src|href|srcset)="([^"]*)"/gi)].map(m => m[1]);
    const away = loads.filter(u => /^(https?:)?\/\//i.test(u));
    check(away.length === 0, `${name}: loads nothing from another host (${away.join(', ') || 'nothing'})`);
    check(/@media \(prefers-color-scheme:dark\)/.test(html) && /--tint:#2B4FC7/.test(html), `${name}: DESIGN.md tokens, light and dark`);
  }

  console.log('\n— the licence, as lib/licence.js issues and renews it —');
  check(T.includes(`Each key lasts ${LICENCE_DAYS} days`), `terms: a key lasts LICENCE_DAYS (${LICENCE_DAYS}) days`);
  check(T.includes(`at most ${LICENCE_DAYS} days after it was issued`), 'terms: a key outlives the subscription by at most that');
  check(T.includes(`expired more than ${RENEW_GRACE_DAYS} days ago cannot renew itself`), `terms: renewal ends RENEW_GRACE_DAYS (${RENEW_GRACE_DAYS}) days after expiry`);

  console.log('\n— deletion, as lib/account.js decides it —');
  {
    const removes = DELETION_REMOVES.filter(x => !P.includes(plain(x)));
    /* The last of DELETION_KEEPS is said from inside the app ("this
       device"); the page says it as "your computer". */
    const keeps = DELETION_KEEPS.slice(0, -1).filter(x => !P.includes(plain(x)));
    check(removes.length === 0, `privacy names everything DELETION_REMOVES names (${removes.join('; ') || 'all'})`);
    check(keeps.length === 0 && /this device/.test(DELETION_KEEPS.at(-1)) && P.includes('what the app holds on your computer, until you uninstall it'),
      `and everything DELETION_KEEPS names (${keeps.join('; ') || 'all'})`);
    /* The page describes every live status in words; a status added to
       LIVE_STATUSES without words here fails. */
    const SAID = { active: 'active', trialing: 'in a trial', past_due: 'retrying a failed payment' };
    for (const st of LIVE_STATUSES) {
      check(!!SAID[st] && P.includes(SAID[st]) && !!blocksDeletion({ status: st, plan: 'pro' }),
        `status ${st} blocks deletion, and the page says so ("${SAID[st] || '?'}")`);
    }
    check(!!blocksDeletion({ status: 'incomplete' }) && /While Stripe has not yet confirmed a payment/.test(P),
      'a payment still being confirmed blocks it, and the page says so');
    const now = Date.parse('2026-09-29T12:00:00Z');
    const ago = d => new Date(now - d * 86400000).toISOString();
    check(!!blocksDeletion({ status: 'canceled', event_at: ago(LICENCE_DAYS - 1) }, now)
      && blocksDeletion({ status: 'canceled', event_at: ago(LICENCE_DAYS + 1) }, now) === null,
      `the wait after a subscription ends is LICENCE_DAYS (${LICENCE_DAYS})`);
    check(P.includes(`For ${LICENCE_DAYS} days after a subscription ends`) && T.includes(`for ${LICENCE_DAYS} days after a subscription ends`),
      'and both pages say that number');
    check(blocksDeletion({ status: 'incomplete_expired' }) === null && blocksDeletion(null) === null,
      'nothing else waits, so the pages list nothing else');
  }

  console.log('\n— plans and prices, as lib/plan.js sells them —');
  {
    for (const p of SELLABLE) {
      const line = `${PLANS[p].label}, ${money(PLANS[p].price)} a month`;
      check(T.includes(line), `terms: "${line}"`);
    }
    const quoted = new Set([...(P + T).matchAll(/\$\d+(?:\.\d+)?/g)].map(m => m[0]));
    const sold = new Set(SELLABLE.map(p => money(PLANS[p].price)));
    check([...quoted].every(q => sold.has(q)), `no price but the plans on sale (${[...quoted].join(', ')})`);
    const unsold = ORDER.filter(p => !PLANS[p].sellable);
    check(unsold.every(p => !(P + T).includes(PLANS[p].label.replace('RATA ', ''))),
      `no plan that cannot be bought (${unsold.map(p => PLANS[p].label).join(', ')})`);
    check(WORDS[PLANS.base.mail] && T.includes(`Up to ${WORDS[PLANS.base.mail]} mailboxes`), `Base: up to ${PLANS.base.mail} mailboxes`);
    check(PLANS.pro.mail === UNLIMITED && T.includes('as many mailboxes as you have'), 'Pro: no mailbox limit');
    check(PLANS.pro.split && !PLANS.base.split && T.includes('mailboxes side by side'), 'side by side is Pro');
    check(!!validate({ task: 'translate', text: 'x', to: 'fr' }, PLANS.base).task && validate({ task: 'summarize', text: 'x' }, PLANS.base).status === 403
      && !!validate({ task: 'summarize', text: 'x' }, PLANS.pro).task && !!validate({ task: 'tasks', messages: [{ id: 'a', text: 'x' }] }, PLANS.pro).task,
      'the relay allows translation on Base and summaries and the briefing only on Pro');
    check(T.includes('Translation is on Base and Pro; summaries and the briefing are on Pro'), 'and the terms say the same');
  }

  console.log('\n— what the AI relay is sent, as lib/ai.js and app.html send it —');
  {
    const app = readFileSync(join(SITE, 'public', 'app.html'), 'utf8');
    const brief = app.match(/const BRIEF_MAX=(\d+),BRIEF_DAYS=(\d+);/);
    const max = brief && Number(brief[1]), days = brief && Number(brief[2]);
    check(max === LIMITS.briefing && P.includes(`up to ${max} of the most recent messages in your inbox from the last ${days} days`),
      `the briefing: up to ${max} messages (relay takes ${LIMITS.briefing}) from the last ${days} days`);
    check((app.includes(`.slice(0,${LIMITS.briefingEach})`) || app.includes(`AI_BRIEF_EACH=${LIMITS.briefingEach}`)) && P.includes(`the first ${thousands(LIMITS.briefingEach)} characters`),
      `the start of each: ${thousands(LIMITS.briefingEach)} characters`);
    check(app.includes(`AI_TEXT=${LIMITS.text},`) && P.includes(`up to the first ${thousands(LIMITS.text)} characters`), `a summary or translation: up to ${thousands(LIMITS.text)} characters, and the page cuts to exactly that`);
    /* The home page's privacy column says the same, in fewer words (BUG-D:
       it once said only one message ever reached the relay, and that the
       site kept only an email, a plan and a key). */
    const home = visible(readFileSync(join(SITE, 'public', 'index.html'), 'utf8'));
    check(home.includes(`the first ${thousands(LIMITS.briefingEach)} characters of up to ${max} recent messages in your inbox from the last ${days} days`),
      'the home page gives the briefing the same numbers');
    check(home.includes("Your preferences, if you use RATA's page here") && home.includes('The desktop app uploads none of this'),
      'and names the synced preferences, which the desktop app does not upload');
    check(home.includes('Cancel any time in the billing portal, which you reach from Settings when signed in at mailrata.org')
      && T.includes('in the billing portal, which you reach from Settings when signed in at mailrata.org'),
      'and cancelling is where the terms say: the billing portal, from Settings');
    /* What the online account keeps of someone's preferences, as app.html
       sends it. A field added to either list changes the page. */
    const fields = app.match(/const CLOUD_FIELDS=(\[[^\]]*\]);/);
    const strip = app.match(/const CLOUD_STRIP=(\{[^;]*\});/);
    const f = fields && JSON.parse(fields[1].replace(/'/g, '"'));
    const st = strip && JSON.parse(strip[1].replace(/'/g, '"').replace(/(\w+):/g, '"$1":'));
    check(JSON.stringify(f) === JSON.stringify(['settings', 'folders', 'rules', 'linked']),
      `synced preferences are settings, folders, rules and linked mailboxes: ${JSON.stringify(f)}`);
    check(st && ['api', 'sigs'].every(k => st.settings.includes(k)) && P.includes('Signatures and API keys are left out'),
      'signatures and API keys are left out, and the page says so');
    const sync = readFileSync(join(REPO, 'desktop', 'rata-app', 'sync-ui.sh'), 'utf8');
    check(/supabaseUrl: '',/.test(sync) && P.includes('The desktop app does not upload any of this'),
      'the desktop build has no online account to upload to, and the page says so');
    /* "This is all of it": every table the site creates is one the page
       describes, and the AI usage table holds no text. */
    const sql = readFileSync(join(SITE, 'database.sql'), 'utf8');
    const tables = [...sql.matchAll(/create table if not exists public\.(\w+)/g)].map(m => m[1]).sort();
    check(JSON.stringify(tables) === JSON.stringify(['ai_usage', 'apple_subscriptions', 'pending_subscriptions', 'subscriptions', 'workspaces']),
      `the site keeps five tables, the ones the page lists: ${tables.join(', ')}`);
    /* pending_subscriptions (BUG-S) holds a subscription event that arrived
       before its checkout: keyed by the Stripe customer, never an address. */
    const kept = (sql.match(/create table if not exists public\.pending_subscriptions \(([\s\S]*?)\n\);/) || ['', ''])[1];
    const keptCols = [...kept.matchAll(/^\s*(\w+)\s+(?:text|bigint|integer|timestamptz)/gm)].map(m => m[1]);
    check(JSON.stringify(keptCols) === JSON.stringify(['stripe_customer', 'plan', 'domain_addons', 'status', 'event_at', 'updated_at']),
      `a subscription event kept before its checkout is a customer id, a plan, a status and a time, no address: ${keptCols.join(', ')}`);
    check(P.includes('kept under your Stripe customer id alone') && P.includes('no email address'),
      'and the page says so, under the subscription record');
    const usage = (sql.match(/create table if not exists public\.ai_usage \(([\s\S]*?)\n\);/) || ['', ''])[1];
    const cols = [...usage.matchAll(/^\s*(\w+)\s+(?:text|bigint|integer|timestamptz)/gm)].map(m => m[1]);
    check(JSON.stringify(cols) === JSON.stringify(['email', 'month', 'micro_usd', 'requests', 'updated_at']),
      `the AI usage record is an address, a month, a cost and a count, never any text: ${cols.join(', ')}`);
  }

  console.log('\n— the iPhone app\'s subscriptions, as lib/apple.js and database.sql keep them —');
  {
    /* apple_subscriptions (database.sql §7): every column the code writes is
       one the privacy policy names, in the policy's words. A column added
       to the table without words on the page fails here. */
    const sql = readFileSync(join(SITE, 'database.sql'), 'utf8');
    const table = (sql.match(/create table if not exists public\.apple_subscriptions \(([\s\S]*?)\n\);/) || ['', ''])[1];
    const cols = [...table.matchAll(/^\s*(\w+)\s+(?:text|bigint|integer|timestamptz|uuid)/gm)].map(m => m[1]);
    check(JSON.stringify(cols) === JSON.stringify(['original_transaction_id', 'product_id', 'plan', 'environment', 'period_from', 'expires_at',
      'grace_until', 'grace_signed_at', 'revoked_at', 'signed_at', 'app_account_token', 'email', 'updated_at']),
      `an App Store subscription is kept as Apple's number, a product, a plan, an environment and dates: ${cols.join(', ')}`);
    const SAID = {
      original_transaction_id: 'its original transaction id',
      product_id: 'the product and its plan',
      plan: 'the product and its plan',
      environment: 'whether it was bought in Apple\'s test environment',
      period_from: 'when its current period began',
      expires_at: 'when it expires',
      grace_until: 'when a billing grace period ends if a renewal fails',
      revoked_at: 'when it was refunded or revoked',
      signed_at: 'when Apple signed what it last told us about the subscription',
      grace_signed_at: 'and about a grace period',
    };
    const fact = (P.match(/Your App Store subscription record, if you subscribe in the iPhone app.*?No name, no email address and no Apple ID: Apple tells us none of them\./) || [''])[0];
    const unsaid = Object.entries(SAID).filter(([, words]) => !fact.includes(words)).map(([c]) => c);
    check(!!fact && unsaid.length === 0, `privacy names each column it keeps under "What mailrata.org keeps" (${unsaid.join(', ') || 'all'})`);
    /* app_account_token and email are in the table for linking a purchase
       to an account later; the page says no address is kept, so nothing may
       write either yet. */
    const t = { originalTransactionId: '2000000000000001', productId: Object.keys(APPLE_PRODUCTS)[0], expiresDate: Date.now() + 86400e3,
      signedDate: Date.now(), type: 'Auto-Renewable Subscription', bundleId: 'org.mailrata.mail', environment: 'Production' };
    const written = [apple.appleRowOf(t, 'Production'), ...[apple.applePurchase(t, 'Production'), ...apple.APPLE_ACTED.map((type) =>
      apple.appleChange({ notificationType: type, subtype: 'GRACE_PERIOD', signedDate: Date.now() }, t, { gracePeriodExpiresDate: Date.now() }, 'Production'))]
      .flatMap((c) => (c ? [c.core, c.insert, ...(c.grace ? [c.grace] : [])] : []))];
    const keys = new Set(written.flatMap(Object.keys));
    check(written.length > 2 && [...keys].every(k => cols.includes(k) && SAID[k]) && !keys.has('email') && !keys.has('app_account_token'),
      `what the code writes is only what the page names, never an address: ${[...keys].join(', ')}`);
    check(P.includes('the country of the App Store and the price') && P.includes('the country and the price are not kept') && !keys.has('price'),
      'what Apple sends and is not kept is named too');
    check(P.includes('Apple takes the payment and handles billing, renewal, cancellation and refunds'), 'privacy: Apple handles the payment');
    check(P.includes('we keep it, and an AI usage record kept under its number, for as long as RATA sells subscriptions in the iPhone app'),
      'privacy: and how long the record is kept');
    /* A subscriber has no account here and is never shown Apple's
       transaction id, which keys the record, so the rights the page lists
       say how one asks: with the order ID on Apple's receipt, which
       LAUNCH.md section 11 turns into the record through Apple's Look Up
       Order ID. */
    const rights = visible((privacy.match(/<section id="rights"[\s\S]*?<\/section>/) || [''])[0]);
    const launch = readFileSync(join(SITE, 'LAUNCH.md'), 'utf8');
    check(rights.includes('If you subscribed in the iPhone app, you have no account here: write instead with the order ID from any of Apple\'s receipts for your subscription (in Apple\'s email receipt, or your purchase history in the App Store)')
      && /\*\*When a subscriber asks about their record\*\*[\s\S]*?\*Look Up Order ID\*[\s\S]*?`original_transaction_id`[\s\S]*?`apple:<that number>`/.test(launch),
      'privacy: a subscriber in the iPhone app is told how to ask about their record (the order ID), and LAUNCH.md section 11 how to find it');
    check(P.includes('Deleting an account does not cancel it') && T.includes('deleting an account here does not cancel it'),
      'both pages: deleting an account does not cancel an App Store subscription');

    /* The plans sold in the app are plans on sale here, and the terms say
       Apple bills them, with the App Store's own auto-renewal sentence. */
    const sold = [...new Set(Object.values(APPLE_PRODUCTS))];
    check(sold.length === 2 && sold.every(p => SELLABLE.includes(p)) && T.includes('RATA Base and RATA Pro are also sold in RATA\'s iPhone app, as App Store subscriptions'),
      `terms: both plans (${sold.join(', ')}) are also App Store subscriptions`);
    check(T.includes('One bought in the app is billed, renewed, cancelled and refunded by Apple, under Apple\'s terms'),
      'terms: Apple bills, renews, cancels and refunds them, under Apple\'s terms');
    check(T.includes(plain(apple.APPLE_AUTO_RENEWAL)), `terms: the auto-renewal sentence, word for word: "${apple.APPLE_AUTO_RENEWAL.slice(0, 60)}…"`);
    check(T.includes('In the iPhone app there is no key to paste'), 'terms: the iPhone app\'s licence needs no key');
    /* How long an App Store licence lasts (lib/apple.js, appleLicenceUntil):
       the margins are named constants, and both pages state them. */
    const hours = (n) => `${n} hour${n === 1 ? '' : 's'}`;
    /* Until the end of the current period as Apple last reported it, or of
       its billing grace period (appleUntil), plus the margin, and never more
       than LICENCE_DAYS: never "past the period Apple has been paid for",
       which a refund would make untrue (a refund does not shorten a licence
       already issued). */
    const until = 'lasts until the end of the subscription\'s current period as Apple last reported it (or of its billing grace period)';
    check(T.includes(`A licence for a subscription bought in the iPhone app ${until}, plus ${apple.APPLE_MARGIN_DAYS} days, and never more than ${LICENCE_DAYS} days`)
      && T.includes(`lasts no more than ${hours(apple.APPLE_SANDBOX_MARGIN_HOURS)} past the end of its period`),
      `terms: an App Store licence lasts to its period's end or its grace period's, plus APPLE_MARGIN_DAYS (${apple.APPLE_MARGIN_DAYS}), APPLE_SANDBOX_MARGIN_HOURS (${apple.APPLE_SANDBOX_MARGIN_HOURS}) in the sandbox, at most LICENCE_DAYS`);
    check(P.includes(`The licence ${until}, plus ${apple.APPLE_MARGIN_DAYS} days (${hours(apple.APPLE_SANDBOX_MARGIN_HOURS)}, for a purchase in Apple's test environment), and never more than ${LICENCE_DAYS} days`),
      'privacy: the same, under Subscribing in the iPhone app');
    check([T, P].every((page) => page.includes('(or of its billing grace period)')), 'both pages count a billing grace period in it');
    check(!/past the end of the period Apple has been paid for/.test(T + P), 'and neither says a licence ends with what Apple has been paid for');
    check(P.includes('Each purchase names the app, the product') && P.indexOf('Each purchase names the app, the product') < P.indexOf(`The licence ${until}`),
      'privacy: what each purchase names comes straight after the purchases, before the licence\'s lifetime');
    check(T.includes('AI features stop as soon as Apple refunds or revokes the subscription; mail keeps working until that licence\'s end')
      && P.includes('A refund stops AI at once, and mail keeps working until that licence\'s end')
      && P.includes('For a subscription bought in the iPhone app, the relay also checks the App Store subscription record above that the subscription has not been refunded or revoked'),
      'both pages: a refund or a revocation stops AI at once (app/api/ai, appleAiLive), and mail runs to the licence\'s end');
    /* claude/iap-native sends only `transactions` so far, so the page says
       the renewal information comes from a version that sends it
       (LAUNCH.md section 11, step 4). */
    check(P.includes('With them, a version of the app that reports billing grace periods itself also sends Apple\'s signed renewal information for each subscription')
      && P.includes('nor anything in the renewal information but when a billing grace period ends, and when Apple signed that'),
      'privacy: the renewal information a version of the app that reports grace periods sends (renewalInfos), and that only the grace period\'s end, and when Apple signed it (grace_signed_at), are kept from it');
    /* What a renewal info carries (the library's JWSRenewalInfoDecodedPayload)
       beyond the grace period, said where the page lists what the app sends. */
    check(['whether it will renew and when', 'whether Apple is still retrying a failed payment', 'when the subscription most recently began',
      'Apple\'s numbers for it, a value StoreKit uses to check that it belongs to your device, and when Apple signed it'].every((w) => P.includes(w)),
      'privacy: the renewal information is described whole: when it renews, a payment Apple is retrying, when it began, Apple\'s numbers and its signing date');
    /* Every field Apple's library reads from a signed transaction and from
       a renewal info (JWSTransactionDecodedPayload and
       JWSRenewalInfoDecodedPayload, in the version package.json pins) is
       named on the page, under what the app sends, so a field a new
       version adds fails here until the page says it. Two are left out,
       each for its reason: appAccountToken, which the app never sets
       (claude/iap-native buys with no purchase options), and
       advancedCommerceInfo, which only an Advanced Commerce product
       carries, and RATA sells none. */
    const sends = (P.match(/When you subscribe or restore a purchase.*?Apple does not tell us them\./) || [''])[0];
    const NOT_SENT = ['appAccountToken', 'advancedCommerceInfo'];
    const fieldsOf = (model) => [...readFileSync(join(SITE, 'node_modules', '@apple', 'app-store-server-library', 'dist', 'models', `${model}.d.ts`), 'utf8')
      .matchAll(/^ {4}(\w+)\?: /gm)].map((m) => m[1]);
    const purchase = (sends.match(/Each purchase names (.*?)\. With them/) || ['', ''])[1];
    const renewalInfo = (sends.match(/renewal information for each subscription: (.*?)\. Apple also tells/) || ['', ''])[1];
    const TRANSACTION_SAID = {
      originalTransactionId: 'Apple\'s numbers for the purchase', transactionId: 'Apple\'s numbers for the purchase', webOrderLineItemId: 'Apple\'s numbers for the purchase',
      bundleId: 'the app', productId: 'the product', subscriptionGroupIdentifier: 'its subscription group',
      purchaseDate: 'when it was bought', originalPurchaseDate: 'when it was bought', expiresDate: 'when it renews or expires',
      quantity: 'how many were bought', type: 'what kind of purchase it is', inAppOwnershipType: 'Family Sharing', signedDate: 'when Apple signed it',
      revocationReason: 'why and how much of it was refunded or revoked', revocationDate: 'refunded or revoked', revocationType: 'why and how much of it was refunded',
      revocationPercentage: 'how much of it was refunded', isUpgraded: 'whether it was upgraded',
      offerType: 'any offer applied to it', offerIdentifier: 'any offer applied to it', offerDiscountType: 'any offer applied to it', offerPeriod: 'and for how long',
      environment: 'whether it was made in Apple\'s test environment', storefront: 'the country of the App Store', storefrontId: 'the country of the App Store',
      transactionReason: 'whether it was a renewal', currency: 'in its currency', price: 'and the price', appTransactionId: 'for the app\'s download',
      billingPlanType: 'its billing plan', commitmentInfo: 'any commitment to pay for several periods',
    };
    const RENEWAL_SAID = {
      expirationIntent: 'why it expired if it has', originalTransactionId: 'Apple\'s numbers for it', autoRenewProductId: 'to which product', productId: 'to which product',
      autoRenewStatus: 'whether it will renew', isInBillingRetryPeriod: 'whether Apple is still retrying a failed payment',
      priceIncreaseStatus: 'whether a price increase was agreed to', gracePeriodExpiresDate: 'when a billing grace period ends',
      offerType: 'any offer that applies', offerIdentifier: 'any offer that applies', offerDiscountType: 'any offer that applies', offerPeriod: 'any offer that applies',
      signedDate: 'when Apple signed it', environment: 'whether it is in Apple\'s test environment', recentSubscriptionStartDate: 'when the subscription most recently began',
      renewalDate: 'whether it will renew and when', currency: 'in which currency', renewalPrice: 'at what price', eligibleWinBackOfferIds: 'any Apple may make to win it back',
      appTransactionId: 'Apple\'s numbers for it', commitmentInfo: 'any commitment to pay for several periods', renewalBillingPlanType: 'on which billing plan',
    };
    const unnamed = (model, said, words) => fieldsOf(model).filter((f) => !NOT_SENT.includes(f) && !(said[f] && words.includes(said[f])));
    const txUnnamed = unnamed('JWSTransactionDecodedPayload', TRANSACTION_SAID, purchase);
    const riUnnamed = unnamed('JWSRenewalInfoDecodedPayload', RENEWAL_SAID, renewalInfo);
    check(fieldsOf('JWSTransactionDecodedPayload').length >= 30 && fieldsOf('JWSRenewalInfoDecodedPayload').length >= 20 && txUnnamed.length === 0 && riUnnamed.length === 0,
      `privacy: every field of a signed purchase and of its renewal information (the library's models) is named where the page lists what the app sends${txUnnamed.length || riUnnamed.length ? `; not: ${[...txUnnamed, ...riUnnamed].join(', ')}` : ''}`);
    /* And what StoreKit adds on the phone, outside the library's models:
       deviceVerification and deviceVerificationNonce, a value tied to the
       device (Apple's identifierForVendor) and its nonce, which StoreKit
       documents on a Transaction and on a
       Product.SubscriptionInfo.RenewalInfo alike. */
    check(purchase.includes('a value StoreKit uses to check that the purchase belongs to your device')
      && renewalInfo.includes('a value StoreKit uses to check that it belongs to your device')
      && P.includes('the device value or anything else a purchase names that the record does not list'),
      'privacy: and StoreKit\'s device value, on the purchase and on the renewal information, which is sent and not kept');
    /* What Apple's notifications say (App Store Server Notifications V2):
       every notification type in the library's NotificationTypeV2, and
       every field of a notification and of its data, is named in the
       sentences after the renewal information, so a type or field a new
       version adds fails here until the page says it. Apple posts every
       type to the URL, whatever RATA does with it; the route acts on some
       and answers the rest 200. Left out, each for its reason: TEST,
       which Apple sends only when the owner asks for one, about no
       subscription; EXTERNAL_PURCHASE_TOKEN (and the payload's
       externalPurchaseToken), only for apps using Apple's External
       Purchase APIs; ONE_TIME_CHARGE, only for consumables,
       non-consumables and non-renewing subscriptions, which RATA does not
       sell; METADATA_UPDATE, MIGRATION and PRICE_CHANGE, only for apps
       using the Advanced Commerce API; and of the payload, `data` (what it
       holds is named field by field), `version` (the format's, "2.0") and
       `summary`, which comes only with RENEWAL_EXTENSION's SUMMARY after
       the owner asks Apple to extend every subscription at once, and holds
       counts, a product and countries, naming no subscription. */
    const told = (sends.match(/(Apple also tells mailrata\.org of each change to the subscription, in the same terms: .*?)\. Each of these names/) || ['', ''])[1];
    const consent = (sends.match(/When a parent or guardian withdraws consent (.*?)\. None of it carries/) || ['', ''])[1];
    const model = (f) => readFileSync(join(SITE, 'node_modules', '@apple', 'app-store-server-library', 'dist', 'models', `${f}.d.ts`), 'utf8');
    const types = [...model('NotificationTypeV2').matchAll(/^ {4}([A-Z_]+) = "/gm)].map((m) => m[1]);
    const NOT_TOLD = ['TEST', 'EXTERNAL_PURCHASE_TOKEN', 'ONE_TIME_CHARGE', 'METADATA_UPDATE', 'MIGRATION', 'PRICE_CHANGE', 'data', 'version', 'summary', 'externalPurchaseToken'];
    const TOLD_SAID = {
      SUBSCRIBED: 'when it is bought', DID_RENEW: 'renews', DID_FAIL_TO_RENEW: 'fails to renew', EXPIRED: 'or expires', GRACE_PERIOD_EXPIRED: 'when a billing grace period ends',
      DID_CHANGE_RENEWAL_PREF: 'when it is moved to another product', OFFER_REDEEMED: 'or offer', DID_CHANGE_RENEWAL_STATUS: 'or set to renew or not',
      PRICE_INCREASE: 'when its price is to rise', RENEWAL_EXTENDED: 'when its renewal date is extended', RENEWAL_EXTENSION: 'when its renewal date is extended',
      CONSUMPTION_REQUEST: 'when a refund is asked for', REFUND: 'when one is given', REFUND_DECLINED: 'declined', REFUND_REVERSED: 'or reversed',
      REVOKE: 'when it is revoked, as when access to it through Family Sharing ends',
      notificationType: 'of each change to the subscription', subtype: 'of each change to the subscription', notificationUUID: 'Apple\'s number for the message',
      signedDate: 'and when Apple signed it', environment: 'whether it is in Apple\'s test environment', appAppleId: 'Apple\'s number for the app',
      bundleId: 'names the app', bundleVersion: 'and its version', signedTransactionInfo: 'in the same terms', signedRenewalInfo: 'in the same terms',
      status: 'the subscription\'s status', consumptionRequestReason: 'and why',
    };
    /* RESCIND_CONSENT carries appData instead of data: the app, its
       environment and Apple's signed record of the app on the device (an
       AppTransaction). */
    const CONSENT_SAID = {
      RESCIND_CONSENT: 'for a child\'s use of the app', appData: 'its signed record of the app on that device',
      appAppleId: 'Apple\'s numbers for it', bundleId: 'the app and its version', environment: 'whether it is in Apple\'s test environment',
      signedAppTransactionInfo: 'its signed record of the app on that device', receiptType: 'whether it is in Apple\'s test environment',
      applicationVersion: 'the app and its version', versionExternalIdentifier: 'the app and its version', receiptCreationDate: 'when Apple made and signed it',
      originalPurchaseDate: 'when and in which version it was first downloaded', originalApplicationVersion: 'when and in which version it was first downloaded',
      deviceVerification: 'a value StoreKit uses to check that it belongs to the device', deviceVerificationNonce: 'a value StoreKit uses to check that it belongs to the device',
      preorderDate: 'whether it was pre-ordered', appTransactionId: 'Apple\'s numbers for it', originalPlatform: 'on which kind of device',
    };
    const each = (sends.match(/Each of these names (.*?)\. When a parent/) || ['', ''])[1];
    const toldBy = (names, said, words) => names.filter((f) => !NOT_TOLD.includes(f) && !(said[f] && words.includes(said[f])));
    const notTold = [
      ...toldBy(types.filter((t) => t !== 'RESCIND_CONSENT'), TOLD_SAID, told),
      ...toldBy([...fieldsOf('ResponseBodyV2DecodedPayload').filter((f) => f !== 'appData'), ...fieldsOf('Data')], TOLD_SAID, `${told} Each of these names ${each}`),
      ...toldBy(['RESCIND_CONSENT', 'appData', ...fieldsOf('AppData'), ...fieldsOf('AppTransaction')], CONSENT_SAID, consent),
    ];
    check(types.length >= 20 && types.includes('RESCIND_CONSENT') && fieldsOf('Data').length >= 8 && fieldsOf('AppTransaction').length >= 12 && !!told && !!each && !!consent && notTold.length === 0,
      `privacy: every notification type Apple posts for a subscription, and every field of a notification, its data and the consent notice's record of the app (the library's models), is named where the page says what Apple tells mailrata.org${notTold.length ? `; not: ${notTold.join(', ')}` : ''}`);
    /* When the app sends them: not only at a purchase or a restore, but by
       itself while its licence is due (claude/iap-native, appstore.rs: at
       launch and every six hours) and on each transaction StoreKit hands
       over (a renewal, or a purchase on another device). */
    check(sends.startsWith('When you subscribe or restore a purchase, and by itself while your licence is due to be renewed and whenever Apple tells the app of a renewal or of a purchase made on another device, the app sends mailrata.org the purchases')
      && P.includes('The app sends mailrata.org the purchases Apple signed, for your licence, then, and by itself while the licence is due to be renewed.'),
      'privacy: the app sends its purchases when it subscribes or restores, and by itself while the licence is due and when Apple renews the subscription');
    check(T.includes('for a subscription bought in the iPhone app, press Restore purchases there'),
      'terms: a key too old to renew itself comes back, for the iPhone app, with Restore purchases (APPLE_SAID.tooOld)');
  }
}
