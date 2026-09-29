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
    check(app.includes(`.slice(0,${LIMITS.briefingEach})`) && P.includes(`the first ${thousands(LIMITS.briefingEach)} characters`),
      `the start of each: ${thousands(LIMITS.briefingEach)} characters`);
    check(P.includes(`up to the first ${thousands(LIMITS.text)} characters`), `a summary or translation: up to ${thousands(LIMITS.text)} characters`);
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
    check(JSON.stringify(tables) === JSON.stringify(['ai_usage', 'subscriptions', 'workspaces']),
      `the site keeps three tables, the ones the page lists: ${tables.join(', ')}`);
    const usage = (sql.match(/create table if not exists public\.ai_usage \(([\s\S]*?)\n\);/) || ['', ''])[1];
    const cols = [...usage.matchAll(/^\s*(\w+)\s+(?:text|bigint|integer|timestamptz)/gm)].map(m => m[1]);
    check(JSON.stringify(cols) === JSON.stringify(['email', 'month', 'micro_usd', 'requests', 'updated_at']),
      `the AI usage record is an address, a month, a cost and a count, never any text: ${cols.join(', ')}`);
  }
}
