/* The store-safe pages under /in-app/ (the store plan's stage 1, item 6).

   Google Play fails a consumption-only app when any link in it "eventually
   leads to" a purchase, so what is checked here is the whole of what a
   store build could reach from these pages: every same-site link is
   followed, every page found must be under /in-app/, an outside link may go
   only to another service's privacy policy, and no link or URL string on
   any of them, or in a script they load, leads to the prices, the licence
   page, the app, the sign-in, Stripe or the releases page. The copies must
   say everything their pages say, from the page as it is now, so the help
   page's and the privacy policy's own tests keep covering them. And the
   deletion page must say what deletion removes and keeps as lib/account.js
   decides it, how long the backups hold it and where to write without a
   sign-in as the privacy policy says them, and work, by keyboard and
   screen reader, against the same stand-in Supabase as the account route's
   own test. */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import AxeBuilder from '@axe-core/playwright';
import { startServer, makeChecker, fakeSupabase, fakeSupabaseKey } from './helpers.mjs';
import { IN_APP_PAGES, storeSafe } from '../lib/in-app.js';
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
const isPolicy = u => u.protocol === 'https:' && !u.port && POLICIES.some(([host, path]) =>
  u.hostname === host && (u.pathname === path || u.pathname.startsWith(path.endsWith('/') ? path : path + '/')));

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
const CUT = /<!-- in-app:cut -->([\s\S]*?)<!-- in-app:end -->/g;

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
    out.push({ tag: m[1].toLowerCase(), attrs, closed });
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
    const at = GOES_BY[tag];
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
        <meta http-equiv=Refresh content="0;url=/app"> <a href="/in-app/terms>11</a>`).map(l => `${l.tag} ${l.url}`);
      const want = ['a /help', 'a /privacy.html', 'a /terms.html', 'a /app', 'a /account', 'a /auth.html', 'a #here', 'form /x',
        'button /y', 'link /app', 'a null', 'meta refresh 0;url=/app'];
      check(JSON.stringify(got) === JSON.stringify(want),
        `single quotes, no quotes, any case, a > in a value, data-href and a quote left open are all read: ${JSON.stringify(got)}`);
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
          if (!isPolicy(u)) wrong.push(`${url}, which is not another service's privacy policy`);
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
      /* What a reader of the page's main part sees is the same, word for
         word, but for what a source marks to leave out, and those marks
         inside the text are only ever a link's few words. */
      const cuts = [...mainOf(source).matchAll(CUT)].map(m => m[1]);
      check(cuts.every(c => /<a\b/i.test(c) && visible(c).length <= 40),
        `${file}: what it leaves out of its text is only links (${cuts.map(c => JSON.stringify(visible(c))).join(', ') || 'none'})`);
      const want = visible(mainOf(source).replace(CUT, ''));
      const got = visible(mainOf(served));
      let at = 0;
      while (at < want.length && want[at] === got[at]) at++;
      check(want.length > 1000 && want === got,
        `/in-app/${name} says all ${want.length} characters of ${file}'s text` +
        (want === got ? '' : `\n         first difference: ${JSON.stringify(want.slice(at, at + 60))} became ${JSON.stringify(got.slice(at, at + 60))}`));
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
    }

    console.log('\n— a link added to a page later is judged too —');
    {
      /* storeSafe keeps only what it knows is safe, so a source that gains
         a link nobody thought about still makes a safe copy. */
      const src = read('help.html').replace('<h1>Help</h1>',
        '<h1>Help</h1><p>New: <a href="index.html#pricing">plans</a>, <a href="/account">licence</a>, '
        + '<a href="https://buy.stripe.com/x">buy</a>, <a href="https://mailrata.org/">home</a>, <a href="//mailrata.org/app">app</a>, '
        + '<a href="privacy.html?x=1">odd</a>, <a href="/delete-account">gone</a>, <a href="https://policies.google.com.evil.example/">fake</a>, '
        + '<a href="mailto:help@example.org">mail</a>, <a href="terms.html#ai">terms</a>, <a href="/in-app/delete-account">delete</a>, '
        + '<a href="https://policies.google.com/privacy">Google</a>, <a href="#install">install</a>.</p>');
      const kept = [...storeSafe('help', src).matchAll(/<a\b[^>]*href="([^"]*)"[^>]*>([^<]*)<\/a>/g)]
        .filter(m => /^(plans|licence|buy|home|app|odd|gone|fake|mail|terms|delete|Google|install)$/.test(m[2]))
        .map(m => `${m[2]}=${m[1]}`);
      check(JSON.stringify(kept) === JSON.stringify(['mail=mailto:help@example.org', 'terms=/in-app/terms#ai',
        'delete=/in-app/delete-account', 'Google=https://policies.google.com/privacy', 'install=#install']),
        `only mailto:, the in-app pages, a policy page and the page itself stay links: ${kept.join(', ')}`);
      const odd = storeSafe('help', read('help.html').replace('<h1>Help</h1>',
        `<h1>Help</h1><p><a href=index.html>bare</a> <a href='/app'>single</a> <a id="here">anchor</a></p>`));
      check(!/<a href=index|<a href='\/app'/.test(odd) && /<p>bare single <a id="here">anchor<\/a><\/p>/.test(odd),
        'an address written without double quotes is not judged, so it is not kept either');
      for (const [what, bad] of [['a script', '<script>location="/"</script>'], ['a handler', '<p onclick="go()">x</p>'],
        ['a refresh', '<meta http-equiv="refresh" content="0;url=/">'], ['a form', '<form action="/"></form>']]) {
        let made = true;
        try { storeSafe('help', read('help.html').replace('<h1>Help</h1>', `<h1>Help</h1>${bad}`)); } catch { made = false; }
        check(!made, `a page that gains ${what} makes no copy, rather than one nobody judged`);
      }
      const unmarked = read('privacy.html').replace(/<!-- in-app:(?:brand|cut|end) -->/g, '');
      let threw = '';
      try { storeSafe('privacy', unmarked); } catch (e) { threw = e.message; }
      check(/marks nothing/.test(threw), `a page that loses its marks makes no copy at all: "${threw}"`);
      threw = '';
      try { storeSafe('privacy', read('privacy.html').replace('<!-- in-app:cut --><footer>', '<footer>')); } catch (e) { threw = e.message; }
      check(/without its other half|footer/.test(threw), `nor one whose footer is no longer marked: "${threw}"`);
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
         privacy policy says it. */
      check(JSON.stringify(keeps.slice(0, -1)) === JSON.stringify(DELETION_KEEPS.slice(0, -1))
        && /this device/.test(DELETION_KEEPS.at(-1)) && keeps.length === DELETION_KEEPS.length
        && /^what the RATA app holds on your device, until you uninstall it$/.test(keeps.at(-1) || ''),
        `what it keeps is DELETION_KEEPS, the last said as on a web page: ${JSON.stringify(keeps)}`);
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
    const refusal = blocksDeletion(fake.sub);
    await pg.keyboard.press('Tab');
    check((await focused(pg)) === 'del-go', 'Tab reaches Delete my account');
    fake.calls = [];
    await pg.keyboard.press('Enter');
    await pg.waitForFunction(() => { const a = document.querySelector('#del-msg'); return a && !a.hidden && a.textContent.trim(); });
    const refused = await alertOf(pg);
    check(refused?.role === 'alert' && refused.text === refusal, `the server's own sentence: ${JSON.stringify(refused?.text)}`);
    check(fake.calls.some(c => c === 'GET /auth/v1/user') && !fake.calls.some(c => c.startsWith('DELETE ')),
      'the session went with the request, and nothing was deleted');
    check(await pg.evaluate(() => !document.querySelector('#del-confirm').hidden && !document.querySelector('#del-go').disabled),
      'the question stays, to try again once the subscription is cancelled');
    await axe(pg, 'a refusal');

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
