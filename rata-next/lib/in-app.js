/* ---------------------------------------------------------------------------
   Store-safe copies of the help page, the privacy policy and the terms.

   Google Play fails a consumption-only app when any link in it "eventually
   leads to" a purchase, and Apple's 3.1.1 counts any call to action that
   sends a customer to pay outside the store, not only links. Every page on
   mailrata.org does both: each carries the site's navigation and footer,
   which lead to the prices, the licence page and the sign-in, and the terms
   name the prices and Stripe's billing portal. So the pages a store build
   opens live under /in-app/, and each is made from its page when the site
   is built (app/in-app/[page]/route.js), so the words are written once and a
   copy cannot drift from them; a page whose copy cannot be made fails the
   build, so it never reaches the live site:

   - What a source marks is changed in the copy only. The marks are
     comments, so the website's own page does not change:

       <!-- in-app:brand --> … <!-- in-app:end -->
         The site navigation; RATA's name, unlinked, takes its place.
       <!-- in-app:cut ID --> … <!-- in-app:end -->
         Left out.
       <!-- in-app:words --> … <!-- in-app:end
         ID: OLD => NEW
       -->
         The store's own wording: in what the marks hold, each OLD, the
         source's own HTML, which must be there exactly once, becomes NEW (or
         nothing). A sentence that changes on the website stops matching, and
         no copy is made until somebody decides what the store copy says
         instead. The marks can stand at a section's edges, away from the
         sentences they change.
       <!-- in-app:add ID
         HTML
       -->
         Something only the store copy has, such as where to write for
         help where the website sends people to GitHub, or the help page's
         side gutter on a phone, which its own layout takes away (a store
         build opens it on phones). On the website it is a comment, so the
         page's look and words do not change.

     Every ID is in STORE_CUTS, and every ID there is in its page. No mark
     sits inside another.
   - Nothing in a copy sells (SELLING): no price, no "a month", no Stripe,
     billing portal, licence page, releases page, installer, "Choose a
     plan", "Upgrade", "Buy", no saying that mailrata.org sells RATA or
     that it was bought "here", no "pay with" an address or at a place, and
     no call to subscribe or sign in at, to or on mailrata.org, in any of
     those words' forms. Each OLD must carry
     one of these and its NEW none, what a store copy adds carries none, and
     a copy that still carries one anywhere, in its text, in an attribute
     or in a quoted string of its stylesheet, is not made.
   - Every link is judged, its attributes read as a browser reads them, and
     only these stay links: a place on the same page, another in-app page
     (sent to /in-app/<name>), mailto:, an outside service's policy page in
     OUTSIDE_POLICIES and a mail provider's own help in PROVIDER_HELP. Any
     other link, and any link whose address cannot be read for certain,
     becomes its own text, so a link added to a source later is safe without
     anyone remembering this file. A tag that could take somebody elsewhere
     and is not a link (a form, a frame, a base address, a <link> that is
     not an icon or a stylesheet, a handler), and one whose contents a
     browser may show without reading them as tags (noscript, xmp, noembed,
     noframes, plaintext), makes no copy at all.
   - Comments go, the stylesheet's own notes too (they name the site's
     other pages), and the page's files (fonts, icons) are asked for from
     the root, since the copy is served one folder down.
   - Search engines are asked to leave the copy out; the original is the
     page to find.

   The deletion page, /in-app/delete-account, is not a copy: it is
   public/in-app/delete-account.html itself (proxy.js), held to the same
   words by tests/in-app.test.mjs.
   --------------------------------------------------------------------------- */

/* Each in-app copy, by the name it is served under, and its source in
   public/. */
export const IN_APP_PAGES = { help: 'help.html', privacy: 'privacy.html', terms: 'terms.html' };

/* The page that is not a copy, which a source may also link to. */
export const DELETE_PAGE = 'delete-account';

/* Every cut and every piece of store wording, by page. A mark whose ID is not
   here makes no copy, and nor does an ID here that its page has lost. */
