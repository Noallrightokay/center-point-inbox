// Takes the website's screenshots of the real interface: the desktop app's
// ui/ (as sync-ui.sh assembles it) with a fake Tauri backend and sample mail,
// under the app's own content security policy. DESIGN.md → Website: the hero
// shows the real app, never a mock-up. Retake after any visible change.
//
// Run from the repository root, after ./sync-ui.sh and `npm ci` in rata-next:
//   (cd desktop/rata-app/ui && python3 -m http.server 3185 --bind 127.0.0.1 &)
//   node desktop/rata-app/harness/shots.mjs http://127.0.0.1:3185 rata-next/public/shots
// Writes app-light.webp and app-dark.webp (1600×975) into the directory given.
//
// Every name, address and line of mail here is invented, and every address is
// at a reserved example domain (RFC 2606), so nothing on the website can look
// like somebody's real mail. Keep it that way.
import pw from '../../../rata-next/node_modules/playwright/index.js';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
const { chromium } = pw;
const B = process.argv[2];
const OUT = process.argv[3];
if (!B || !OUT) {
  console.error('usage: node shots.mjs <base url of ui/> <output directory>');
  process.exit(2);
}
mkdirSync(OUT, { recursive: true });

const ME = 'sam@example.net';
const W = 1600; // the width index.html declares for the picture
const VIEW = { width: 1280, height: 780 }; // drawn at 2x, then scaled to W

const NOW = Date.now();
const MAIL = [
  ['Mireille Okafor', 'mireille@studio.example', 'Revised floor plans for the Hayes St. unit',
    'Attached are the revised plans. We moved the pantry wall 40 cm so the fridge fits, and the stair now clears the window.\n\nThe only open question is the skylight. The supplier can do 90 × 120 or 78 × 140; the second lets more light onto the landing but costs about €310 more. Could you let me know which by Thursday? After that the order window closes until November.\n\nI am on site Wednesday morning if it is easier to talk it through there.\n\nMireille',
    true, false, 0.7, [{ index: 1, name: 'Hayes-St-plans-rev4.pdf', mime: 'application/pdf', size: 2411520 }]],
  ['Tomás Brennan', 'tomas@northfold.example', 'Invoice 2291 for September',
    'Hi, please find invoice 2291 for September attached. Payment terms are 14 days as usual.',
    true, true, 2, [{ index: 1, name: 'invoice-2291.pdf', mime: 'application/pdf', size: 88211 }]],
  ['Example Credit Union', 'statements@bank.example', 'Your October statement is ready',
    'Your statement for the account ending 0000 is now available to view in online banking.', false, false, 3, []],
  ['Priya Venkataraman', 'priya@climbing.example', 'Re: Saturday climbing?',
    'I can do 10am at the bouldering wall. Bring the new shoes, I want to see them. Kiran is coming too.', true, false, 5, []],
  ['Linnea Sjöberg', 'linnea@fjellwerk.example', 'Contract draft v3, two open questions',
    'Two questions before we sign: the notice period in clause 7, and whether the IP assignment covers prior work.', false, true, 9, []],
  ['Dmitri Halvorsen', 'dmitri@photos.example', 'Photos from the Tromsø trip',
    'Finally sorted the photos. The aurora ones came out better than I expected, the rest are mostly of Arne falling over.', false, false, 26, []],
  ['Amara Diallo', 'amara@civic.example', 'Volunteer rota for November',
    'Here is the draft rota. Shout if a date does not work, I will swap people around before Friday.', false, false, 40, []],
  ['Oskar Lindqvist', 'oskar@api.example', 'Quick question about the API limits',
    'Is the 60 requests a minute per key or per account? We are seeing 429s at about 45.', false, false, 55, []],
  ['Fern & Fig Bakery', 'hello@bakery.example', 'Your order is ready for pickup',
    'Order #3318 (1 sourdough, 2 cardamom buns) is ready at the counter until 6pm.', false, false, 70, []],
  ['Keiko Tanabe', 'keiko@bookclub.example', 'Re: Book club, next pick',
    'I vote for the Ishiguro. Short enough that people will actually finish it this time.', false, false, 90, []],
];
const SEED = MAIL.map(([n, a, s, p, unread, starred, hrs, atts], i) => ({
  id: ME + '_' + (100 + i), folder: 'inbox', acct: ME, acct_label: 'Example',
  from_name: n, from_addr: a, to_name: '', to_addr: ME, to_all: [ME], cc: [],
  subject: s, preview: p.replace(/\n+/g, ' '), body: p,
  ts: NOW - hrs * 3600e3, unread, starred, uid: 100 + i, uidvalidity: 7,
  message_id: 'm' + i + '@example.org', reply_to: '', truncated: false, attachments: atts, html: false,
}));

