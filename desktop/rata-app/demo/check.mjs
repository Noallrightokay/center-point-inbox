// Checks the demo keeps its rules: it builds both forms (a folder, served
// over http, and the one file opened from disk) and drives each in
// Chromium. Nothing it does reaches a server, it leaves other pages'
// storage alone, every address in it is invented, and every control the
// interface offers lands back in the demo. It also checks build.sh never
// deletes a folder that is not an earlier demo build.
//
// Run from the repository root, after `npm ci` in rata-next (Playwright):
//   node desktop/rata-app/demo/check.mjs
// Exit status is non-zero when any check fails.
import pw from '../../../rata-next/node_modules/playwright/index.js';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const { chromium } = pw;
const here = path.dirname(fileURLToPath(import.meta.url));
let fails = 0;
const check = (c, m) => { console.log(`${c ? '  PASS' : '  FAIL'}  ${m}`); if (!c) fails++; };
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'rata-demo-check-'));
process.on('exit', () => fs.rmSync(scratch, { recursive: true, force: true }));
const build = (...args) => spawnSync(path.join(here, 'build.sh'), args, { encoding: 'utf8' });

console.log('\n— every address in the sample is invented (RFC 2606 names) —');
{
  const src = fs.readFileSync(path.join(here, 'demo-backend.js'), 'utf8');
  /* Names no one can own, and the few real hosts the demo means: the
     provider a hospital's Microsoft 365 mailbox is read from, the release
     page, and mailrata.org, which it answers for itself (never asks). */
  const reserved = (d) => /(^|\.)(example|test|invalid|localhost)$/i.test(d) || /(^|\.)example\.(com|net|org)$/i.test(d);
  const real = new Set(['outlook.office365.com', 'github.com', 'mailrata.org']);
  const addrs = [...src.matchAll(/[\w.%+-]+@([\w-]+(?:\.[\w-]+)+)/g)].map((m) => m[0]);
  const badAddrs = addrs.filter((a) => !reserved(a.split('@')[1]));
  check(addrs.length > 20 && !badAddrs.length, `${addrs.length} addresses, none at a domain someone could own: ${JSON.stringify([...new Set(badAddrs)])}`);
  const hosts = [...src.matchAll(/\b(?:[a-z0-9-]+\.)+(?:com|org|net|edu|gov|io|co|uk|us|info|biz)\b/gi)].map((m) => m[0].toLowerCase());
  const badHosts = hosts.filter((h) => !real.has(h) && !reserved(h));
  check(!badHosts.length, `no host or domain outside the reserved names but ${[...real].join(', ')}: ${JSON.stringify([...new Set(badHosts)])}`);
  const boxes = [...src.matchAll(/host:\s*'([^']+)'/g)].map((m) => m[1]);
  check(boxes.length >= 5 && boxes.every((h) => real.has(h) || reserved(h)), `the mailboxes' servers are invented too: ${JSON.stringify(boxes)}`);
}