export const STORE_CUTS = {
  help: ['gutter', 'footer', 'get', 'install-steps', 'key', 'write'],
  privacy: ['footer', 'card', 'record', 'record-early', 'processor', 'processor-records', 'github', 'payment',
    'how', 'records', 'live', 'pending'],
  terms: ['footer', 'agree', 'sells', 'pay-with', 'base-price', 'pro-price', 'billing', 'changes', 'licence-page',
    'new-key', 'stop', 'unsigned'],
};

/* What a store build may never show, in a page's text or its attributes:
   anything that sells, or sends somebody to pay or to the pages that do. */
export const SELLING = [
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
  /* "Buy", and buying a plan, a subscription, a licence or RATA ("By
     buying a plan …"); not "asking again is not buying again". */
  ['Buy', /\bbuy\b|\bbu(?:ys|ying)\s+(?:(?:a|the|your)\s+)?(?:plans?|subscriptions?|licen[cs]es?|RATA)\b/i],
  /* "mailrata.org sells the plans", "sold at mailrata.org", "Bought here"
     and "also sold": on a page of mailrata.org, "here" is the website, and
     "also" is the website besides the store. */
  ['Sold at mailrata.org', /\b(?:sells?|sold|selling|bought)\b[^.]{0,60}?\bmailrata\.org\b|\bmailrata\.org\b[^.]{0,20}?\b(?:sells?|sold|selling)\b|\b(?:bought|sold|buy)\s+here\b|\b(?:also|too)\s+(?:sold|bought)\b/i],
  /* "Pay with the same address as your account": no store payment is made
     with an address, so it can only mean paying somewhere else. */
  ['Pay with or at', /\bpay(?:s|ing)?\s+(?:with|at|on)\b/i],
  /* Subscribing or signing in (or on) at, to, on or through mailrata.org,
     in any form ("signing in at", "Log on to", "subscribed through");
     not "Sign in with your mailrata.org email address", which is how the
     deletion page asks for the account it deletes. */
  ['Subscribe at mailrata.org', /\bsubscri(?:be|bes|bed|bing|ption)\b[^.]{0,40}?\b(?:at|on|to|through|via)\s+(?:https?:\/\/)?(?:www\.)?mailrata\.org\b/i],
  ['Sign in at mailrata.org', /\b(?:sign|log)(?:s|ed|ged|ing|ging)?[ -]?(?:in|on)\b[^.]{0,20}?\b(?:at|to|on|into|through|via)\s+(?:https?:\/\/)?(?:www\.)?mailrata\.org\b/i],
];

/* Outside pages a copy may still link to. https only; the host exactly, and
   the path itself or below it. Other services' policies, which sell
   nothing: */
export const OUTSIDE_POLICIES = [
  { host: 'www.anthropic.com', path: '/legal' },
  { host: 'privacy.microsoft.com', path: '/' },
  { host: 'policies.google.com', path: '/' },
  { host: 'developers.google.com', path: '/terms/api-services-user-data-policy' },
];

/* and the mail providers' own help and account pages, where the help page's
   steps make an app password; they sell nothing of RATA's, and without them
   those steps cannot be followed. */
export const PROVIDER_HELP = [
  { host: 'support.google.com', path: '/' },
  { host: 'myaccount.google.com', path: '/apppasswords' },
  { host: 'help.yahoo.com', path: '/' },
  { host: 'login.yahoo.com', path: '/account/security' },
  { host: 'www.fastmail.help', path: '/' },
  { host: 'support.apple.com', path: '/' },
  { host: 'account.apple.com', path: '/' },
  { host: 'support.hostinger.com', path: '/' },
  { host: 'support.microsoft.com', path: '/' },
  { host: 'www.zoho.com', path: '/mail/help' },
];

const SITE = 'https://mailrata.org/';
const BRAND = '<header><div class="wrap nav-in"><span class="logo">'
  + '<img class="mk" src="icons/mark-256.png" alt="" width="30" height="30"><b>RATA</b>'
  + '</span></div></header>';

const under = (url, list) => url.protocol === 'https:' && !url.port && !url.username && !url.password
  && list.some(({ host, path }) => url.hostname === host
    && (url.pathname === path || url.pathname.startsWith(path.endsWith('/') ? path : path + '/')));
export const isOutsidePolicy = url => under(url, OUTSIDE_POLICIES);
export const isProviderHelp = url => under(url, PROVIDER_HELP);