/* Only what the interface asks for while it starts, refreshes and opens one
   message. Anything else is a question these pictures should not need. */
const MOCK = ({ me, seed }) => {
  if (window !== window.top) return;
  const M = { refresh: null };
  window.__mock = M;
  window.__TAURI__ = { core: { invoke: async (cmd, args) => {
    switch (cmd) {
      case 'licence_status':
      case 'set_licence':
        return { licensed: true, message: 'Licensed for RATA Pro until 26 October 2026.',
          plan: { key: 'pro', label: 'RATA Pro', mail: null, chat: 3, split: true, ai: true },
          used: 1, limit: null, renewSoon: false, token: 'v1.sample-licence.sig' };
      case 'list_mailboxes': return [{ email: me, host: 'imap.example.net', port: 993, label: 'Example' }];
      case 'refresh_mail': return M.refresh || { messages: [], problems: [], skipped: [] };
      case 'open_message': {
        const m = seed.find((x) => x.uid === args.uid);
        return { text: m ? m.body : '', truncated: false, attachments: m ? m.attachments : [] };
      }
      case 'reread_mail': return [];
      case 'watching': return [];
      case 'notify_mail': return null;
      case 'check_update': return { enabled: false, current: '0.0.0', releases: '' };
      case 'change_messages': return { ok: true, done: args.uids, gone: [] };
      default: throw 'not needed for screenshots: ' + cmd;
    }
  } } };
  localStorage.setItem('centra_session', JSON.stringify({ uid: 'local_shots', email: me, mode: 'local' }));
};

/* The app's own content security policy, as the packaged app applies it. */
const APP_CSP = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8')).app.security.csp;

const browser = await chromium.launch();
let errors = 0;

async function shoot(scheme) {
  const ctx = await browser.newContext({ viewport: VIEW, colorScheme: scheme, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  await page.route('**/app.html', async (route) => {
    const resp = await route.fetch();
    await route.fulfill({ response: resp, headers: { ...resp.headers(), 'content-security-policy': APP_CSP } });
  });
  page.on('pageerror', (e) => { console.log('  PAGE ERROR: ' + e.message); errors++; });
  await page.addInitScript(MOCK, { me: ME, seed: SEED });
  await page.goto(B + '/app.html');
  await page.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function', null, { timeout: 20000 });
  await page.waitForFunction(() => lastSync > 0 && !SYNCING, null, { timeout: 10000 }).catch(() => {});
  await page.evaluate(async (msgs) => {
    S.settings.name = 'Sam Example';
    document.querySelector('#acct-btn').textContent = acctInitials();
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail', true);
    go('inbox');
  }, SEED);
  await page.waitForTimeout(300);
  await page.evaluate(() => openMail(S.messages.find((m) => m.subj && m.subj.startsWith('Revised floor')).id));
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(3500); // let the "new messages" toast go
  await page.evaluate(() => { const t = document.querySelector('#toast'); if (t) t.classList.remove('show'); });
  await page.waitForTimeout(300);
  const png = await page.screenshot();

  /* Scaled down and encoded in the browser, so no image tool is needed. */
  const enc = await browser.newPage();
  const b64 = await enc.evaluate(async ({ src, w }) => {
    const img = new Image(); img.src = src; await img.decode();
    const c = document.createElement('canvas'); c.width = w; c.height = Math.round(img.height * w / img.width);
    const g = c.getContext('2d'); g.imageSmoothingQuality = 'high'; g.drawImage(img, 0, 0, c.width, c.height);
    return c.toDataURL('image/webp', 0.84).split(',')[1];
  }, { src: 'data:image/png;base64,' + png.toString('base64'), w: W });
  const file = `${OUT}/app-${scheme}.webp`;
  writeFileSync(file, Buffer.from(b64, 'base64'));
  console.log('  wrote ' + file);
  await enc.close();
  await ctx.close();
}

await shoot('light');
await shoot('dark');
await browser.close();
if (errors) { console.log(`${errors} page error(s)`); process.exit(1); }
