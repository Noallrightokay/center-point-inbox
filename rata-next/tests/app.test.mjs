/* Browser-level behaviour: signup, the two-file Format Bridge, and the
   in-house/offline guarantees. Runs against the production build in Chromium. */
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { startServer, makeChecker } from './helpers.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const CSV = join(HERE, 'fixtures', 'sample.csv');

const ENGINES = [
  '/vendor/supabase-js-2.112.4.js',
  '/vendor/mammoth-1.8.0.browser.min.js',
  '/vendor/xlsx-0.20.3.full.min.js',
  '/vendor/jspdf-2.5.1.umd.min.js',
];

export default async function run(state) {
  const check = makeChecker(state);
  const s = await startServer();
  const browser = await chromium.launch();

  try {
    /* ---- boot: no session must redirect cleanly, with a session must finish ---- */
    console.log('\n— boot —');
    {
      const ctx = await browser.newContext();
      const page = await ctx.newPage();
      const errs = [];
      page.on('pageerror', e => errs.push(e.message));
      await page.goto(s.url + '/app.html');
      await page.waitForURL(/auth\.html$/, { timeout: 15000 }).catch(() => {});
      check(errs.length === 0 && /auth\.html$/.test(page.url()),
        errs.length ? `no session threw: ${errs.join(' | ')}` : 'no session redirects to auth.html without throwing');
      await ctx.close();
    }

    const ctx = await browser.newContext({ acceptDownloads: true });
    const page = await ctx.newPage();
    const errs = [];
    const hosts = new Set();
    page.on('pageerror', e => errs.push(e.message));
    page.on('request', r => { try { hosts.add(new URL(r.url()).host); } catch {} });

    /* ---- signup with nothing configured: RATA must still be usable ---- */
    console.log('\n— signup with no backend configured —');
    /* A first-time visitor must land on the form they came for. Nearly all of
       them are new, and the old default made them find a tab first. */
    await page.goto(s.url + '/auth.html');
    check(await page.isVisible('#s-email'), 'a new visitor lands on the signup form');
    check(await page.isHidden('#login-form'), 'the login form is not what greets them');
    check((await page.getAttribute('#tab-signup', 'class') || '').includes('on'),
      'the Create account tab is the one highlighted');
    check(!(await page.isVisible('#l-email')) , 'no stray second email field on screen');

    /* ?mode= still wins, so the sign-out link keeps working. */
    await page.goto(s.url + '/auth.html?mode=login');
    check(await page.isVisible('#l-email'), '?mode=login still opens Log in');

    /* And the home page's "Log in" link must actually mean Log in, even for a
       visitor this browser has never seen. */
    await page.goto(s.url + '/index.html');
    const loginHref = await page.getAttribute('a.plain:has-text("Log in")', 'href');
    check(/mode=login/.test(loginHref || ''), `home page Log in link -> ${loginHref}`);
    await page.goto(s.url + '/' + loginHref);
    check(await page.isVisible('#l-email'), 'and following it opens the Log in form');

    await page.goto(s.url + '/auth.html?mode=signup');
    /* With no backend the badge must say the account is device-only, and must
       not claim it syncs — that promise is the one thing a user would act on. */
    check(!/all your devices/i.test(await page.textContent('#mode-badge')),
      `mode badge: "${(await page.textContent('#mode-badge')).trim()}"`);

    /* Signup is one screen: email, password, name, done. */
    await page.fill('#s-email', 'owner@example.com');
    await page.fill('#s-pw', 'S3cure-pass-2026');
    await page.fill('#s-name', 'Owner');
    check(await page.isHidden('#step2'), 'no second signup step to wade through');
    check((await page.textContent('#s-next')).includes('Create'), `button says: "${(await page.textContent('#s-next')).trim()}"`);
    await page.click('#s-next');
    await page.waitForURL(/app\.html/, { timeout: 25000 });
    await page.waitForFunction(() => typeof S !== 'undefined' && !!S, null, { timeout: 25000 });

    const sess = await page.evaluate(() => ({
      mode: JSON.parse(localStorage.getItem('centra_session')).mode,
      msgs: S.messages.length,
      rail: document.querySelector('#rail-acct').textContent,
    }));
    check(sess.mode === 'local', `account mode: ${sess.mode}`);
    check(sess.msgs === 0, `inbox starts empty: ${sess.msgs} messages`);
    check(sess.rail === 'owner@example.com', `boot ran to completion (#rail-acct = ${sess.rail})`);


    /* Having signed in once, this browser is no longer a first-timer. Return to
       the app afterwards — the suites below run against a booted app.html. */
    await page.goto(s.url + '/auth.html');
    check(await page.isVisible('#l-email'), 'a returning visitor opens on Log in instead');
    await page.goto(s.url + '/app.html');
    await page.waitForFunction(() => {
      const el = document.querySelector('#rail-acct');
      return !!el && el.textContent.trim().length > 0;
    });
    if (await page.$('#welcome-ov.open')) {
      await page.click('#w-enter');
      await page.waitForSelector('#welcome-ov.open', { state: 'detached', timeout: 8000 }).catch(() => {});
    }

    /* ---- a new workspace arrives switched off ---- */
    console.log('\n— nothing is connected until the user connects it —');
    const fresh = await page.evaluate(() => ({
      plugins: S.plugins, connections: S.connections, linked: S.linked.length,
    }));
    const onByDefault = [
      ...Object.entries(fresh.plugins).filter(([, v]) => v).map(([k]) => 'plugin:' + k),
      ...Object.entries(fresh.connections).filter(([, v]) => v).map(([k]) => 'connection:' + k),
    ];
    check(onByDefault.length === 0, onByDefault.length
      ? `switched on without asking: ${onByDefault.join(', ')}`
      : 'every plugin and connection starts off');
    check(fresh.linked === 0, `no accounts assumed: ${fresh.linked} linked`);

    /* ---- one mail form, any provider ---- */
    console.log('\n— adding an email account —');
    await page.click('[data-view="conn"], [data-view="set"]').catch(() => {});
    const addButtons = await page.$$eval('[data-addlink]', bs => bs.map(b => b.dataset.addlink));
    check(addButtons.includes('mail'), `add buttons: ${addButtons.join(', ')}`);
    check(!addButtons.some(b => b.startsWith('imap:')),
      'no per-provider Gmail/iCloud buttons left to choose between');

    await page.evaluate(() => document.querySelector('[data-addlink="mail"]').click());
    const form = await page.evaluate(() => ({
      title: document.querySelector('#lf-title').textContent,
      sub: document.querySelector('#lf-sub').textContent,
      placeholder: document.querySelector('#lf-input').placeholder,
      passShown: document.querySelector('#lf-pass').style.display !== 'none',
      hostShown: document.querySelector('#lf-host').style.display !== 'none',
    }));
    check(/email account/i.test(form.title), `form title: "${form.title}"`);
    check(form.placeholder === 'you@anywhere.com', `placeholder invites any provider: "${form.placeholder}"`);
    check(form.passShown, 'it asks for an app password');
    check(!form.hostShown, 'and does not ask for a server address unless it has to');
    check(!/gmail|icloud/i.test(form.sub), 'the help text names no single provider');
    await page.evaluate(() => document.querySelector('#lf-cancel').click());

    /* ---- two inboxes on one screen ---- */
    console.log('\n— two inboxes side by side —');
    /* Two mailboxes and a message in each, so the panes have something to
       distinguish. Linking for real needs a mail server; the UI under test is
       what happens once two accounts exist. */
    await page.evaluate(() => {
      const a = addLinked('mail', 'work@example.com', { host: 'imap.example.com' });
      const b = addLinked('mail', 'personal@example.net', { host: 'imap.example.net' });
      S.messages.push(
        { id: 'ta1', ch: 'email', prov: 'imap', acct: a.id, cid: null, fromName: 'Client', fromAddr: 'client@x.com',
          subj: 'Signed contract', prev: 'attached', body: 'attached', ts: Date.now(), unread: true, starred: false, atts: [] },
        { id: 'tb1', ch: 'email', prov: 'imap', acct: b.id, cid: null, fromName: 'Sister', fromAddr: 'sis@y.com',
          subj: 'Holiday photos', prev: 'hi', body: 'hi', ts: Date.now() - 1000, unread: false, starred: false, atts: [] });
      save(); renderMailFilters(); renderMail();
    });

    check(await page.isHidden('#split-pane'), 'the second inbox is not there until asked for');
    await page.click('#split-on');
    check(await page.isVisible('#split-pane'), 'the button opens it');
    check(await page.isHidden('#mail-detail'), 'and the reading pane gives way rather than a third column');

    const panes = await page.evaluate(() => ({
      choices: [...document.querySelectorAll('#split-acct option')].map(o => o.textContent),
      picked: document.querySelector('#split-acct').value,
      left: document.querySelectorAll('#mail-scroll .mail-row').length,
      right: document.querySelectorAll('#split-scroll .mail-row').length,
      draggable: document.querySelector('#mail-scroll .mail-row')?.getAttribute('draggable'),
    }));
    check(panes.choices.length === 3, `the second pane can show: ${panes.choices.join(' / ')}`);
    check(!!panes.picked, 'and defaults to an account rather than repeating the first pane');
    check(panes.left > 0 && panes.right > 0, `both panes have messages: ${panes.left} | ${panes.right}`);
    check(panes.draggable === 'true', 'rows become draggable in this mode');

    /* Dropping a message on the other inbox opens a forward from that account —
       it must never send by itself. */
    const moved = await page.evaluate(() => {
      const b = S.linked.find(l => l.label === 'personal@example.net');
      transferMessage('ta1', b.id);
      return {
        composeOpen: document.querySelector('#compose-ov').classList.contains('open'),
        from: document.querySelector('#cmp-from').selectedOptions[0]?.textContent || '',
        subj: document.querySelector('#cmp-subj').value,
        to: document.querySelector('#cmp-to').value,
        body: document.querySelector('#cmp-body').value,
      };
    });
    check(moved.composeOpen, 'the transfer opens the composer');
    check(/personal@example\.net/.test(moved.from), `sending from the inbox it was dropped on: ${moved.from}`);
    check(/^Fwd: Signed contract$/.test(moved.subj), `pre-filled as a forward: "${moved.subj}"`);
    check(/Forwarded/.test(moved.body) && /Client/.test(moved.body), 'with the original quoted beneath');
    check(moved.to === '', 'and no recipient assumed — a drag must not send mail on its own');

    await page.evaluate(() => { document.querySelector('#cmp-cancel')?.click(); closeCompose(); setSplit(false); });
    check(await page.isHidden('#split-pane'), 'closing returns to one inbox');
    await page.evaluate(() => {
      S.messages = S.messages.filter(m => !['ta1', 'tb1'].includes(m.id));
      S.linked = []; save(); renderMailFilters(); renderMail();
    });

    /* ---- the Bridge keeps BOTH files ---- */
    console.log('\n— Format Bridge: conversion keeps the original —');
    /* Files moved behind "More"; reveal it the way a person would. */
    await page.evaluate(() => { document.getElementById('nav-more')?.removeAttribute('hidden'); go('docs'); });
    await page.setInputFiles('#br-file', CSV);
    await page.waitForSelector('#br-loaded', { state: 'visible', timeout: 15000 });
    const dl = page.waitForEvent('download', { timeout: 30000 }).catch(() => null);
    await page.click('#br-save');
    await page.waitForFunction(() => S.documents.length >= 2, null, { timeout: 45000 });

    const pair = await page.evaluate(() => {
      const src = S.documents.find(d => d.role === 'source');
      const con = S.documents.find(d => d.role === 'converted');
      return {
        total: S.documents.length, srcName: src?.name, conName: con?.name,
        linked: !!(src && con && src.pair === con.pair && con.from === src.id),
        srcHasText: !!src?.content,
      };
    });
    check(pair.total === 2, `two documents, not one: ${pair.total}`);
    check(pair.linked, `original kept and linked: ${pair.srcName} -> ${pair.conName}`);
    check(pair.srcHasText, 'original carries its extracted text into the workspace');
    check(!!(await dl), 'converted file downloaded');

    /* ---- bytes are real and in IndexedDB, not in the synced workspace ---- */
    console.log('\n— stored bytes —');
    const vault = await page.evaluate(async () => {
      const ids = S.documents.filter(d => d.hasFile).map(d => d.id);
      const blobs = [];
      for (const id of ids) { const b = await fvGet(id); blobs.push(b instanceof Blob && b.size > 0); }
      return { count: blobs.length, allReal: blobs.every(Boolean), wsBytes: JSON.stringify(S).length };
    });
    check(vault.count === 2 && vault.allReal, `${vault.count} real blobs in the file vault`);
    check(vault.wsBytes < 200000, `workspace JSON stayed small: ${vault.wsBytes.toLocaleString()} bytes (file bytes are not in it)`);

    /* ---- the produced .xlsx is genuinely readable, on a non-vulnerable build ---- */
    console.log('\n— the .xlsx we wrote reads back correctly —');
    const rt = await page.evaluate(async () => {
      const con = S.documents.find(d => d.role === 'converted');
      const X = await brLib('xlsx', 'XLSX');
      const wb = X.read(await (await fvGet(con.id)).arrayBuffer(), { type: 'array' });
      return { version: X.version, rows: X.utils.sheet_to_json(wb.Sheets[wb.SheetNames[0]], { header: 1 }) };
    });
    check(rt.rows.length === 4, `round-tripped ${rt.rows.length} rows`);
    check(JSON.stringify(rt.rows[0]) === JSON.stringify(['Region', 'Q1', 'Q2']), `header intact: ${JSON.stringify(rt.rows[0])}`);
    /* npm's xlsx is frozen at 0.18.5, vulnerable to prototype pollution when
       reading a crafted file (CVE-2023-30533). The Bridge reads user files. */
    check(!rt.version.startsWith('0.18'), `SheetJS ${rt.version} — not the vulnerable 0.18.5 npm build`);

    /* ---- in-house: precached, offline-capable, no third-party CDN ---- */
    console.log('\n— in-house engines —');
    const cached = await page.evaluate(async () => {
      const keys = await caches.keys();
      const reqs = await (await caches.open(keys[0])).keys();
      return { version: keys[0], urls: reqs.map(r => new URL(r.url).pathname) };
    });
    for (const e of ENGINES) check(cached.urls.includes(e), `precached ${e}`);

    await ctx.setOffline(true);
    const offline = await page.evaluate(async () => {
      try {
        const X = await brLib('xlsx', 'XLSX');
        const wb = X.utils.book_new();
        X.utils.book_append_sheet(wb, X.utils.aoa_to_sheet([['a', 'b'], [1, 2]]), 'S1');
        /* type:'array' yields an ArrayBuffer (byteLength), not a typed array. */
        const out = X.write(wb, { type: 'array', bookType: 'xlsx' });
        return { bytes: out.byteLength ?? out.length };
      } catch (e) { return { error: e.message }; }
    });
    await ctx.setOffline(false);
    check(!offline.error && offline.bytes > 0,
      offline.error ? `offline conversion failed: ${offline.error}` : `built a real .xlsx with the network off (${offline.bytes} bytes)`);

    const third = [...hosts].filter(h => !h.startsWith('127.0.0.1') && !h.startsWith('localhost'));
    check(!third.some(h => h.includes('jsdelivr') || h.includes('unpkg') || h.includes('cdn')),
      third.length ? `no script CDN contacted (saw only: ${third.join(', ')})` : 'no third-party host contacted at all');

    check(errs.length === 0, errs.length ? `page errors: ${errs.join(' | ')}` : 'no page errors throughout');
  } finally {
    await browser.close();
    await s.stop();
  }
}
