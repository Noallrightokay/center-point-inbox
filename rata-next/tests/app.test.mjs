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
    await page.goto(s.url + '/auth.html?mode=signup');
    check(!/cloud accounts active/i.test(await page.textContent('#mode-badge')),
      `mode badge: "${(await page.textContent('#mode-badge')).trim()}"`);

    await page.fill('#s-email', 'owner@example.com');
    await page.fill('#s-pw', 'S3cure-pass-2026');
    await page.fill('#s-pw2', 'S3cure-pass-2026');
    await page.click('#s-next');
    await page.waitForSelector('#s-name', { state: 'visible', timeout: 10000 });
    await page.fill('#s-name', 'Owner');
    await page.click('#s-next');
    await page.waitForSelector('#s-next', { timeout: 10000 });
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

    if (await page.$('#welcome-ov.open')) {
      await page.click('#w-enter');
      await page.waitForSelector('#welcome-ov.open', { state: 'detached', timeout: 8000 }).catch(() => {});
    }

    /* ---- the Bridge keeps BOTH files ---- */
    console.log('\n— Format Bridge: conversion keeps the original —');
    await page.evaluate(() => go('docs'));
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