console.log('\n— build.sh deletes only an earlier demo build —');
{
  const stranger = path.join(scratch, 'someone');
  fs.mkdirSync(stranger);
  fs.writeFileSync(path.join(stranger, 'keep.txt'), 'mine');
  let r = build(stranger);
  check(r.status !== 0 && fs.existsSync(path.join(stranger, 'keep.txt')) && /not a demo build/.test(r.stderr),
    `a folder holding something else is refused and kept: status ${r.status}, ${JSON.stringify(r.stderr.trim())}`);
  const file = path.join(scratch, 'a-file');
  fs.writeFileSync(file, 'mine');
  r = build(file);
  check(r.status !== 0 && fs.readFileSync(file, 'utf8') === 'mine', `a file is refused and kept: status ${r.status}`);
  const empty = path.join(scratch, 'empty');
  fs.mkdirSync(empty);
  r = build(empty);
  check(r.status === 0 && fs.existsSync(path.join(empty, 'rata-demo.html')), `an empty folder is built into: status ${r.status} ${r.stderr.trim()}`);
  /* A file an older build wrote (a library since upgraded) and the one a
     Mac's Finder leaves: the build is still only the demo's. */
  fs.writeFileSync(path.join(empty, 'vendor', 'old-library-1.0.js'), 'old');
  fs.writeFileSync(path.join(empty, '.DS_Store'), '');
  r = build(empty);
  check(r.status === 0 && fs.existsSync(path.join(empty, 'rata-demo.html')) && !fs.existsSync(path.join(empty, 'vendor', 'old-library-1.0.js')),
    `an earlier build is built again, afresh: status ${r.status} ${r.stderr.trim()}`);
  /* A build copied into a folder of other pages, as the README says to
     publish it: holding rata-demo.html does not make that folder the demo's. */
  fs.writeFileSync(path.join(empty, 'index.html'), 'my site');
  fs.mkdirSync(path.join(empty, 'blog'));
  fs.writeFileSync(path.join(empty, 'blog', 'post.html'), 'my post');
  r = build(empty);
  check(r.status !== 0 && /not a demo build \(it holds blog, index\.html\)/.test(r.stderr) && fs.readFileSync(path.join(empty, 'index.html'), 'utf8') === 'my site'
    && fs.readFileSync(path.join(empty, 'blog', 'post.html'), 'utf8') === 'my post' && fs.existsSync(path.join(empty, 'rata-demo.html')),
    `a demo build that also holds other pages is refused, and all of it kept: status ${r.status}, ${JSON.stringify(r.stderr.trim())}`);
  /* A demo name of the wrong kind is not the demo's either. */
  const odd = path.join(scratch, 'odd');
  fs.mkdirSync(path.join(odd, 'rata-demo.html'), { recursive: true });
  fs.writeFileSync(path.join(odd, 'rata-demo.html', 'notes.txt'), 'mine');
  fs.writeFileSync(path.join(odd, 'RATA-demo.html'), 'the one file someone was sent');
  r = build(odd);
  check(r.status !== 0 && fs.existsSync(path.join(odd, 'rata-demo.html', 'notes.txt')) && fs.existsSync(path.join(odd, 'RATA-demo.html')) && /RATA-demo\.html, rata-demo\.html|rata-demo\.html, RATA-demo\.html/.test(r.stderr),
    `a folder named rata-demo.html, or RATA-demo.html outside the demo’s own out/, is refused and kept: status ${r.status}, ${JSON.stringify(r.stderr.trim())}`);
  const fresh = path.join(scratch, 'new', 'demo');
  r = build(fresh);
  check(r.status === 0 && fs.existsSync(path.join(fresh, 'rata-demo.html')), `a folder that is not there yet is made: status ${r.status} ${r.stderr.trim()}`);
  /* The demo's own out/, as the README uses it: the one file first, then
     the folder over it. Only when there is no out/ yet, so a contributor's
     own build is never touched. */
  const own = path.join(here, 'out');
  if (fs.existsSync(own)) console.log('  SKIP  the demo’s own out/ is there already; remove it to check it');
  else {
    const one = build('--single');
    r = build();
    check(one.status === 0 && r.status === 0 && fs.existsSync(path.join(own, 'rata-demo.html')),
      `its own out/ is built into after --single wrote there: status ${one.status}, ${r.status} ${r.stderr.trim()}`);
    fs.writeFileSync(path.join(own, 'notes.txt'), 'mine');
    r = build();
    check(r.status !== 0 && fs.readFileSync(path.join(own, 'notes.txt'), 'utf8') === 'mine',
      `and refused, keeping it, once it holds something else: status ${r.status}, ${JSON.stringify(r.stderr.trim())}`);
    fs.rmSync(own, { recursive: true, force: true });
  }
}

/* The two forms, built afresh for the rest. */
const folder = path.join(scratch, 'folder');
let r = build(folder);
check(r.status === 0, `the folder build works: ${r.stderr.trim()}`);
const single = path.join(scratch, 'file', 'RATA-demo.html');
r = build('--single', single);
check(r.status === 0 && fs.existsSync(single), `the one-file build works: ${r.stderr.trim()}`);

console.log('\n— the page sends nobody to a page the demo does not ship —');
{
  const page = fs.readFileSync(path.join(folder, 'rata-demo.html'), 'utf8');
  const one = fs.readFileSync(single, 'utf8');
  /* Only the two that cannot run in the demo are left: no sign-in at
     start (demo-backend.js signs in first) and an online account's. */
  const KEPT = ["if(!SESSION){location.replace('auth.html')}", "if(!session){localStorage.removeItem('centra_session');location.replace('auth.html');return}"];
  const ways = (t) => (t.match(/(?<![\w\s])auth\.html/g) || []).length;
  check([page, one].every((t) => ways(t) === 2 && KEPT.every((k) => t.includes(k))), `neither build sends a viewer to auth.html, which it does not ship: ${ways(page)} and ${ways(one)} places name it`);
}

/* The folder build over http, with one other page on the same origin
   (what a second local page or tool on that port would be). */
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.woff2': 'font/woff2', '.png': 'image/png', '.json': 'application/json' };
const STRANGER = '<!doctype html><meta charset="utf-8"><title>Another page</title><p>Another page on this computer.</p>';
const server = http.createServer((req, res) => {
  const p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  if (p === '/stranger.html') { res.writeHead(200, { 'content-type': 'text/html' }); return res.end(STRANGER); }
  const f = path.join(folder, path.normalize(p).replace(/^([/\\])+/, ''));
  if (!f.startsWith(folder) || !fs.existsSync(f) || !fs.statSync(f).isFile()) { res.writeHead(404); return res.end('not found'); }
  res.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream' });
  fs.createReadStream(f).pipe(res);
});
await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
const base = `http://127.0.0.1:${server.address().port}`;
fs.writeFileSync(path.join(path.dirname(single), 'stranger.html'), STRANGER);

