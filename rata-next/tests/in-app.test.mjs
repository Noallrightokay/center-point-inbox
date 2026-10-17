/* The store-safe pages under /in-app/ (the store plan's stage 1, item 6).

   Google Play fails a consumption-only app when any link in it "eventually
   leads to" a purchase, and Apple's 3.1.1 counts any call to action that
   sends a customer to pay outside the store, so what is checked here is the
   whole of what a store build could reach from these pages, and all they
   say: every same-site link is followed, every page found must be under
   /in-app/, an outside link may go only to another service's privacy
   policy or a mail provider's own help, no link or URL string on any of
   them, or in a script they load, leads to the prices, the licence page,
   the app, the sign-in, Stripe or the releases page, and nothing on them,
   in their text or an attribute, sells (BUYING: a price, Stripe, the
   billing portal and the rest). The copies must say everything their pages
   say but what each page marks for the store, from the page as it is now,
   keeping every paragraph that sells nothing, so the help page's and the
   privacy policy's own tests keep covering them. And the deletion page must
   say what deletion removes and keeps as lib/account.js decides it, how
   long the backups hold it and where to write without a sign-in as the
   privacy policy says them, and work, by keyboard and screen reader,
   against the same stand-in Supabase as the account route's own test. */
import { readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import AxeBuilder from '@axe-core/playwright';
import { startServer, makeChecker, fakeSupabase, fakeSupabaseKey } from './helpers.mjs';
import { IN_APP_PAGES, STORE_CUTS, storeSafe } from '../lib/in-app.js';
import { blocksDeletion, DELETION_REMOVES, DELETION_KEEPS } from '../lib/account.js';

const SITE = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = f => readFileSync(join(SITE, 'public', f), 'utf8');

const START = ['/in-app/help', '/in-app/privacy', '/in-app/terms', '/in-app/delete-account'];

/* The outside pages an in-app page may link to: other services' privacy
   policies, which sell nothing. Written here as well as in lib/in-app.js on
   purpose, so a page added there is added here too, by somebody deciding it
   sells nothing. */
const POLICIES = [
  ['www.anthropic.com', '/legal'],
  ['privacy.microsoft.com', '/'],
  ['policies.google.com', '/'],
  ['developers.google.com', '/terms/api-services-user-data-policy'],
];
/* And the mail providers' own help and account pages, where the help page's
   steps make an app password. GitHub's issue template is not one of them:
   it stays text. */
const PROVIDERS = [
  ['support.google.com', '/'],
  ['myaccount.google.com', '/apppasswords'],
  ['help.yahoo.com', '/'],
  ['login.yahoo.com', '/account/security'],
  ['www.fastmail.help', '/'],
  ['support.apple.com', '/'],
  ['account.apple.com', '/'],
  ['support.hostinger.com', '/'],
  ['support.microsoft.com', '/'],
  ['www.zoho.com', '/mail/help'],
];
const within = list => u => u.protocol === 'https:' && !u.port && !u.username && !u.password && list.some(([host, path]) =>
  u.hostname === host && (u.pathname === path || u.pathname.startsWith(path.endsWith('/') ? path : path + '/')));
const isPolicy = within(POLICIES);
const isProvider = within(PROVIDERS);

/* What none of them may say, in its text or in an attribute: a price, a
   price by the month or year, Stripe, the billing portal, the licence
   page, the releases page or an installer, the calls to choose a plan,
   upgrade, buy, or subscribe or sign in (or on) at, to or on mailrata.org,
   in any of those words' forms, that mailrata.org
   sells RATA or that it is bought "here", and paying with an address or at
   a place. Written here as well as in lib/in-app.js (SELLING), which will
   not make a copy that says one. */
const BUYING = [
  ['a price', /[$€£¥]\s*\d|\b\d+(?:[.,]\d+)?\s*(?:USD|EUR|GBP|dollars?|euros?)\b|\b(?:USD|EUR|GBP)\s*\d/i],
  ['a price by the month or year', /\b(?:a|per)\s+(?:month|year)\b|\b(?:billed|charged|paid)\s+(?:monthly|yearly|annually)\b|\/\s?(?:mo|month|yr|year)\b/i],
  ['Stripe', /\bstripe\b/i],
  ['the billing portal', /\bbilling portal\b/i],
  ['the licence page', /\blicen[cs]e page\b/i],
  ['the releases page', /\breleases page\b/i],
  ['an installer', /\binstallers?\b/i],
  ["an installer's file", /\bRATA_<version>_\S*\.(?:exe|dmg|deb|AppImage|msi)\b/i],
  ['Choose a plan', /\bchoose (?:a|your) plan\b/i],
  ['Upgrade', /\bupgrad(?:e|es|ed|ing)\b/i],
  ['Buy', /\bbuy\b|\bbu(?:ys|ying)\s+(?:(?:a|the|your)\s+)?(?:plans?|subscriptions?|licen[cs]es?|RATA)\b/i],
  ['Sold at mailrata.org', /\b(?:sells?|sold|selling|bought)\b[^.]{0,60}?\bmailrata\.org\b|\bmailrata\.org\b[^.]{0,20}?\b(?:sells?|sold|selling)\b|\b(?:bought|sold|buy)\s+here\b|\b(?:also|too)\s+(?:sold|bought)\b/i],
  ['Pay with or at', /\bpay(?:s|ing)?\s+(?:with|at|on)\b/i],
  ['Subscribe at mailrata.org', /\bsubscri(?:be|bes|bed|bing|ption)\b[^.]{0,40}?\b(?:at|on|to|through|via)\s+(?:https?:\/\/)?(?:www\.)?mailrata\.org\b/i],
  ['Sign in at mailrata.org', /\b(?:sign|log)(?:s|ed|ged|ing|ging)?[ -]?(?:in|on)\b[^.]{0,20}?\b(?:at|to|on|into|through|via)\s+(?:https?:\/\/)?(?:www\.)?mailrata\.org\b/i],
];
const sellsIn = text => BUYING.filter(([, re]) => re.test(text)).map(([what]) => what);

/* Every cut and every piece of store wording, by page, as lib/in-app.js's
   STORE_CUTS names them; a new one is added here too, by somebody deciding
   the store copy may say those words differently. */
const CUTS = {
  help: ['gutter', 'footer', 'get', 'install-steps', 'key', 'write'],
  privacy: ['footer', 'card', 'record', 'record-early', 'processor', 'processor-records', 'github', 'payment',
    'how', 'records', 'live', 'pending'],
  terms: ['footer', 'agree', 'sells', 'pay-with', 'base-price', 'pro-price', 'billing', 'changes', 'licence-page',
    'new-key', 'stop', 'unsigned'],
};

/* Where nothing on these pages may lead, written as URL strings, wherever
   they are: an attribute, a script, a comment or the text. */
const BANNED = [
  ['the prices', /#pricing\b/i],
  ['Stripe', /\bstripe\.com\b/i],
  ['the releases page', /\/releases\b/i],
  ['a page that sells, by its file', /\b(?:index|account|app|auth)\.html\b/i],
  ['a page that sells, by its path', /(?:mailrata\.org|["'`(=])\/(?:account|app|auth)(?:\.html)?(?![\w.-])/i],
  ['the home page', /(?:https?:)?\/\/mailrata\.org\/?(?=["'`\s<>)#?]|$)/i],
];

const ENT = { amp: '&', lt: '<', gt: '>', quot: '"', nbsp: ' ', rsquo: '’', lsquo: '‘', ldquo: '“', rdquo: '”', rarr: '→', hellip: '…' };
const entities = s => s.replace(/&(\w+);/g, (m, n) => ENT[n] ?? m).replace(/&#(\d+);/g, (m, n) => String.fromCharCode(+n));
/* What a reader sees, as one line. A link's tags and a comment take no room
   on screen, so they leave none here: a link a copy turns into its words
   reads exactly as it did. */
const visible = html => entities(html
  .replace(/<style[\s\S]*?<\/style>/gi, ' ')
  .replace(/<script[\s\S]*?<\/script>/gi, ' ')
  .replace(/<!--[\s\S]*?-->/g, '')
  .replace(/<\/?a\b[^>]*>/gi, '')
  .replace(/<[^>]+>/g, ' ')).replace(/\s+/g, ' ').trim();
const mainOf = html => (html.match(/<main\b[\s\S]*<\/main>/i) || [''])[0];
/* The marks a source carries for its store copy (lib/in-app.js says what
   each does), read here on their own. */
const CUT = /<!-- in-app:cut ([a-z0-9-]+) -->([\s\S]*?)<!-- in-app:end -->/g;
const WORDS = /<!-- in-app:words -->([\s\S]*?)<!-- in-app:end\n([\s\S]*?)-->/g;
const ADD = /<!-- in-app:add ([a-z0-9-]+)\n([\s\S]*?)-->/g;
const wordingOf = tail => tail.split('\n').map(l => l.trim()).filter(Boolean).map(l => {
  const w = /^([a-z0-9-]+): (.+?) =>(?: (.*))?$/.exec(l);
  return w ? { id: w[1], old: w[2], now: w[3] || '' } : { id: null, old: l, now: '' };
});
/* The page as its store copy should read: what it cuts left out, its
   store wording in place of the words it replaces, and what only the store
   copy says added. */
const asCopy = html => html.replace(CUT, '').replace(WORDS, (all, body, tail) =>
  wordingOf(tail).reduce((b, { old, now }) => b.replace(old, () => now), body)).replace(ADD, (all, id, put) => put.trim());
/* Each paragraph, list item, heading and quote, as a reader sees it. */
const blocksOf = html => [...html.replace(/<!--[\s\S]*?-->/g, '')
  .matchAll(/<(p|li|h[1-4]|dt|dd|blockquote)\b[^>]*>([\s\S]*?)<\/\1>/gi)].map(m => visible(m[2])).filter(Boolean);

/* Every tag of these names, with its attributes read as a browser reads
   them: a value in double quotes, in single quotes or in none, a name in
   any case, a name only where an attribute starts (data-href is not href),
   the first of a name winning, entities decoded. A tag that never closes
   (a quote left open) comes back with `closed` false, so a check can fail
   on it rather than skip it. */
function tagsIn(html, names) {
  const out = [];
  const GAP = /[\s/]*/y;
  const NAME = /[^\s"'>/=]+/y;
  const EQ = /\s*=\s*/y;
  const BARE = /[^\s>]*/y;
  for (const m of html.matchAll(new RegExp(`<(${names.join('|')})(?=[\\s/>])`, 'gi'))) {
    const attrs = new Map();
    let i = m.index + m[0].length;
    let closed = false;
    while (i < html.length) {
      GAP.lastIndex = i; GAP.exec(html); i = GAP.lastIndex;
      if (html[i] === '>') { closed = true; break; }
      NAME.lastIndex = i;
      if (!NAME.exec(html)) { i++; continue; }
      const name = html.slice(i, NAME.lastIndex).toLowerCase();
      i = NAME.lastIndex;
      let value = '';
      EQ.lastIndex = i;
      if (EQ.exec(html)) {
        i = EQ.lastIndex;
        const q = html[i];
        if (q === '"' || q === "'") {
          const end = html.indexOf(q, i + 1);
          if (end < 0) break;
          value = html.slice(i + 1, end);
          i = end + 1;
        } else {
          BARE.lastIndex = i; BARE.exec(html);
          value = html.slice(i, BARE.lastIndex);
          i = BARE.lastIndex;
        }
      }
      if (!attrs.has(name)) attrs.set(name, entities(value));
    }
    out.push({ tag: m[1].toLowerCase(), attrs, closed, at: m.index, end: i + 1 });
  }
  return out;
}

/* Everything on a page that can take somebody somewhere, with the address
   it goes to; `url` is null where the address cannot be read. A stylesheet
   or an icon is part of the page, not somewhere to go, and an <a> with no
   href goes nowhere. */
const GOES_BY = { a: 'href', area: 'href', link: 'href', base: 'href', form: 'action', button: 'formaction', input: 'formaction', iframe: 'src', frame: 'src', embed: 'src', object: 'data' };
const OPTIONAL = new Set(['a', 'area', 'button', 'input']);
const PART_OF_THE_PAGE = new Set(['stylesheet', 'icon', 'apple-touch-icon']);
function linksIn(html) {
  const out = [];
  for (const { tag, attrs, closed } of tagsIn(html, Object.keys(GOES_BY))) {
    /* An SVG link's address is its xlink:href. */
    const at = tag === 'a' && !attrs.has('href') && attrs.has('xlink:href') ? 'xlink:href' : GOES_BY[tag];
    if (closed && OPTIONAL.has(tag) && !attrs.has(at)) continue;
    if (closed && tag === 'link') {
      const rel = (attrs.get('rel') || '').toLowerCase().split(/\s+/).filter(Boolean);
      if (rel.length && rel.every(r => PART_OF_THE_PAGE.has(r))) continue;
    }
    out.push({ tag, url: closed ? attrs.get(at) ?? '' : null });
  }
  for (const { attrs, closed } of tagsIn(html, ['meta'])) {
    if (!closed || /^refresh$/i.test(attrs.get('http-equiv') || '')) out.push({ tag: 'meta refresh', url: closed ? attrs.get('content') ?? '' : null });
  }
  return out;
}

/* The address each tag of these names loads, by the first of `at` it has
   ('' for none), or null where the tag cannot be read. */
const loadsIn = (html, names, at) => tagsIn(html, names)
  .map(t => (t.closed ? t.attrs.get(at.find(a => t.attrs.has(a)) || '') ?? '' : null));

export default async function run(state) {
  const check = makeChecker(state);

  /* The site as a store build's links would find it, with Stripe's links
     set, so a page that loaded the whole site configuration would be
     caught carrying them. */
  const s = await startServer({ env: {
    STRIPE_BASE: 'https://buy.stripe.com/test_base',
    STRIPE_PRO: 'https://buy.stripe.com/test_pro',
    STRIPE_PORTAL: 'https://billing.stripe.com/p/login/test_portal',
  } });
  try {
    const origin = new URL(s.url).origin;

    console.log('\n— the crawl reads an address however it is written —');
    {
      /* The deletion page is written by hand and never passes through
         storeSafe, so the crawl below is all that stands between it and a
         link to the site's own pages, which carry the selling navigation. */
      const got = linksIn(`<a href='/help'>1</a> <a href=/privacy.html>2</a> <A HREF = "/terms.html">3</A>
        <a data-href="/in-app/help" href="/app">4</a> <a title="a > b" href="/account">5</a> <a/href="/auth.html">6</a>
        <a id="here">7</a> <a href="#here" href="/app">8</a> <form action='/x'></form> <button formaction=/y>9</button>
        <button type="submit">10</button> <link rel="icon" href="/i.png"> <link rel="icon prefetch" href="/app">
        <meta http-equiv=Refresh content="0;url=/app"> <a data-x=">" href="index.html#pricing">12</a> <a xlink:href="/app">13</a>
        <a href="/in-app/terms>11</a>`).map(l => `${l.tag} ${l.url}`);
      const want = ['a /help', 'a /privacy.html', 'a /terms.html', 'a /app', 'a /account', 'a /auth.html', 'a #here', 'form /x',
        'button /y', 'link /app', 'a index.html#pricing', 'a /app', 'a null', 'meta refresh 0;url=/app'];
      check(JSON.stringify(got) === JSON.stringify(want),
        `single quotes, no quotes, any case, a > in a value, data-href, xlink:href and a quote left open are all read: ${JSON.stringify(got)}`);
      const srcs = loadsIn(`<script src='/a.js'></script><SCRIPT SRC=/b.js></SCRIPT><script>x</script><script src="/c.js`, ['script'], ['src']);
      check(JSON.stringify(srcs) === JSON.stringify(['/a.js', '/b.js', '', null]), `and so is a script's address: ${JSON.stringify(srcs)}`);
    }

    console.log('\n— every page a store build can reach from here is under /in-app/ —');
    const pages = new Map();
    const scripts = new Map();
    /* Links into a place on another in-app page, checked once every page is read. */
    const into = [];
    {
      const queue = [...START];
      while (queue.length) {
        const path = queue.shift();
        if (pages.has(path)) continue;
        const r = await fetch(s.url + path, { redirect: 'manual' });
        const html = await r.text();
        pages.set(path, html);
        check(r.status === 200 && /text\/html/.test(r.headers.get('content-type') || ''), `${path} -> ${r.status} ${r.headers.get('content-type')}`);
        const wrong = [];
        for (const { tag, url } of linksIn(html)) {
          if (url == null) { wrong.push(`a <${tag}> whose address cannot be read`); continue; }
          /* A form with no action goes nowhere but its own page, whose
             script takes the submit (the deletion page's sign-in). */
          if (tag === 'form' && !url) continue;
          if (!['a', 'area', 'form', 'button', 'input'].includes(tag)) { wrong.push(`a <${tag}> to ${url || '(nowhere)'}`); continue; }
          if (url.startsWith('#')) {
            if (url.length > 1 && !html.includes(`id="${url.slice(1)}"`)) wrong.push(`${url}, which is nowhere on the page`);
            continue;
          }
          let u;
          try { u = new URL(url, s.url + path); } catch { wrong.push(`${url}, which does not parse`); continue; }
          if (u.protocol === 'mailto:') continue;
          if (u.origin === origin) {
            if (!u.pathname.startsWith('/in-app/')) wrong.push(`${url}, a page outside /in-app/`);
            else {
              queue.push(u.pathname);
              if (u.hash.length > 1) into.push({ from: path, url, to: u.pathname, id: decodeURIComponent(u.hash.slice(1)) });
            }
            continue;
          }
          if (!isPolicy(u) && !isProvider(u)) wrong.push(`${url}, which is neither another service's privacy policy nor a mail provider's own help`);
        }
        check(wrong.length === 0, `${path} links nowhere else${wrong.length ? `:\n         ${wrong.join('\n         ')}` : ''}`);
        if (/<link\b[^>]*rel="manifest"/i.test(html)) check(false, `${path} links the web app manifest, which opens /app`);
        for (const src of loadsIn(html, ['script'], ['src'])) {
          if (src == null) check(false, `${path}: a <script> whose address cannot be read`);
          else if (src) scripts.set(new URL(src, s.url + path).href, null);
        }
      }
      check(START.every(p => pages.has(p)) && pages.size === START.length,
        `found ${pages.size} pages, all of them the four in-app pages: ${[...pages.keys()].join(', ')}`);
      const lost = into.filter(l => !(pages.get(l.to) || '').includes(`id="${l.id}"`));
      check(into.length > 0 && lost.length === 0,
        `all ${into.length} links into a place on another in-app page find that place` +
        (lost.length ? `:\n         ${lost.map(l => `${l.from} -> ${l.url}`).join('\n         ')}` : ''));
      for (const path of ['/in-app/app', '/in-app/index', '/in-app/account', '/in-app/auth', '/in-app/help.html']) {
        const r = await fetch(s.url + path, { redirect: 'manual' });
        check(r.status === 404, `${path} is no page (${r.status}): only the copies are made`);
      }
    }

    console.log('\n— the copies are made when the site is built, and go out as their pages do —');
    {
      /* Made by `npm run build`, so a page whose copy cannot be made (a
         sentence changed under its store wording) fails the build, in CI
         and on the host, instead of answering 500 once deployed. */
      let built = {};
      try { built = JSON.parse(readFileSync(join(SITE, '.next', 'prerender-manifest.json'), 'utf8')).routes || {}; } catch { /* checked below */ }
      const names = Object.keys(IN_APP_PAGES);
      check(names.every(n => built[`/in-app/${n}`]), `Next made ${names.map(n => `/in-app/${n}`).join(', ')} at build time`);
      /* Next would send a page it made at build time with s-maxage of a
         year, which a cache in between may keep long after the policy
         changes; proxy.js sends each as the page it is made from goes. */
      const PAGE_AT = { help: '/help', privacy: '/privacy.html', terms: '/terms.html' };
      for (const [name, file] of Object.entries(IN_APP_PAGES)) {
        const [copy, page] = await Promise.all([`/in-app/${name}`, PAGE_AT[name] || `/${file}`]
          .map(p => fetch(s.url + p, { redirect: 'manual' })));
        const [a, b] = [copy, page].map(r => r.headers.get('cache-control'));
        check(copy.status === 200 && page.status === 200 && !!a && a === b && !/s-maxage/.test(a),
          `/in-app/${name} goes out with its page's Cache-Control: ${JSON.stringify(a)} (${PAGE_AT[name] || file}: ${JSON.stringify(b)})`);
      }
      /* Nor do they read public/ while the site runs: a host that starts
         Next from another folder still serves them. */
      const away = mkdtempSync(join(tmpdir(), 'rata-elsewhere-'));
      const s2 = await startServer({ from: away });
      try {
        for (const name of names) {
          const r = await fetch(`${s2.url}/in-app/${name}`, { redirect: 'manual' });
          const html = await r.text();
          check(r.status === 200 && html === (pages.get(`/in-app/${name}`) || null),
            `/in-app/${name} is served the same by a server started from another folder (${r.status})`);
        }
      } finally {
        await s2.stop();
        rmSync(away, { recursive: true, force: true });
      }
    }

    console.log('\n— nothing on them, or in what they load, leads to buying —');
    {
      /* Scripts are followed too, as are scripts a script names (the
         Supabase library the deletion page loads when someone signs in). */
      const queue = [...scripts.keys()];
      while (queue.length) {
        const href = queue.shift();
        if (scripts.get(href) != null) continue;
        const u = new URL(href);
        if (u.origin !== origin) { check(false, `a script from another host: ${href}`); scripts.set(href, ''); continue; }
        const r = await fetch(href);
        const js = await r.text();
        scripts.set(href, js);
        check(r.status === 200, `script ${u.pathname}${u.search} -> ${r.status}`);
        for (const m of js.matchAll(/["'`](\/(?:vendor|in-app)\/[\w./-]+\.js)["'`]/g)) {
          const next = new URL(m[1], href).href;
          if (!scripts.has(next)) { scripts.set(next, null); queue.push(next); }
        }
      }
      for (const [where, text] of [...pages, ...[...scripts].map(([h, js]) => [new URL(h).pathname + new URL(h).search, js])]) {
        const found = BANNED.flatMap(([what, re]) => {
          const m = text.match(re);
          return m ? [`${what}: ${JSON.stringify(text.slice(Math.max(0, m.index - 30), m.index + m[0].length + 20))}`] : [];
        });
        check(found.length === 0, `${where}: no link or URL string to the prices, the licence page, the app, the sign-in, Stripe or the releases${found.length ? `\n         ${found.join('\n         ')}` : ''}`);
      }
      check([...scripts.keys()].some(h => h.includes('/api/config')),
        'the deletion page reads the site configuration, so the scan above saw what it reads');
    }

    console.log('\n— nothing on them says anything that sells, in their text or an attribute —');
    {
      /* Not only links: a price, Stripe or the billing portal written as
         plain text is still a call to pay outside the store. */
      const where = (text, what) => {
        const re = BUYING.find(([w]) => w === what)[1];
        const m = text.match(re);
        return m ? JSON.stringify(text.slice(Math.max(0, m.index - 40), m.index + m[0].length + 20)) : '';
      };
      for (const [path, html] of pages) {
        const attrs = tagsIn(html, ['[a-zA-Z][\\w:-]*']).flatMap(t => [...t.attrs.values()]);
        const text = `${visible(html)} ${entities(attrs.join(' '))}`;
        const found = sellsIn(text);
        check(attrs.length > 10 && found.length === 0,
          `${path} says nothing that sells, in its text or its ${attrs.length} attribute values${found.length ? `:\n         ${found.map(w => `${w}: ${where(text, w)}`).join('\n         ')}` : ''}`);
      }
      /* The deletion page's own script, every sentence it can show, and the
         refusals the account route gives it (it asks for the store's words). */
      const js = [...scripts].filter(([h]) => h.includes('/in-app/')).map(([, j]) => j || '').join('\n');
      check(js.length > 0 && sellsIn(js).length === 0, `the deletion page's script says nothing that sells: ${sellsIn(js).join(', ') || 'nothing'}`);
      const at = Date.parse('2026-10-17T12:00:00Z');
      const rows = [{ status: 'active', plan: 'pro' }, { status: 'trialing' }, { status: 'past_due', plan: 'base' },
        { status: 'incomplete', plan: 'pro' }, { status: 'canceled', plan: 'pro', updated_at: new Date(at - 86400000).toISOString() }];
      const store = rows.map(r => blocksDeletion(r, at, { store: true }));
      const site = rows.map(r => blocksDeletion(r, at));
      check(store.every(Boolean) && store.every(r => !sellsIn(r).length && !/[–—]/.test(r)),
        `every refusal the deletion page can show sells nothing: ${JSON.stringify(store.map(r => r && sellsIn(r)))}`);
      check(site.slice(0, 4).every(r => sellsIn(r).length) && site[4] === store[4]
        && [null, { status: 'canceled', updated_at: new Date(at - 40 * 86400000).toISOString() }, { status: 'incomplete_expired' }]
          .every(r => blocksDeletion(r, at, { store: true }) === null),
        "and they are the website's refusals, by the same rules, in other words only where those name the billing portal or Stripe");
    }

    console.log('\n— the copies say everything their pages say, from the page as it is now —');
    for (const [name, file] of Object.entries(IN_APP_PAGES)) {
      const source = read(file);
      const served = pages.get(`/in-app/${name}`) || '';
      check(served === storeSafe(name, source), `/in-app/${name} is exactly what lib/in-app.js makes from public/${file} as it is now`);
      check(/<nav\b/i.test(source) && /<footer\b/i.test(source) && !/<nav\b|<footer\b/i.test(served),
        `/in-app/${name}: ${file}'s site navigation and footer are left out`);
      const brand = (served.match(/<header><div class="wrap nav-in">([\s\S]*?)<\/header>/) || [])[1] || '';
      check(/<b>RATA<\/b>/.test(brand) && !/<a\b/i.test(brand), `/in-app/${name} names RATA at the top, as text`);
      check(/<meta name="robots" content="noindex">/.test(served), `/in-app/${name} asks search engines for the page itself instead`);
      /* Every mark is one somebody decided on: named here and in
         lib/in-app.js. */
      const wording = [...source.matchAll(WORDS)].flatMap(m => wordingOf(m[2]));
      const added = [...source.matchAll(ADD)].map(m => [m[1], m[2]]);
      const ids = [...[...source.matchAll(CUT)].map(m => m[1]), ...wording.map(w => w.id), ...added.map(([id]) => id)].sort();
      check(JSON.stringify(ids) === JSON.stringify([...CUTS[name]].sort())
        && JSON.stringify([...STORE_CUTS[name]].sort()) === JSON.stringify([...CUTS[name]].sort()),
        `${file}: its cuts and store wording are the ${CUTS[name].length} named here and in lib/in-app.js: ${ids.join(', ')}`);
      /* A cut in the page's text takes out something that sells (or, as
         before, only a link's few words), and store wording replaces words
         that sell with words that do not. */
      const cuts = [...mainOf(source).matchAll(CUT)].map(m => [m[1], m[2]]);
      const badCuts = cuts.filter(([, c]) => !sellsIn(visible(c)).length && !(/<a\b/i.test(c) && visible(c).length <= 40));
      check(badCuts.length === 0, `${file}: what it leaves out of its text sells${badCuts.length ? `, but not ${badCuts.map(([id]) => id).join(', ')}` : ''}`);
      const badWords = wording.filter(w => !w.id || !sellsIn(visible(w.old)).length || sellsIn(visible(w.now)).length);
      check(wording.length > 0 && badWords.length === 0,
        `${file}: each of its ${wording.length} pieces of store wording replaces words that sell with words that do not${badWords.length ? `: ${badWords.map(w => w.id || w.old).join(', ')}` : ''}`);
      /* Words, or a stylesheet (the help page's gutter on a phone), whose
         own text sells nothing either. */
      const badAdds = added.filter(([, put]) => (/^<style\b[^>]*>[^<]*<\/style>$/i.test(put.trim())
        ? sellsIn(put).length : !visible(put) || sellsIn(visible(put)).length));
      check(badAdds.length === 0, `${file}: what its store copy adds (${added.length}) sells nothing${badAdds.length ? `, but ${badAdds.map(([id]) => id).join(', ')} does` : ''}`);
      /* What a reader of the page's main part sees is the same, word for
         word, but for what the page marks for the store. */
      const want = visible(mainOf(asCopy(source)));
      const got = visible(mainOf(served));
      let at = 0;
      while (at < want.length && want[at] === got[at]) at++;
      check(want.length > 1000 && want === got,
        `/in-app/${name} says all ${want.length} characters of ${file}'s text, with its store wording` +
        (want === got ? '' : `\n         first difference: ${JSON.stringify(want.slice(at, at + 60))} became ${JSON.stringify(got.slice(at, at + 60))}`));
      /* And every paragraph, list item and heading of the page that sells
         nothing is in the copy whole, so store wording can only ever change
         what sells. */
      const blocks = blocksOf(mainOf(source.replace(CUT, '')));
      const clean = blocks.filter(b => !sellsIn(b).length);
      const lost = clean.filter(b => !got.includes(b));
      check(clean.length > 20 && lost.length === 0,
        `/in-app/${name} keeps all ${clean.length} of ${file}'s paragraphs, items and headings that sell nothing` +
        (lost.length ? `:\n         ${lost.slice(0, 3).map(b => JSON.stringify(b.slice(0, 90))).join('\n         ')}` : ''));
    }
    {
      /* The help page's quotes and the privacy policy's promises are
         checked on the pages themselves (help.test.mjs, legal.test.mjs);
         the copies carry the same, so a store build shows them too. */
      const help = pages.get('/in-app/help') || '';
      const said = h => (h.match(/<blockquote class="said">/g) || []).length;
      check(said(help) > 0 && said(help) === said(read('help.html')), `/in-app/help quotes all ${said(help)} of RATA's sentences`);
      const policy = visible(pages.get('/in-app/privacy') || '');
      check(DELETION_REMOVES.every(x => policy.includes(x.replace(/[‘’]/g, '’'))), '/in-app/privacy names everything deletion removes');
      /* Where to delete an account, in the store copy, is the deletion page
         itself, not Settings on the website. */
      const del = ((pages.get('/in-app/privacy') || '').match(/<section id="delete"[\s\S]*?<\/section>/) || [''])[0];
      check(/<a href="\/in-app\/delete-account">[^<]+<\/a>/.test(del) && !/Settings/.test(visible(del)),
        `/in-app/privacy#delete sends a reader to the deletion page: ${JSON.stringify(visible((del.match(/<p>[\s\S]*?<\/p>/) || [''])[0]))}`);
      const ending = ((pages.get('/in-app/terms') || '').match(/<section id="ending"[\s\S]*?<\/section>/) || [''])[0];
      check(/<a href="\/in-app\/delete-account">/.test(ending), '/in-app/terms#ending does too');
      /* The terms' store copy says neither that mailrata.org sells the plans
         nor to pay with the account's address (no store payment is made
         with an address), and agreeing to them needs no purchase. */
      const terms = visible(pages.get('/in-app/terms') || '');
      check(/By creating an account or using RATA, you agree to them\./.test(terms) && /mailrata\.org issues the licence that unlocks the app/.test(terms)
        && !/buying a plan|sells the plans|Pay with the same address|matched by that address|licence page|Paste it/i.test(terms),
        '/in-app/terms says nothing of buying at mailrata.org, of paying with the account\'s address, or of pasting a key from a page');
      /* A store build's Report a bug opens /in-app/help#report, where the
         website's link to GitHub is only words. The copy names where to
         write instead, the privacy policy's own address, as a mailto: link
         (Apple 1.5: an easy way to reach the developer); live-check.sh
         refuses to see its placeholder on the live site. */
      const contact = (visible(read('privacy.html')).match(/For anything on this page, write to (.+?)\.(?:\s|$)/) || [])[1];
      const report = (help.match(/<section id="report"[\s\S]*?<\/section>/) || [''])[0];
      check(!!contact && report.includes(`<a href="mailto:${contact}">${contact}</a>`) && /write to/.test(visible(report))
        && !/github\.com/.test(report) && !visible(read('help.html')).includes(`write to ${contact} with the same details`),
        `/in-app/help#report says where to write, the privacy policy's address (${contact}), and the website's help page does not change`);
      check(!/If RATA asks for your licence key|paste it in: one line/.test(visible(help)), '/in-app/help asks nobody to paste a licence key');
    }

    console.log('\n— a link added to a page later is judged too, its attributes read as a browser reads them —');
    {
      /* storeSafe keeps only what it knows is safe, so a source that gains
         a link nobody thought about still makes a safe copy. */
      const into = extra => read('help.html').replace('<h1>Help</h1>', `<h1>Help</h1>${extra}`);
      /* Each link left in a copy, as text=address, read as the crawl reads it. */
      const linked = html => tagsIn(html, ['a']).filter(t => t.attrs.has('href') || t.attrs.has('xlink:href'))
        .map(t => `${visible(html.slice(t.end, t.end + html.slice(t.end).search(/<\/a\b/i)))}=${t.attrs.get('href') ?? t.attrs.get('xlink:href')}`);
      const mine = /^(plans|licence|checkout|home|app|odd|gone|fake|fake2|plain|mail|terms|delete|Google|provider|template|install|single|upper|x|y|z|q|r|dup|ping|bare|prices)=/;
      const src = into('<p>New: <a href="index.html#pricing">plans</a>, <a href="/account">licence</a>, '
        + '<a href="https://buy.stripe.com/x">checkout</a>, <a href="https://mailrata.org/">home</a>, <a href="//mailrata.org/app">app</a>, '
        + '<a href="privacy.html?x=1">odd</a>, <a href="/delete-account">gone</a>, <a href="https://policies.google.com.evil.example/">fake</a>, '
        + '<a href="https://support.google.com.evil.example/">fake2</a>, <a href="http://support.google.com/mail/">plain</a>, '
        + '<a href="mailto:help@example.org">mail</a>, <a href="terms.html#ai">terms</a>, <a href="/in-app/delete-account">delete</a>, '
        + '<a href="https://policies.google.com/privacy">Google</a>, <a href="https://support.google.com/mail/answer/7126229">provider</a>, '
        + '<a href="https://github.com/Noallrightokay/center-point-inbox/issues/new?template=beta-bug.md">template</a>, '
        + '<a href="#install">install</a>.</p>');
      const kept = linked(storeSafe('help', src)).filter(l => mine.test(l));
      check(JSON.stringify(kept) === JSON.stringify(['mail=mailto:help@example.org', 'terms=/in-app/terms#ai',
        'delete=/in-app/delete-account', 'Google=https://policies.google.com/privacy',
        'provider=https://support.google.com/mail/answer/7126229', 'install=#install']),
        `only mailto:, the in-app pages, a policy page, a provider's help and the page itself stay links: ${kept.join(', ')}`);

      /* The reviewers' links: a > inside a quoted value, an SVG link's
         xlink:href, and the rest of what a browser reads that a quick
         pattern does not. Each loses its link and keeps its words. */
      const hostile = '<a title="a > b" href="/account">x</a> <a data-x=">" href="index.html#pricing">y</a> <a xlink:href="/app">z</a> '
        + '<a title="a > b" href="index.html#pricing">prices</a> <a href="&#105;ndex.html#pricing">q</a> <a href="index&period;html">r</a> '
        + '<a href="#install" href="/app">dup</a> <a ping="/app" href="#install">ping</a> <a href=index.html>bare</a> '
        + "<a href='/in-app/terms#ai'>single</a> <A HREF=\"terms.html#ai\">upper</A>";
      const made = storeSafe('help', into(`<p id="hostile">${hostile}</p>`));
      const p = (made.match(/<p id="hostile">([\s\S]*?)<\/p>/) || [])[1] || '';
      check(JSON.stringify(linked(p)) === JSON.stringify(['single=/in-app/terms#ai', 'upper=/in-app/terms#ai'])
        && visible(p) === 'x y z prices q r dup ping bare single upper',
        `a > in a quoted value, xlink:href, a character reference, two addresses or a ping each make the link plain text: ${JSON.stringify(p)}`);
      const urls = BANNED.filter(([, re]) => re.test(p)).map(([what]) => what);
      check(urls.length === 0 && !/\/app\b|\/account\b/.test(p), `and leave no address behind: ${urls.join(', ') || 'none'}`);
      let threw = '';
      try { storeSafe('help', into('<p><a title="a > b" href="index.html#pricing">buy</a></p>')); } catch (e) { threw = e.message; }
      check(/still names Buy/.test(threw), `the reviewers' link with its own word, "buy", makes no copy at all: "${threw}"`);

      for (const [what, bad] of [['a script', '<script>location="/"</script>'], ['a handler', '<p onclick="go()">x</p>'],
        ['a <noscript>', '<noscript><a href="/account">Get RATA Pro now</a> for $12.99 a month</noscript>'],
        ['an <xmp>', '<xmp>RATA Pro, $23.99 a month</xmp>'], ['a <noembed>', '<noembed><a href="/app">x</a></noembed>'],
        ['a <noframes>', '<noframes><a href="/app">x</a></noframes>'], ['a <plaintext>', '<plaintext>RATA Pro, $23.99 a month'],
        ['a refresh', '<meta http-equiv="refresh" content="0;url=/">'], ['a form', '<form action="/"></form>'],
        ['a <link rel="prev">', '<link rel="prev" href="index.html">'], ['a <link> to another site', '<link rel="stylesheet" href="https://fonts.example/x.css">'],
        ['an address on another tag', '<blockquote cite="/app">x</blockquote>'], ['an outside picture', '<img src="https://example.org/x.png" alt="">'],
        ['an SVG', '<svg><a xlink:href="/app"><text>t</text></a></svg>'], ['a link inside a link', '<a href="#install"><a href="#link">x</a></a>'],
        ['a quote left open', '<p><a href="/in-app/terms>11</a></p>']]) {
        let why = '';
        try { storeSafe('help', into(bad)); } catch (e) { why = e.message; }
        check(!!why, `a page that gains ${what} makes no copy, rather than one nobody judged${why ? `: "${why}"` : ''}`);
      }
    }

    console.log('\n— and so is a word that sells, and a mark that no longer fits —');
    {
      const refused = (page, html) => { try { storeSafe(page, html); return ''; } catch (e) { return e.message; } };
      const privacy = read('privacy.html');
      let threw = refused('privacy', privacy.replace(/<!-- in-app:[\s\S]*?-->/g, ''));
      check(/marks nothing/.test(threw), `a page that loses its marks makes no copy at all: "${threw}"`);
      threw = refused('privacy', privacy.replace('<!-- in-app:cut footer --><footer>', '<footer>'));
      check(/without its other half|footer/.test(threw), `nor one whose footer is no longer marked: "${threw}"`);
      /* The card's store wording, read from the page, so these hold
         whatever the sentence says now. */
      const card = [...privacy.matchAll(WORDS)].flatMap(m => wordingOf(m[2])).find(w => w.id === 'card');
      check(!!card && privacy.split(card.old).length === 3, `privacy.html has its "card" store wording: ${card && card.old}`);
      /* As claude/iap-server did to this sentence ("…, or Apple in the
         iPhone app;"): the website's words change, so the store's must be
         decided again. Only the page's own sentence changes, not the mark. */
      threw = refused('privacy', privacy.replace(card.old, card.old.replace(/;$/, ', or Apple in the iPhone app;').replace(/payment$/, 'payment, or Apple')));
      check(/"card" finds its words 0 times[\s\S]*decide what the store copy says now/.test(threw), `nor one whose sentence changed under its store wording: "${threw}"`);
      threw = refused('privacy', privacy.replace('  card: ', '  kard: '));
      check(/"kard" is not in STORE_CUTS/.test(threw), `nor one with a mark nobody named: "${threw}"`);
      threw = refused('privacy', privacy.replace(/\n *pending: [^\n]*/, ''));
      check(/STORE_CUTS\.privacy names pending, which the page no longer marks/.test(threw), `nor one that lost a named piece of wording: "${threw}"`);
      threw = refused('privacy', privacy.replace(`=> ${card.now}`, '=> Stripe takes the card;'));
      check(/"card" still names Stripe/.test(threw), `nor store wording that still sells: "${threw}"`);
      threw = refused('privacy', privacy.replace(`  card: ${card.old} =>`, '  card: we never see the card number. =>'));
      check(/"card" changes words that sell nothing/.test(threw), `nor store wording that changes what sells nothing: "${threw}"`);
      threw = refused('privacy', privacy.replace('<h1>Privacy policy</h1>', '<h1>Privacy policy</h1><p>Pay with Stripe.</p>'));
      check(/still names Stripe/.test(threw), `nor a copy whose page gains a word that sells: "${threw}"`);
      threw = refused('terms', read('terms.html').replace('<h1>Terms of service</h1>', '<h1 title="RATA Pro, $23.99 a month">Terms of service</h1>'));
      check(/still names a price/.test(threw), `in an attribute too: "${threw}"`);
      threw = refused('terms', read('terms.html').replace('</style>', '.lede::after{content:"RATA Pro, $23.99 a month"}</style>'));
      check(/still names a price/.test(threw), `or in a stylesheet's words: "${threw}"`);
      for (const [what, said] of [['sells the plans', 'mailrata.org sells the plans.'], ['bought here', 'Bought here, both renew.'],
        ['also sold', 'Both are also sold in the iPhone app.'], ['pay with', 'Pay with the address on your account.'],
        ['buying', 'By buying a plan you agree.'], ['signing in at', 'Renew it by signing in at mailrata.org.'],
        ['sign in to', 'Sign in to mailrata.org for a new key.'], ['log on to', 'Log on to https://mailrata.org to renew.'],
        ['subscribing through', 'Keep subscribing through mailrata.org.']]) {
        threw = refused('terms', read('terms.html').replace('<h1>Terms of service</h1>', `<h1>Terms of service</h1><p>${said}</p>`));
        check(/still names (Sold at mailrata\.org|Pay with or at|Buy|Sign in at mailrata\.org|Subscribe at mailrata\.org)/.test(threw)
          && sellsIn(said).length > 0, `nor "${what}": "${threw}"`);
      }
      /* But the deletion page may ask for the account it deletes. */
      const asks = 'Sign in with your mailrata.org email address and password.';
      threw = refused('terms', read('terms.html').replace('<h1>Terms of service</h1>', `<h1>Terms of service</h1><p>${asks}</p>`));
      check(!threw && !sellsIn(asks).length, `while "${asks}" sells nothing${threw ? `: "${threw}"` : ''}`);
      threw = refused('help', read('help.html').replace('with the same details.</p>\n  -->', 'with the same details, or buy RATA Pro.</p>\n  -->'));
      check(/adds as "write" names Buy/.test(threw), `nor what a store copy adds, when it sells: "${threw}"`);
      /* claude/iap-server adds a sentence to the licence paragraph and to
         the 90-day line. Their store wording still finds its words, so the
         copy is still made, with what was added; its other changes need
         store wording of their own, and make no copy until they have it.
         (Once it is merged the page says these already, and the
         replacements below change nothing.) */
      const iap = read('terms.html')
        .replace('so RATA works offline.</p>', 'so RATA works offline. In the iPhone app there is no key to paste.</p>')
        .replace('Sign in at mailrata.org to get a new one.</li>', 'Sign in at mailrata.org to get a new one, or, for a subscription bought in the iPhone app, press Restore purchases there.</li>');
      let made = '';
      try { made = visible(storeSafe('terms', iap)); } catch (e) { made = e.message; }
      const licence = made.slice(Math.max(0, made.indexOf('Once your payment')), made.indexOf('Without a current licence'));
      check(/^Once your payment is confirmed, RATA gets a licence key for your plan\. The app checks it on your computer, so RATA works offline\. In the iPhone app there is no key to paste[.:]/.test(licence)
        && /RATA then says how to get a new one, or, for a subscription bought in the iPhone app, press Restore purchases there\./.test(licence),
        `claude/iap-server's additions to the licence section keep their store wording: ${JSON.stringify(licence.slice(0, 160) || made.slice(0, 160))}`);
    }

    console.log('\n— the website\'s service worker leaves them to the network —');
    {
      /* Offline, the worker answers a page it never cached with the app,
         which leads to the prices. */
      const sw = read('sw.js');
      const at = sw.search(/if \(url\.origin === location\.origin && url\.pathname\.startsWith\('\/in-app\/'\)\) return;/);
      check(at > 0 && at < sw.indexOf("caches.match('./app.html')"), 'sw.js never answers an /in-app/ page');
    }

    console.log('\n— the deletion page —');
    const page = pages.get('/in-app/delete-account') || '';
    {
      const text = visible(page);
      check(/<title>[^<]*RATA[^<]*<\/title>/.test(page) && /<h1\b[^>]*>[^<]*RATA[^<]*<\/h1>/.test(page), 'it names RATA, in its title and heading');
      check(!/<nav\b|<footer\b/i.test(page), 'it has no site navigation or footer');
      const list = id => [...((page.match(new RegExp(`<ul id="${id}"[^>]*>([\\s\\S]*?)</ul>`)) || [])[1] || '').matchAll(/<li>([\s\S]*?)<\/li>/g)].map(m => visible(m[1]));
      const removes = list('del-removes');
      const keeps = list('del-keeps');
      check(JSON.stringify(removes) === JSON.stringify(DELETION_REMOVES),
        `what it removes is DELETION_REMOVES, in order: ${JSON.stringify(removes)}`);
      /* The last of DELETION_KEEPS is said from inside the app ("this
         device ... here"); on a web page it is the app's own copy, as the
         privacy policy says it. Stripe's billing records are named as the
         privacy policy's store copy names them, since a store build opens
         this page. */
      const records = [...((pages.get('/in-app/privacy') || '').match(/<section id="delete"[\s\S]*?<\/section>/) || [''])[0]
        .matchAll(/<li>([\s\S]*?)<\/li>/g)].map(m => visible(m[1])).find(t => /billing records/.test(t));
      check(JSON.stringify(keeps.slice(0, 2)) === JSON.stringify(DELETION_KEEPS.slice(0, 2))
        && /^Stripe's billing records/.test(DELETION_KEEPS[2]) && !!records && keeps[2] === records
        && /this device/.test(DELETION_KEEPS.at(-1)) && keeps.length === DELETION_KEEPS.length
        && /^what the RATA app holds on your device, until you uninstall it$/.test(keeps.at(-1) || ''),
        `what it keeps is DELETION_KEEPS, the billing records as the privacy policy's store copy says them and the last as on a web page: ${JSON.stringify(keeps)}`);
      check(/does not cancel/.test(text) && /App Store/.test(text) && /Google Play/.test(text),
        'it says deleting the account does not cancel a subscription paid through Apple or Google');
      /* Google Play asks a deletion page for any further time the data is
         kept: the backups. Said in the privacy policy's own sentence, word
         for word, and every number of days on the page is the policy's,
         at the question and once it is done too. */
      const policy = visible(read('privacy.html'));
      const backups = (policy.match(/Backups of the website’s database[^.]*\./) || [''])[0];
      const days = (backups.match(/within (\d+) days\.$/) || [])[1];
      const apart = `apart from the backups, which go within ${days} days`;
      const counted = [...text.matchAll(/\b(\d+) days\b/g)].map(m => m[1]);
      check(!!days && text.includes(backups) && policy.includes(apart)
        && text.split(apart).length - 1 === 2 && counted.length === 4 && counted.every(d => d === days),
        `it says the backups keep what it removes for up to ${days} days, in the privacy policy's words: ${JSON.stringify(backups.slice(0, 60))}…`);
      /* Someone who cannot sign in (a forgotten password) can still ask,
         where the privacy policy says to write: the policy's address,
         placeholder and all until the owner fills both in, which
         live-check.sh refuses to see on the live site. */
      /* The policy says "write to" earlier too, for anything on the page, so
         the address is what stands between the nearest "write to" and the
         rest of the sentence. */
      const writeTo = t => (t.match(/write to ((?:(?!write to ).)+?) from the address on your account/) || [])[1];
      const signin = (page.match(/<form id="del-signin"[\s\S]*?<\/form>/) || [''])[0];
      const write = visible((signin.match(/<p id="del-write">[\s\S]*?<\/p>/) || [''])[0]);
      check(!!writeTo(policy) && writeTo(write) === writeTo(policy)
        && /^If you cannot sign in, you can still ask for your account to be deleted: write to /.test(write),
        `in the sign-in form, someone who cannot sign in is told where to write, as the privacy policy says: ${JSON.stringify(writeTo(write))}`);
      check(!/[—–]/.test(text) && !/!/.test(text), 'no em dash, en dash or exclamation mark (DESIGN.md)');
      const loads = loadsIn(page, ['script', 'img', 'link'], ['src', 'href']).filter(u => u !== '');
      check(loads.length > 0 && loads.every(u => u != null && u.startsWith('/') && !u.startsWith('//')), `it loads only its own site's files: ${loads.join(', ')}`);
      check(!/\son[a-z]+=/i.test(page) && !/\sstyle=/i.test(page), 'no inline event handler or style attribute');
      const js = [...scripts].filter(([h]) => h.includes('/in-app/')).map(([, j]) => j || '').join('\n');
      check(js.length > 0 && !/innerHTML|outerHTML|insertAdjacentHTML|document\.write/.test(js),
        'its script draws every word with textContent, never as HTML');
      const lib = (read('auth.html').match(/vendor\/supabase-js-[\w.-]+\.js/) || [])[0];
      check(!!lib && js.includes(`'/${lib}'`), `it signs in through the sign-in page's own Supabase client (${lib})`);
    }

    console.log('\n— without a sign-in service, it says so instead of offering a form that cannot work —');
    {
      const browser = await chromium.launch();
      try {
        const pg = await browser.newPage();
        await pg.goto(s.url + '/in-app/delete-account');
        await pg.waitForFunction(() => document.querySelector('#del-off') && !document.querySelector('#del-off').hidden);
        const off = await pg.evaluate(() => ({
          said: document.querySelector('#del-off').textContent.trim(),
          disabled: [...document.querySelectorAll('#del-signin input, #del-signin button')].every(e => e.disabled),
          write: (w => (w && w.offsetParent !== null ? w.textContent.trim() : ''))(document.querySelector('#del-write')),
        }));
        check(off.disabled && /not switched on/i.test(off.said), `the form is off and the page says why: "${off.said}"`);
        check(/^If you cannot sign in/.test(off.write), 'and still says where to write to ask for deletion');
      } finally { await browser.close(); }
    }

    console.log('\n— on a phone, 390 px wide, every page keeps its 16 px side gutter —');
    {
      /* A store build opens these on phones. help.html's layout takes its
         .wrap's side padding away, so its text touches the screen's edge;
         the website's page keeps its look, and its store copy gets the
         gutter back (in-app:add gutter). */
      const browser = await chromium.launch();
      try {
        const pg = await browser.newPage({ viewport: { width: 390, height: 844 } });
        for (const path of START) {
          await pg.goto(s.url + path);
          const m = await pg.evaluate(() => {
            const all = [...document.querySelectorAll('main h1, main h2, main h3, main p, main li, main blockquote, main dl, main aside, main section')]
              .filter(e => e.getClientRects().length);
            const out = all.map(e => ({ e, r: e.getBoundingClientRect() }))
              .filter(({ r }) => r.left < 15.5 || r.right > innerWidth - 15.5)
              .map(({ e, r }) => `${e.tagName.toLowerCase()}${e.id ? '#' + e.id : ''} ${Math.round(r.left)}..${Math.round(r.right)}`);
            const first = document.querySelector('#install') || document.querySelector('main section, main h1');
            return { seen: all.length, out: out.slice(0, 4), wide: document.documentElement.scrollWidth, left: first ? first.getBoundingClientRect().left : -1 };
          });
          check(m.seen > 5 && m.out.length === 0 && m.wide <= 390 && m.left >= 16,
            `${path}: all ${m.seen} blocks sit 16 px or more from each side (the first at ${Math.round(m.left)} px), with no sideways scroll (${m.wide} px)${m.out.length ? `: ${m.out.join(', ')}` : ''}`);
        }
      } finally { await browser.close(); }
    }
  } finally { await s.stop(); }

  await deletesForReal(check);
}

/* The page against the stand-in Supabase the account route's own test
   uses: signing in through the browser, then DELETE /api/account. */
async function deletesForReal(check) {
  const supabase = await fakeSupabase();
  const { USER, fake } = supabase;
  const s = await startServer({ env: {
    SUPABASE_URL: supabase.url,
    SUPABASE_ANON_KEY: fakeSupabaseKey('anon'),
    SUPABASE_SERVICE_ROLE_KEY: fakeSupabaseKey('service_role'),
  } });
  const browser = await chromium.launch();
  const axe = async (pg, where) => {
    await pg.evaluate(() => Promise.race([
      Promise.all(document.getAnimations().map(a => a.finished.catch(() => {}))),
      new Promise(r => setTimeout(r, 1000)),
    ]));
    const r = await new AxeBuilder({ page: pg }).analyze();
    const bad = r.violations.filter(v => v.impact === 'serious' || v.impact === 'critical');
    for (const v of r.violations) console.log(`        axe ${v.impact}: ${v.id} at ${v.nodes.slice(0, 4).map(n => n.target.join(' ')).join(' | ')}`);
    check(bad.length === 0, `axe, ${where}: no serious or critical violation (${r.passes.length} rules pass): ${JSON.stringify(bad.map(v => v.id))}`);
  };
  const focused = pg => pg.evaluate(() => document.activeElement && (document.activeElement.id || document.activeElement.tagName));
  const alertOf = pg => pg.evaluate(() => {
    const a = document.querySelector('#del-msg');
    return a && !a.hidden ? { role: a.getAttribute('role'), text: a.textContent.trim() } : null;
  });
  try {
    for (const scheme of ['light', 'dark']) {
      const ctx = await browser.newContext({ colorScheme: scheme });
      const pg = await ctx.newPage();
      await pg.goto(s.url + '/in-app/delete-account');
      await axe(pg, `the page as it opens, ${scheme}`);
      await ctx.close();
    }

    console.log('\n— by keyboard alone: a wrong password is refused in the sign-in service\'s own words —');
    const ctx = await browser.newContext();
    const pg = await ctx.newPage();
    await pg.goto(s.url + '/in-app/delete-account');
    let tabs = 0;
    while ((await focused(pg)) !== 'del-email' && tabs++ < 12) await pg.keyboard.press('Tab');
    check((await focused(pg)) === 'del-email', `Tab reaches the email field (${tabs} presses)`);
    await pg.keyboard.type('leaver@example.com');
    await pg.keyboard.press('Tab');
    check((await focused(pg)) === 'del-password', 'then the password');
    await pg.keyboard.type('not my password');
    fake.calls = [];
    await pg.keyboard.press('Enter');
    await pg.waitForFunction(() => { const a = document.querySelector('#del-msg'); return a && !a.hidden && a.textContent.trim(); });
    const wrong = await alertOf(pg);
    check(wrong?.role === 'alert' && wrong.text === 'Invalid login credentials', `said as an alert: ${JSON.stringify(wrong)}`);
    check(fake.calls.some(c => c.startsWith('POST /auth/v1/token')) && !fake.calls.some(c => c.startsWith('DELETE ')),
      'the password went to the sign-in service, and nothing was deleted');
    await axe(pg, 'a refused password');

    /* The same refusal a second time (the password mistyped twice, or a
       field left empty twice) is emptied first and said again, so a screen
       reader hears it again: the same words put back into an alert are no
       change it announces. Watched as the alert changes. */
    const sayAgain = async (what, want) => {
      await pg.evaluate(() => {
        const a = document.querySelector('#del-msg');
        const said = window.__said = [];
        if (window.__watch) window.__watch.disconnect();
        window.__watch = new MutationObserver(() => said.push(a.hidden ? '' : a.textContent.trim()));
        window.__watch.observe(a, { attributes: true, childList: true, characterData: true, subtree: true });
      });
      await pg.keyboard.press('Enter');
      const again = await pg.waitForFunction(w => {
        const said = window.__said || [];
        const a = document.querySelector('#del-msg');
        return said.indexOf('') >= 0 && said.slice(said.indexOf('') + 1).includes(w) && !a.hidden && a.textContent.trim() === w;
      }, want, { timeout: 5000 }).then(() => true, () => false);
      const said = await pg.evaluate(() => window.__said);
      check(again && (await focused(pg)) === 'del-password', `${what}: the alert is emptied, then says ${JSON.stringify(want)} again: ${JSON.stringify(said)}`);
    };
    fake.calls = [];
    await sayAgain('the same wrong password again', 'Invalid login credentials');
    check(fake.calls.filter(c => c.startsWith('POST /auth/v1/token')).length === 1, 'and the sign-in really was tried again');
    await pg.keyboard.press('ControlOrMeta+a');
    await pg.keyboard.press('Backspace');
    await pg.keyboard.press('Enter');
    await pg.waitForFunction(() => document.querySelector('#del-msg').textContent.trim() === 'Type your email address and password.');
    fake.calls = [];
    await sayAgain('an empty password twice', 'Type your email address and password.');
    check(!fake.calls.some(c => c.startsWith('POST /auth/v1/token')), 'and nothing went to the sign-in service');

    console.log('\n— signed in, it asks once more, and shows the server\'s own refusal —');
    await pg.keyboard.press('ControlOrMeta+a');
    await pg.keyboard.type(fake.password);
    await pg.keyboard.press('Enter');
    await pg.waitForFunction(() => !document.querySelector('#del-confirm').hidden);
    const asked = await pg.evaluate(() => ({
      signinHidden: document.querySelector('#del-signin').hidden,
      said: document.querySelector('#del-confirm').textContent.replace(/\s+/g, ' ').trim(),
      focus: document.activeElement && document.activeElement.id,
      msg: document.querySelector('#del-msg').hidden,
      password: document.querySelector('#del-password').value,
    }));
    check(asked.signinHidden && asked.said.includes('leaver@example.com'), `it names the account it is about to delete: "${asked.said.slice(0, 90)}…"`);
    check(asked.focus === 'del-confirm-h', `and the focus moves to that question (${asked.focus})`);
    check(asked.msg && asked.password === '', 'the old refusal is gone, and so is the password from the form');
    await axe(pg, 'the question before deleting');

    fake.sub = { plan: 'pro', status: 'active', event_at: null, updated_at: new Date().toISOString() };
    const refusal = blocksDeletion(fake.sub, Date.now(), { store: true });
    await pg.keyboard.press('Tab');
    check((await focused(pg)) === 'del-go', 'Tab reaches Delete my account');
    fake.calls = [];
    await pg.keyboard.press('Enter');
    await pg.waitForFunction(() => { const a = document.querySelector('#del-msg'); return a && !a.hidden && a.textContent.trim(); });
    const refused = await alertOf(pg);
    check(refused?.role === 'alert' && refused.text === refusal && !sellsIn(refused.text).length && refusal !== blocksDeletion(fake.sub),
      `the server's own sentence, in the words it gives a store build: ${JSON.stringify(refused?.text)}`);
    check(fake.calls.some(c => c === 'GET /auth/v1/user') && !fake.calls.some(c => c.startsWith('DELETE ')),
      'the session went with the request, and nothing was deleted');
    check(await pg.evaluate(() => !document.querySelector('#del-confirm').hidden && !document.querySelector('#del-go').disabled),
      'the question stays, to try again once the subscription is cancelled');
    await axe(pg, 'a refusal');

    const backToSignIn = async () => {
      await pg.waitForFunction(() => !document.querySelector('#del-signin').hidden);
      await pg.waitForFunction(() => document.activeElement && document.activeElement.id === 'del-password');
      await pg.waitForFunction(() => !document.querySelector('#del-msg').hidden);
      return pg.evaluate(() => ({
        confirmHidden: document.querySelector('#del-confirm').hidden,
        who: document.querySelector('#del-who').textContent,
        email: document.querySelector('#del-email').value,
        said: (a => (a && !a.hidden ? { role: a.getAttribute('role'), text: a.textContent.trim() } : null))(document.querySelector('#del-msg')),
      }));
    };
    const signInAgain = async () => {
      await pg.keyboard.type(fake.password);
      await pg.keyboard.press('Enter');
      await pg.waitForFunction(() => !document.querySelector('#del-confirm').hidden);
      return pg.evaluate(() => document.querySelector('#del-msg').hidden && document.activeElement.id === 'del-confirm-h');
    };

    console.log('\n— a sign-in that has run out goes back to the form, and says so in the page\'s words —');
    {
      /* The page never renews its session, so an hour on the route answers
         200 with "Session invalid — sign in again" (lib/server.js), a
         sentence for the app, with an em dash. Here the stand-in forgets the
         page's session, as GoTrue does once it expires. */
      for (const t of [...fake.tokens]) if (t !== 'a-session') fake.tokens.delete(t);
      fake.sub = null;
      fake.calls = [];
      await pg.focus('#del-go');
      await pg.keyboard.press('Enter');
      const ended = await backToSignIn();
      check(ended.said?.role === 'alert' && ended.said.text === 'Your sign-in has ended. Sign in again to delete the account.',
        `it goes back to the sign-in and says why: ${JSON.stringify(ended.said)}`);
      check(ended.confirmHidden && ended.who === '' && ended.email === 'leaver@example.com',
        'the question is gone, the email is still filled in, and the focus is on the password');
      check(fake.calls.includes('GET /auth/v1/user') && !fake.calls.some(c => c.startsWith('DELETE ')), 'nothing was deleted');
      await axe(pg, 'a sign-in that ran out');
      check(await signInAgain(), 'signing in again asks the question once more, with nothing left over');
    }

    console.log('\n— a deletion that stops part way says so, without the database\'s words, and asks for a sign-in again —');
    {
      fake.aiUsage = 'broken';
      fake.calls = [];
      await pg.focus('#del-go');
      await pg.keyboard.press('Enter');
      const stopped = await backToSignIn();
      check(stopped.said?.role === 'alert' && stopped.said.text === 'mailrata.org could not finish deleting the account. Sign in again to try once more.',
        `it says the deletion did not finish: ${JSON.stringify(stopped.said)}`);
      check(fake.calls.some(c => c.startsWith('DELETE /rest/v1/ai_usage')) && !fake.calls.some(c => c.startsWith('DELETE /auth/v1/admin')),
        `the login was not deleted, so signing in again works (${fake.calls.filter(c => c.startsWith('DELETE ')).join(', ')})`);
      fake.aiUsage = 'rows';
      check(await signInAgain(), 'and signing in again asks the question once more');
    }

    console.log('\n— and once nothing is in the way, the account goes, and the page says so —');
    fake.sub = null;
    fake.calls = [];
    await pg.focus('#del-go');
    await pg.keyboard.press('Enter');
    await pg.waitForFunction(() => !document.querySelector('#del-done').hidden);
    const done = await pg.evaluate(() => ({
      said: document.querySelector('#del-done').textContent.replace(/\s+/g, ' ').trim(),
      focus: document.activeElement && document.activeElement.id,
      confirmHidden: document.querySelector('#del-confirm').hidden,
      msg: document.querySelector('#del-msg').hidden,
    }));
    check(/deleted/i.test(done.said) && done.confirmHidden && done.msg, `it says it is done: "${done.said}"`);
    check(done.focus === 'del-done-h', `and the focus moves there (${done.focus})`);
    check(fake.calls.includes(`DELETE /auth/v1/admin/users/${USER.id}`) && fake.calls.some(c => c.startsWith('DELETE /rest/v1/ai_usage')),
      'the login and everything keyed to it were deleted');
    await axe(pg, 'the account deleted');
    await ctx.close();

    console.log('\n— Cancel ends this page\'s sign-in only, and starts again —');
    {
      /* The person is signed in elsewhere too, on mailrata.org or another
         device. Backing out of a deletion must leave that alone: the
         client's default sign-out scope, 'global', would end it. */
      const r = await fetch(supabase.url + '/auth/v1/token?grant_type=password', {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ email: USER.email, password: fake.password }),
      });
      const elsewhere = (await r.json()).access_token;
      const c2 = await browser.newContext();
      const p2 = await c2.newPage();
      await p2.goto(s.url + '/in-app/delete-account');
      await p2.fill('#del-email', 'Leaver@Example.com');
      await p2.fill('#del-password', fake.password);
      const before = new Set(fake.tokens);
      await p2.press('#del-password', 'Enter');
      await p2.waitForFunction(() => !document.querySelector('#del-confirm').hidden);
      const mine = [...fake.tokens].filter(t => !before.has(t));
      fake.calls = [];
      await p2.focus('#del-cancel');
      await p2.keyboard.press('Enter');
      await p2.waitForFunction(() => !document.querySelector('#del-signin').hidden);
      await p2.waitForFunction(() => document.activeElement && document.activeElement.id === 'del-email');
      check(fake.calls.filter(c => c.startsWith('POST /auth/v1/logout')).join() === 'POST /auth/v1/logout?scope=local'
        && !fake.calls.some(c => c.startsWith('DELETE ')),
        `the page's own session is ended at the sign-in service (${fake.calls.filter(c => c.includes('logout')).join(', ') || 'no sign-out'}), nothing is deleted, and the focus is back on the email`);
      check(!!elsewhere && mine.length === 1 && !fake.tokens.has(mine[0]) && fake.tokens.has(elsewhere),
        'the sign-in elsewhere is still signed in; only this page\'s has ended');
      const kept = await p2.evaluate(() => [...Object.keys(localStorage), ...Object.keys(sessionStorage)]);
      check(kept.length === 0, `and nothing of the sign-in is kept in the browser (${kept.join(', ') || 'nothing'})`);
      await c2.close();
    }
  } finally {
    await browser.close();
    await s.stop();
    await supabase.close();
  }
}