/* Where a link in a copy of `file` may point, or null when it must become
   plain text. */
export function inAppHref(href, file) {
  if (href.startsWith('#')) return href;
  if (/^mailto:/i.test(href)) return href;
  let u;
  try { u = new URL(href, SITE + file); } catch { return null; }
  if (u.origin === new URL(SITE).origin) {
    const page = u.pathname.match(/^\/(in-app\/)?([a-z-]+)(?:\.html)?$/);
    const name = page && (page[2] === DELETE_PAGE ? (page[1] ? DELETE_PAGE : null)
      : Object.keys(IN_APP_PAGES).find(n => n === page[2]));
    return name && !u.search ? `/in-app/${name}${u.hash}` : null;
  }
  return isOutsidePolicy(u) || isProviderHelp(u) ? href : null;
}

/* ---- reading HTML as a browser does ------------------------------------- */

/* An attribute's value, or null where it holds a character reference this
   file does not read for certain (a browser would read it, so the address
   is not known). */
function attrValue(raw) {
  if (raw == null) return '';
  let sure = true;
  const out = raw.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);?/gi, (m, ref) => {
    if (!m.endsWith(';')) { sure = false; return m; }
    const r = ref.toLowerCase();
    if (r[0] === '#') return String.fromCodePoint(r[1] === 'x' ? parseInt(r.slice(2), 16) : +r.slice(1));
    const known = { amp: '&', quot: '"', apos: "'", lt: '<', gt: '>' }[r];
    if (known == null) sure = false;
    return known ?? m;
  });
  return sure ? out : null;
}

const NAMED = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', rsquo: '’', lsquo: '‘', ldquo: '“',
  rdquo: '”', rarr: '→', larr: '←', hellip: '…', mdash: '—', ndash: '–', middot: '·', copy: '©', times: '×', shy: '' };
/* Text as a reader sees it, for the check on what it says. */
const readable = s => s
  .replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, ref) => (ref[0] === '#'
    ? String.fromCodePoint(/^#x/i.test(ref) ? parseInt(ref.slice(2), 16) : +ref.slice(1))
    : NAMED[ref.toLowerCase()] ?? m))
  .replace(/[\u00ad\u200b-\u200d\u2060\ufeff]/g, '');