const FORMS = [
  { name: 'the folder, served', demo: base + '/rata-demo.html', other: base + '/stranger.html' },
  { name: 'the one file, opened from disk', demo: pathToFileURL(single).href, other: pathToFileURL(path.join(path.dirname(single), 'stranger.html')).href },
];

const browser = await chromium.launch();
for (const form of FORMS) {
  console.log(`\n— ${form.name} —`);
  const ctx = await browser.newContext();
  /* Everything that would leave this computer is stopped and written down. */
  const outside = [];
  const off = (u) => /^(https?|wss?):/i.test(String(u)) && !String(u).startsWith(base + '/');
  await ctx.route(off, (route) => { outside.push(route.request().method() + ' ' + route.request().url()); return route.abort('blockedbyclient'); });
  ctx.on('request', (q) => { if (off(q.url()) && !outside.includes(q.method() + ' ' + q.url())) outside.push(q.method() + ' ' + q.url()); });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('dialog', (d) => d.accept());

  /* What another page on this origin keeps. Chromium gives every page
     opened from disk one store, so for the one file this is any other
     HTML file on the computer. */
  await page.goto(form.other);
  await page.evaluate(() => { localStorage.setItem('notes', 'another page’s notes'); localStorage.setItem('rata', 'not the demo’s'); sessionStorage.setItem('draft', 'kept'); });
  const theirs = () => page.evaluate(() => [localStorage.getItem('notes'), localStorage.getItem('rata'), sessionStorage.getItem('draft')].join('|'));
  const THEIRS = 'another page’s notes|not the demo’s|kept';

  const ready = async () => {
    await page.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function' && S.messages.length > 0, null, { timeout: 20000 });
    await page.waitForFunction(() => lastSync > 0 && !SYNCING, null, { timeout: 10000 }).catch(() => {});
  };
  const inbox = () => page.evaluate(() => ({ url: location.href, mail: S.messages.filter((m) => inInbox(m)).length, boxes: S.linked.filter((l) => l.type === 'mail').length,
    rows: document.querySelectorAll('#mail-list [role=option], .vl-win [role=option]').length }));
  await page.goto(form.demo);
  await ready();
  check((await theirs()) === THEIRS, `opening the demo leaves another page’s storage alone: ${await theirs()}`);

  /* AI, answered in the page. */
  /* Opening fetches the whole message first (open_message), which draws
     the pane again; a button pressed before that would lose its answer. */
  const open = async (subject) => {
    const id = await page.evaluate((s) => { const m = S.messages.find((x) => x.subj === s); go('inbox'); openMail(m.id); return m.id; }, subject);
    await page.waitForFunction((id) => OPENED.has(id), id, { timeout: 5000 }).catch(() => {});
    await page.waitForTimeout(300);
    return id;
  };
  await open('Action needed: reappointment packet due 31 October');
  await page.click('#md-sum');
  await page.waitForFunction(() => { const t = document.querySelector('#md-summary .md-ai-text'); return t && t.textContent && t.textContent !== 'Summarizing…'; }, null, { timeout: 10000 }).catch(() => {});
  const sum = await page.evaluate(() => ({ title: $('#md-summary .md-ai-title').textContent, text: $('#md-summary .md-ai-text').textContent }));
  check(sum.title === 'Summary by AI' && /^Demo summary/.test(sum.text) && /31 October/.test(sum.text), `Summarize answers with the demo’s own summary: ${JSON.stringify(sum)}`);
  const id = await open('Shift swap on the 14th?');
  await page.click('#md-translate');
  await page.waitForFunction((id) => TRANSLATED.has(id), id, { timeout: 10000 }).catch(() => {});
  const tr = await page.evaluate((id) => ({ t: (TRANSLATED.get(id) || {}).text || '', pane: $('#mail-detail').textContent, toast: $('#toast') && $('#toast').textContent }), id);
  check(/^Demo translation/.test(tr.t) && tr.pane.includes('Demo translation') && /take my day shift/.test(tr.t), `Translate answers with the demo’s own translation: ${JSON.stringify(tr.t.slice(0, 160))} ${tr.toast || ''}`);
  await page.evaluate(() => openAssist());
  await page.click('#as-run');
  await page.waitForFunction(() => !$('#as-run').disabled, null, { timeout: 10000 }).catch(() => {});
  const brief = await page.evaluate(() => ({ out: $('#as-out').textContent, flags: S.messages.filter((m) => m.flag).map((m) => m.subj + ': ' + m.flag.task) }));
  check(brief.flags.length >= 3 && brief.flags.every((f) => /: Demo: /.test(f)) && brief.flags.some((f) => /^Action needed: reappointment/.test(f)) && /need/.test(brief.out),
    `Run briefing flags sample mail with the demo’s own tasks: ${JSON.stringify(brief.flags)}`);
  await page.evaluate(() => closeAssist());
  /* A renewal, as bridge.js asks for one (Try again in the licence box). */
  const renewed = await page.evaluate(async () => {
    const r = await fetch('https://mailrata.org/api/licence/renew', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ licence: 'demo' }) });
    return r.json();
  }).catch((e) => ({ error: String(e) }));
  check(renewed.licensed === true && renewed.licence === 'demo', `a licence renewal is answered in the page: ${JSON.stringify(renewed)}`);
  const elsewhere = await page.evaluate(async () => { try { await fetch('https://mailrata.org/api/health'); return 'answered'; } catch (e) { return 'refused'; } });
  check(elsewhere === 'refused', `anything else at mailrata.org is refused in the page: ${elsewhere}`);
  check(!outside.length, `nothing was asked of any server: ${JSON.stringify(outside)}`);

  /* Sign out, and Delete account, land back in the demo. */
  await page.evaluate(() => document.getElementById('btn-signout').click());
  await page.waitForTimeout(800);
  await ready().catch(() => {});
  let at = await inbox().catch((e) => ({ url: page.url(), error: String(e).slice(0, 120) }));
  check(at.url === form.demo && at.mail > 0, `Sign out comes back to the demo’s inbox: ${JSON.stringify(at)}`);
  if (at.url !== form.demo) { await page.goto(form.demo); await ready(); }
  await page.evaluate(() => { go('set'); document.getElementById('btn-delete').click(); });
  await page.waitForFunction(() => !$('#del-go').disabled, null, { timeout: 5000 }).catch(() => {});
  await page.evaluate(() => { $('#del-email').value = SESSION.email; $('#del-go').click(); });
  await page.waitForTimeout(1500);
  await ready().catch(() => {});
  at = await inbox().catch((e) => ({ url: page.url(), error: String(e).slice(0, 120) }));
  check(at.url === form.demo && at.mail > 0 && at.boxes === 5, `Delete account starts the demo again: ${JSON.stringify(at)}`);
  if (at.url !== form.demo) { await page.goto(form.demo); await ready(); }
  await page.waitForFunction(() => S.documents.length >= 3, null, { timeout: 15000 }).catch(() => {});
  check((await page.evaluate(() => S.documents.length)) >= 3, `with its sample documents in Files: ${await page.evaluate(() => S.documents.map((d) => d.name).join(', '))}`);
  check((await theirs()) === THEIRS, `and another page’s storage is still there: ${await theirs()}`);

  /* Reset and the Base/Pro switch start the demo again. */
  await page.evaluate(() => { localStorage.setItem('rata_demo_conn', '{}'); });
  await page.click('#demo-reset');
  await page.waitForTimeout(800);
  await ready();
  check((await page.evaluate(() => localStorage.getItem('rata_demo_conn'))) === null && (await theirs()) === THEIRS,
    `Reset clears the demo’s own keys and only those: ${await theirs()}`);
  await page.click('[data-plan="base"]');
  await page.waitForTimeout(800);
  await ready();
  at = await inbox();
  check(at.boxes === 2 && (await theirs()) === THEIRS, `the Base switch shows two mailboxes and leaves the rest alone: ${at.boxes} mailboxes, ${await theirs()}`);
  await open('November ED schedule is posted');
  await page.click('#md-sum');
  await page.waitForFunction(() => { const t = document.querySelector('#md-summary .md-ai-text'); return t && t.textContent && t.textContent !== 'Summarizing…'; }, null, { timeout: 10000 }).catch(() => {});
  const base2 = await page.evaluate(() => ({ title: $('#md-summary .md-ai-title').textContent, note: $('#md-summary .md-ai-note').textContent }));
  check(base2.title === 'Quick summary' && /part of RATA Pro/.test(base2.note), `on Base, Summarize says AI is Pro’s, as the relay does: ${JSON.stringify(base2)}`);
  await page.click('[data-plan="pro"]');
  await page.waitForTimeout(800);
  await ready();
  check((await inbox()).boxes === 5 && (await theirs()) === THEIRS, 'and back to Pro');
  check(!outside.length, `still nothing asked of any server: ${JSON.stringify(outside)}`);
  check(!errors.length, `no page errors: ${JSON.stringify(errors)}`);
  await ctx.close();
}
await browser.close();
server.close();
console.log(fails ? `\n${fails} FAILED` : '\nALL PASSED');
process.exit(fails ? 1 : 0);
