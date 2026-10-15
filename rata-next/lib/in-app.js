/* ---------------------------------------------------------------------------
   Store-safe copies of the help page, the privacy policy and the terms.

   Google Play fails a consumption-only app when any link in it "eventually
   leads to" a purchase, and every page on mailrata.org does: each carries
   the site's navigation and footer, which lead to the prices, the licence
   page and the sign-in, and the help page links the GitHub releases. So the
   pages a store build opens live under /in-app/, and each is made from its
   page on every request (app/in-app/[page]/route.js), so the words are
   written once and a copy cannot drift from them:

   - What a source marks is left out: `<!-- in-app:cut -->` up to
     `<!-- in-app:end -->` goes, and `<!-- in-app:brand -->` up to
     `<!-- in-app:end -->` goes with RATA's name, unlinked, in its place.
     The marks are comments, so the website's own page does not change.
   - Every link is judged, and only these stay links: a place on the same
     page, another in-app page (sent to /in-app/<name>), mailto:, and an
     outside service's policy page in OUTSIDE_POLICIES. Any other link
     becomes its own text, so a link added to a source later is safe without
     anyone remembering this file.
   - The stylesheet's own notes go (they name the site's other pages), and
     the page's files (fonts, icons) are asked for from the root, since the
     copy is served one folder down.
   - Search engines are asked to leave the copy out; the original is the
     page to find.

   The deletion page, /in-app/delete-account, is not a copy: it is
   public/in-app/delete-account.html itself (proxy.js).
   --------------------------------------------------------------------------- */

/* Each in-app copy, by the name it is served under, and its source in
   public/. */
export const IN_APP_PAGES = { help: 'help.html', privacy: 'privacy.html', terms: 'terms.html' };

/* The page that is not a copy, which a source may also link to. */
export const DELETE_PAGE = 'delete-account';

/* Outside pages a copy may still link to: other services' policies, which
   sell nothing. https only; the host exactly, and the path itself or below
   it. */
export const OUTSIDE_POLICIES = [
  { host: 'www.anthropic.com', path: '/legal' },
  { host: 'privacy.microsoft.com', path: '/' },
  { host: 'policies.google.com', path: '/' },
  { host: 'developers.google.com', path: '/terms/api-services-user-data-policy' },
];

const SITE = 'https://mailrata.org/';
const MARK = /<!-- in-app:(cut|brand) -->([\s\S]*?)<!-- in-app:end -->/g;
const BRAND = '<header><div class="wrap nav-in"><span class="logo">'
  + '<img class="mk" src="icons/mark-256.png" alt="" width="30" height="30"><b>RATA</b>'
  + '</span></div></header>';

const decode = s => s.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
const encode = s => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;');

export function isOutsidePolicy(url) {
  return url.protocol === 'https:' && !url.port && !url.username && !url.password
    && OUTSIDE_POLICIES.some(({ host, path }) => url.hostname === host
      && (url.pathname === path || url.pathname.startsWith(path.endsWith('/') ? path : path + '/')));
}

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
  return isOutsidePolicy(u) ? href : null;
}

/* The in-app copy of one page, from its source's HTML. Throws rather than
   make a copy that still carries the site's navigation or footer. */
export function storeSafe(name, html) {
  const file = IN_APP_PAGES[name];
  if (!file) throw new Error(`there is no in-app page called ${name}`);
  let marks = 0;
  let out = html.replace(MARK, (all, kind) => { marks++; return kind === 'brand' ? BRAND : ''; });
  if (!marks) throw new Error(`${file} marks nothing to leave out of its in-app copy`);
  if (/<!-- in-app:(?:cut|brand|end) -->/.test(out)) throw new Error(`${file}: an in-app mark without its other half`);
  if (/<(?:nav|footer)\b/i.test(out)) throw new Error(`${file}: the site navigation or footer is not marked to be left out`);
  if (/<(?:form|iframe|frame|object|embed|area|base|script)\b|<meta\b[^>]*http-equiv|<[a-z][^>]*\son[a-z]+\s*=/i.test(out)) {
    throw new Error(`${file}: a form, frame, script, base address or handler the in-app copy cannot judge`);
  }

  out = out.replace(/<a\b([^>]*)>([\s\S]*?)<\/a>/gi, (all, attrs, inner) => {
    const at = attrs.match(/\shref="([^"]*)"/i);
    /* An address written any other way is not judged, so it is not kept. */
    if (!at) return /\shref\b/i.test(attrs) ? inner : all;
    const was = decode(at[1]);
    const keep = inAppHref(was, file);
    if (keep == null) return inner;
    return keep === was ? all : `<a${attrs.replace(at[0], ` href="${encode(keep)}"`)}>${inner}</a>`;
  });
  out = out.replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, css => css.replace(/\/\*[\s\S]*?\*\//g, ''));
  out = out.replace(/(\s(?:src|href)=")(?![#/]|[a-z][a-z0-9+.-]*:)/gi, '$1/');

  const head = out.replace(/<meta charset="UTF-8">/i, m => `${m}\n<meta name="robots" content="noindex">`);
  if (head === out) throw new Error(`${file} has no <meta charset="UTF-8"> to follow`);
  return head.replace(/^<!DOCTYPE html>\n?/i, m =>
    `${m}<!-- The store-safe copy of public/${file}, made by lib/in-app.js on every request. Change that page, not this copy. -->\n`);
}