const RAW_TEXT = new Set(['script', 'style', 'textarea', 'title', 'xmp', 'noscript', 'noembed', 'noframes', 'iframe', 'plaintext']);
const OPEN = /<(\/?)([a-zA-Z][^\s/>]*)/y;
const GAP = /[\s/]*/y;
const NAME = /[^\s"'>/=]+/y;
const EQ = /\s*=\s*/y;
const BARE = /[^\s>]*/y;

/* The page as a browser's tokenizer splits it: text, comments and tags, a
   tag's attributes read as a browser reads them (double quotes, single
   quotes or none, a > inside quotes, a name in any case, the first of a
   name winning). Throws on a tag or comment that never closes. */
function tokens(html) {
  const out = [];
  let i = 0;
  let text = 0;
  const flush = end => { if (end > text) out.push({ kind: 'text', start: text, end }); };
  while (i < html.length) {
    const lt = html.indexOf('<', i);
    if (lt < 0) break;
    if (html.startsWith('<!--', lt)) {
      const e = html.indexOf('-->', lt + 4);
      if (e < 0) throw new Error('a comment that never closes');
      flush(lt);
      out.push({ kind: 'comment', start: lt, end: e + 3 });
      i = text = e + 3;
      continue;
    }
    if (html[lt + 1] === '!' || html[lt + 1] === '?') {
      const e = html.indexOf('>', lt);
      if (e < 0) throw new Error('a declaration that never closes');
      flush(lt);
      out.push({ kind: 'other', start: lt, end: e + 1 });
      i = text = e + 1;
      continue;
    }
    OPEN.lastIndex = lt;
    const m = OPEN.exec(html);
    if (!m) { i = lt + 1; continue; }
    const name = m[2].toLowerCase();
    const attrs = [];
    let j = OPEN.lastIndex;
    let closed = false;
    while (j < html.length) {
      GAP.lastIndex = j; GAP.exec(html); j = GAP.lastIndex;
      if (html[j] === '>') { closed = true; j++; break; }
      NAME.lastIndex = j;
      if (!NAME.exec(html)) { j++; continue; }
      const at = { name: html.slice(j, NAME.lastIndex).toLowerCase(), raw: null, start: j, end: NAME.lastIndex };
      j = NAME.lastIndex;
      EQ.lastIndex = j;
      if (EQ.exec(html)) {
        j = EQ.lastIndex;
        const q = html[j];
        if (q === '"' || q === "'") {
          const e = html.indexOf(q, j + 1);
          if (e < 0) { j = html.length; break; }
          at.raw = html.slice(j + 1, e);
          j = e + 1;
        } else {
          BARE.lastIndex = j; BARE.exec(html);
          at.raw = html.slice(j, BARE.lastIndex);
          j = BARE.lastIndex;
        }
        at.end = j;
      }
      if (!attrs.some(a => a.name === at.name)) attrs.push(at); else attrs.push({ ...at, again: true });
    }
    if (!closed) throw new Error(`a <${m[1]}${name}> tag that never closes (a quote left open?)`);
    flush(lt);
    out.push({ kind: m[1] ? 'end' : 'start', name, attrs, start: lt, end: j });
    i = text = j;
    if (!m[1] && RAW_TEXT.has(name)) {
      const close = new RegExp(`</${name}(?=[\\s/>])`, 'ig');
      close.lastIndex = j;
      const c = close.exec(html);
      if (!c) throw new Error(`a <${name}> that never closes`);
      out.push({ kind: name === 'title' || name === 'textarea' ? 'text' : 'raw', start: j, end: c.index });
      i = text = c.index;
    }
  }
  flush(html.length);
  return out;
}

/* Every word a page can show, every attribute value and every quoted
   string in its stylesheet (CSS's `content: "…"` draws words too), for the
   check on what it says. */
function wordsOf(html) {
  const parts = [];
  let inStyle = false;
  for (const t of tokens(html)) {
    if (t.kind === 'text') parts.push(html.slice(t.start, t.end));
    if (t.kind === 'start') for (const a of t.attrs) if (a.raw) parts.push(a.raw);
    if (t.kind === 'raw' && inStyle) {
      for (const m of html.slice(t.start, t.end).matchAll(/"((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'/g)) parts.push(m[1] ?? m[2]);
    }
    if (t.kind === 'start' || t.kind === 'end') inStyle = t.kind === 'start' && t.name === 'style';
  }
  return readable(parts.join(' ')).replace(/\s+/g, ' ');
}

/* What a piece of HTML says, as one line. */
const textOf = html => readable(html.replace(/<[^>]*>/g, '')).replace(/\s+/g, ' ').trim();

/* The first selling term in `text`, or null. */
export function sells(text) {
  const hit = SELLING.find(([, re]) => re.test(text));
  return hit ? hit[0] : null;
}

/* ---- the marks ------------------------------------------------------------ */

const MARK = '<!-- in-app:';
const END = '<!-- in-app:end';
const WORDING = /^([a-z0-9-]+): (.+?) =>(?: (.*))?$/;

function applyMarks(name, file, html) {
  const allowed = new Set(STORE_CUTS[name]);
  const seen = new Set();
  const named = id => {
    if (!allowed.has(id)) throw new Error(`${file}: the in-app mark "${id}" is not in STORE_CUTS.${name} (lib/in-app.js)`);
    if (seen.has(id)) throw new Error(`${file}: two in-app marks called "${id}"`);
    seen.add(id);
  };
  let out = '';
  let at = 0;
  let marks = 0;
  for (;;) {
    const s = html.indexOf(MARK, at);
    if (s < 0) break;
    const add = /^<!-- in-app:add ([a-z0-9-]+)\n/.exec(html.slice(s, s + 80));
    if (add) {
      named(add[1]);
      const close = html.indexOf('-->', s);
      if (close < 0) throw new Error(`${file}: an in-app addition that never closes`);
      const put = html.slice(s + add[0].length, close).trim();
      if (!put) throw new Error(`${file}: the in-app addition "${add[1]}" adds nothing`);
      if (put.includes(MARK)) throw new Error(`${file}: an in-app mark inside another`);
      const said = sells(textOf(put));
      if (said) throw new Error(`${file}: what the store copy adds as "${add[1]}" names ${said}: ${put}`);
      out += html.slice(at, s) + put;
      at = close + 3;
      marks++;
      continue;
    }
    const head = /^<!-- in-app:(brand|cut ([a-z0-9-]+)|words) -->/.exec(html.slice(s, s + 80));
    if (!head) throw new Error(`${file}: an in-app mark without its other half, or one this file does not know: ${html.slice(s, s + 40)}`);
    const from = s + head[0].length;
    const e = html.indexOf(END, from);
    if (e < 0) throw new Error(`${file}: an in-app mark without its other half`);
    let body = html.slice(from, e);
    if (body.includes(MARK)) throw new Error(`${file}: an in-app mark inside another`);
    const close = html.indexOf('-->', e);
    if (close < 0) throw new Error(`${file}: an in-app end that never closes`);
    const tail = html.slice(e + END.length, close);
    let put = '';
    if (head[1] === 'words') {
      if (!tail.startsWith('\n')) throw new Error(`${file}: an in-app:words mark whose end carries no wording`);
      const lines = tail.split('\n').map(l => l.trim()).filter(Boolean);
      if (!lines.length) throw new Error(`${file}: an in-app:words mark whose end carries no wording`);
      for (const line of lines) {
        const w = WORDING.exec(line);
        if (!w) throw new Error(`${file}: store wording that is not "ID: OLD => NEW": ${line}`);
        const [, id, old, now = ''] = w;
        named(id);
        const found = body.split(old).length - 1;
        if (found !== 1) {
          throw new Error(`${file}: the store wording "${id}" finds its words ${found} times in what its marks hold, not once. `
            + `The page changed there: decide what the store copy says now. It looked for: ${old}`);
        }
        if (!sells(textOf(old))) throw new Error(`${file}: the store wording "${id}" changes words that sell nothing: ${old}`);
        const still = sells(textOf(now));
        if (still) throw new Error(`${file}: the store wording "${id}" still names ${still}: ${now}`);
        body = body.replace(old, () => now);
      }
      put = body;
    } else {
      if (tail !== ' ') throw new Error(`${file}: only an in-app:words mark carries wording at its end`);
      if (head[2]) named(head[2]); else put = BRAND;
    }
    out += html.slice(at, s) + put;
    at = close + 3;
    marks++;
  }
  out += html.slice(at);
  if (!marks) throw new Error(`${file} marks nothing to leave out of its in-app copy`);
  const lost = [...allowed].filter(id => !seen.has(id));
  if (lost.length) throw new Error(`${file}: STORE_CUTS.${name} names ${lost.join(', ')}, which the page no longer marks`);
  return out;
}

/* ---- the copy ------------------------------------------------------------- */

/* Tags a copy may not carry. noscript, xmp, noembed, noframes and
   plaintext are read here as raw text, as script and style are, so their
   contents (which a browser may show, or read as tags after all) are never
   judged; a page that holds one makes no copy. */
const REFUSED = new Set(['form', 'iframe', 'frame', 'frameset', 'object', 'embed', 'applet', 'area', 'base', 'script',
  'svg', 'math', 'portal', 'noscript', 'xmp', 'noembed', 'noframes', 'plaintext']);
/* Attributes that hold an address. */
const ADDRESS = /^(?:href|src|srcset|action|formaction|data|poster|ping|background|cite|longdesc|codebase|archive|classid|usemap|lowsrc|dynsrc|manifest|icon|profile|itemtype|xmlns)$|:href$/;
const PART_OF_THE_PAGE = new Set(['icon', 'apple-touch-icon', 'stylesheet']);
/* One of this site's own files, asked for from the root. */
const ownFile = v => (v != null && v !== '' && !/^[a-z][a-z0-9+.-]*:|^\/\/|^[#?]|\\/i.test(v)
  ? (v.startsWith('/') ? v : '/' + v) : null);
const encode = s => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;');

/* The in-app copy of one page, from its source's HTML. Throws rather than
   make a copy that still carries the site's navigation or footer, a tag it
   cannot judge, or a word that sells. */
export function storeSafe(name, html) {
  const file = IN_APP_PAGES[name];
  if (!file) throw new Error(`there is no in-app page called ${name}`);
  const marked = applyMarks(name, file, html);
  if (/<(?:nav|footer)\b/i.test(marked)) throw new Error(`${file}: the site navigation or footer is not marked to be left out`);

  let toks;
  try { toks = tokens(marked); } catch (e) { throw new Error(`${file}: ${e.message}`); }
  const edits = [];
  const refuse = why => { throw new Error(`${file}: ${why}, which the in-app copy cannot judge`); };
  for (let k = 0; k < toks.length; k++) {
    const t = toks[k];
    if (t.kind === 'comment') { edits.push({ start: t.start, end: t.end, put: '' }); continue; }
    if (t.kind !== 'start') continue;
    const addresses = t.attrs.filter(a => ADDRESS.test(a.name));
    if (REFUSED.has(t.name)) refuse(`a <${t.name}>`);
    /* An address with a tag or a line break in it is a quote left open,
       which has taken in the page after it. */
    if (addresses.some(a => /[<>\n]/.test(a.raw || ''))) refuse(`an address on a <${t.name}> that runs on (a quote left open?)`);
    if (t.name === 'meta' && t.attrs.some(a => a.name === 'http-equiv')) refuse('a <meta http-equiv>');
    if (t.attrs.some(a => /^on/.test(a.name))) refuse(`a handler on a <${t.name}>`);
    if (t.name === 'a') {
      if (!addresses.length) continue;
      /* The link's end, so that a link that cannot stay leaves its words. */
      let end = k + 1;
      while (end < toks.length && toks[end].name !== 'a') end++;
      if (end >= toks.length || toks[end].kind !== 'end') refuse('a link inside a link, or one that never ends');
      const href = addresses.length === 1 && addresses[0].name === 'href' && !addresses[0].again ? addresses[0] : null;
      const was = href && attrValue(href.raw);
      const keep = was == null ? null : inAppHref(was, file);
      if (keep == null) {
        edits.push({ start: t.start, end: t.end, put: '' }, { start: toks[end].start, end: toks[end].end, put: '' });
      } else if (keep !== was) {
        edits.push({ start: href.start, end: href.end, put: `href="${encode(keep)}"` });
      }
      continue;
    }
    if (t.name === 'link' || t.name === 'img') {
      const at = t.name === 'link' ? 'href' : 'src';
      if (t.name === 'link') {
        const rel = attrValue((t.attrs.find(a => a.name === 'rel') || {}).raw);
        const rels = (rel || '').toLowerCase().split(/\s+/).filter(Boolean);
        if (!rels.length || !rels.every(r => PART_OF_THE_PAGE.has(r))) refuse(`a <link rel="${rel}">`);
      }
      const own = addresses.length === 1 && addresses[0].name === at && !addresses[0].again ? addresses[0] : null;
      const path = own && ownFile(attrValue(own.raw));
      if (!path) refuse(`a <${t.name}> that does not load one of the site's own files`);
      if (path !== attrValue(own.raw)) edits.push({ start: own.start, end: own.end, put: `${at}="${encode(path)}"` });
      continue;
    }
    if (addresses.length) refuse(`an address on a <${t.name}>`);
  }
  let out = '';
  let at = 0;
  for (const e of edits.sort((a, b) => a.start - b.start)) {
    out += marked.slice(at, e.start) + e.put;
    at = e.end;
  }
  out += marked.slice(at);
  out = out.replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, css => css.replace(/\/\*[\s\S]*?\*\//g, ''));

  const said = sells(wordsOf(out));
  if (said) throw new Error(`${file}: the in-app copy still names ${said}; mark it in the page (STORE_CUTS)`);

  const head = out.replace(/<meta charset="UTF-8">/i, m => `${m}\n<meta name="robots" content="noindex">`);
  if (head === out) throw new Error(`${file} has no <meta charset="UTF-8"> to follow`);
  return head.replace(/^<!DOCTYPE html>\n?/i, m =>
    `${m}<!-- The store-safe copy of public/${file}, made by lib/in-app.js when the site is built. Change that page, not this copy. -->\n`);
}
