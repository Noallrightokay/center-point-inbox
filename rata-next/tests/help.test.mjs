/* The help page (H2).

   What a tester reads when a mailbox will not link, so the checks are about
   it staying true: every provider the live smoke test covers has a section,
   every link leaves only for the provider's own site (or RATA's own
   repository) and only over https, and every sentence the page says RATA
   shows is still one RATA can say. That last check reads the engine's and
   the app's source, so rewording an error there without updating the page
   fails here rather than leaving a tester unable to find their message. */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { startServer, makeChecker } from './helpers.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const SITE = join(HERE, '..');
const REPO = join(SITE, '..');
const read = p => readFileSync(join(REPO, p), 'utf8');

/* Where each provider section may send people: that provider's own
   domains, and nothing else. RATA's own repository (releases, the Beta bug
   template, BETA.md) is allowed anywhere on the page. */
const PROVIDER_DOMAINS = {
  gmail: ['google.com'],
  icloud: ['apple.com'],
  yahoo: ['yahoo.com'],
  fastmail: ['fastmail.com', 'fastmail.help'],
  'custom-domain': ['google.com', 'hostinger.com'],
  microsoft: ['microsoft.com'],
};
const OWN_REPO = { host: 'github.com', path: '/Noallrightokay/center-point-inbox/' };

/* The six the card names. */
const CARD_SECTIONS = ['gmail', 'icloud', 'yahoo', 'fastmail', 'custom-domain', 'microsoft'];

/* The failures the card names, by the id of their heading on the page. */
const ERROR_TOPICS = {
  'err-certificate': 'certificate not trusted',
  'err-password': 'password refused, use an app password',
  'err-no-server': 'no server found',
  'err-local': 'refused to connect to a local address',
  'err-microsoft': 'Microsoft needs its own sign-in',
};

/* Where the sentences the page quotes come from. discover.rs holds MS_HELP
   and MS365_HELP, and app.html the form's own Microsoft sentences. */
const SOURCES = [
  'desktop/rata-mail/src/imap.rs',
  'desktop/rata-mail/src/smtp.rs',
  'desktop/rata-mail/src/resolve.rs',
  'desktop/rata-mail/src/guard.rs',
  'desktop/rata-mail/src/discover.rs',
  'desktop/rata-mail/src/credential.rs',
  'desktop/rata-app/src-tauri/src/core.rs',
  'rata-next/public/app.html',
];

const squash = s => s.replace(/\s+/g, ' ').trim();
const entities = s => s
  .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"')
  .replace(/&#39;/g, "'").replace(/&amp;/g, '&');
const hostIn = (host, domains) => domains.some(d => host === d || host.endsWith('.' + d));

/* Each provider's block, from its opening tag to the next one or the end
   of the section. */
function providerBlocks(html) {
  const out = {};
  const re = /<div class="prov" id="([^"]+)">/g;
  const starts = [...html.matchAll(re)];
  starts.forEach((m, i) => {
    const from = m.index;
    const next = starts[i + 1]?.index ?? html.indexOf('</section>', from);
    out[m[1]] = html.slice(from, next);
  });
  return out;
}

export default async function run(state) {
  const check = makeChecker(state);
  const html = readFileSync(join(SITE, 'public/help.html'), 'utf8');
  const blocks = providerBlocks(html);

  console.log('\n— the page is served, on a clean URL too —');
  {
    const s = await startServer();
    try {
      for (const path of ['/help', '/help.html']) {
        const r = await fetch(s.url + path);
        const body = await r.text();
        check(r.status === 200 && body.includes('id="errors"'), `${path} -> ${r.status}, the help page`);
      }
    } finally { await s.stop(); }
  }

  console.log('\n— it is linked from the landing page and from BETA.md —');
  {
    const index = readFileSync(join(SITE, 'public/index.html'), 'utf8');
    const footer = index.slice(index.lastIndexOf('<footer'));
    check(/<a href="help\.html">Help<\/a>/.test(footer), 'index.html footer links Help');
    check(/https:\/\/mailrata\.org\/help\b/.test(read('BETA.md')), 'BETA.md links https://mailrata.org/help');
    const proxy = readFileSync(join(SITE, 'proxy.js'), 'utf8');
    check(/'\/help':\s*'\/help\.html'/.test(proxy), 'proxy.js serves /help, the address the app will open');
  }

  console.log('\n— every provider in SMOKE.md\'s grid has a section —');
  {
    const smoke = read('docs/SMOKE.md');
    const header = smoke.split('\n').find(l => /^\|\s*#\s*\|\s*Row\s*\|/.test(l));
    const columns = header ? header.split('|').map(c => c.trim()).filter(Boolean).slice(2) : [];
    check(columns.length >= 6, `the grid names ${columns.length} providers: ${columns.join(', ')}`);
    for (const name of columns) {
      const home = Object.entries(blocks).find(([, b]) =>
        [...b.matchAll(/<h[34][^>]*>([\s\S]*?)<\/h[34]>/g)].some(h => h[1].includes(name)));
      check(!!home, `${name} has a section${home ? ` (#${home[0]})` : ''}`);
    }
    for (const id of CARD_SECTIONS) {
      check(!!blocks[id], `the card's ${id} section is there`);
    }
    for (const id of ['gmail', 'icloud', 'yahoo', 'fastmail', 'custom-domain']) {
      const b = blocks[id] || '';
      check(/<a href="https:\/\//.test(b), `${id} links to the provider's own page`);
    }
    const ms = blocks.microsoft || '';
    check(/release note/.test(ms) && /Sign in with Microsoft/.test(ms),
      'Microsoft: Sign in with Microsoft, live only once a release note says so');
    check(/app password/.test(blocks['custom-domain'] || '') && /Hostinger has no app passwords/.test(blocks['custom-domain'] || ''),
      'your own domain: which password, and Hostinger\'s own');
  }

  console.log('\n— every link out is https, to the provider or to RATA\'s own repository —');
  {
    const outside = Object.values(blocks).reduce((h, b) => h.replace(b, ''), html);
    const links = [];
    for (const [where, part] of [...Object.entries(blocks), ['elsewhere', outside]]) {
      for (const m of part.matchAll(/\b(?:href|src)="([^"]+)"/g)) {
        links.push({ where, url: entities(m[1]) });
      }
    }
    const external = links.filter(l => /^[a-z][a-z0-9+.-]*:|^\/\//i.test(l.url));
    check(external.length >= 10, `${external.length} links out found`);
    for (const { where, url } of external) {
      let u;
      try { u = new URL(url); } catch { check(false, `${url} parses`); continue; }
      const ours = u.hostname === OWN_REPO.host && u.pathname.startsWith(OWN_REPO.path);
      const theirs = where !== 'elsewhere' && hostIn(u.hostname, PROVIDER_DOMAINS[where] || []);
      check(u.protocol === 'https:' && (ours || theirs),
        `${where}: ${url}`);
    }
    /* No third-party requests, as on the rest of the site. */
    const loads = [...html.matchAll(/<(?:script|img|link)\b[^>]*\b(?:src|href)="([^"]+)"/g)].map(m => m[1]);
    check(loads.length > 0 && loads.every(u => !/^[a-z]+:|^\/\//i.test(u)), 'everything the page loads is its own');
  }

  console.log('\n— the sentences it quotes are ones RATA can say —');
  {
    const code = squash(SOURCES.map(read).join('\n')
      .replace(/\\"/g, '"').replace(/\\'/g, "'")
      .replace(/\\u\{?2019\}?/g, '’'));
    const quotes = [...html.matchAll(/<blockquote class="said">([\s\S]*?)<\/blockquote>/g)].map(m => m[1]);
    check(quotes.length >= 15, `${quotes.length} sentences quoted`);
    for (const q of quotes) {
      const parts = q.split(/<var>[\s\S]*?<\/var>/).map(p => squash(entities(p.replace(/<[^>]+>/g, ''))));
      const missing = parts.filter(p => /[a-z].*[a-z]/i.test(p) && !code.includes(p));
      const shown = squash(entities(q.replace(/<var>([\s\S]*?)<\/var>/g, '{$1}')));
      check(missing.length === 0, `in the code: ${shown.slice(0, 90)}${shown.length > 90 ? '…' : ''}` +
        (missing.length ? `\n         not found: ${JSON.stringify(missing)}` : ''));
    }
    for (const [id, what] of Object.entries(ERROR_TOPICS)) {
      const at = html.indexOf(`id="${id}"`);
      const next = html.indexOf('<h3', at + 1);
      const part = at < 0 ? '' : html.slice(at, next < 0 ? undefined : next);
      check(/class="said"/.test(part) && /What it means/.test(part), `${what}: quoted and explained`);
    }
  }

  console.log('\n— install matches BETA.md §1, and reporting says what never to paste —');
  {
    const beta = read('BETA.md');
    const install = beta.slice(beta.indexOf('## 1. Install'), beta.indexOf('## 2.'));
    const files = [...new Set(install.match(/RATA_<version>_[A-Za-z0-9_.-]+\.(?:exe|dmg|deb|AppImage)/g) || [])];
    const text = entities(html);
    check(files.length >= 5, `BETA.md §1 names ${files.length} files`);
    for (const f of files) check(text.includes(f), `the page names ${f}`);
    for (const step of ['More info', 'Run anyway', 'Privacy & Security', 'Open Anyway', 'chmod +x', 'sudo apt install ./RATA_<version>_amd64.deb', 'RATA <version> beta']) {
      check(text.includes(step), `install says "${step}"`);
    }
    /* macOS 15 took away right-click, Open for an unsigned app; Open Anyway
       in System Settings is the way past Gatekeeper now, in both places. */
    check(!/right-click/i.test(text) && install.includes('**Open Anyway**'),
      'macOS: Open Anyway in System Settings, not right-click, Open (here and in BETA.md §1)');
    const report = html.slice(html.indexOf('id="report"'));
    check(report.includes('https://github.com/Noallrightokay/center-point-inbox/issues/new?template=beta-bug.md'),
      'report links the Beta bug template');
    const never = (report.match(/<div class="never">([\s\S]*?)<\/div>/) || [])[1] || '';
    check(/app password/.test(never) && /licence key/.test(never), 'never paste: the app password and the licence key');
    check(/exact error text/i.test(report) && /version/i.test(report) && /provider/i.test(report),
      'what to include: the exact error, the version, the provider');
  }

  console.log('\n— copy rules: no em dash but in RATA\'s own quoted words —');
  {
    const bare = html.replace(/<blockquote class="said">[\s\S]*?<\/blockquote>/g, '');
    const dashes = (bare.match(/—/g) || []).length;
    check(dashes === 0, `no em dash outside the quotes (${dashes})`);
    check(!/!\s*<\//.test(bare.replace(/<!--[\s\S]*?-->|<!DOCTYPE[^>]*>/gi, '')), 'no exclamation marks');
  }
}
