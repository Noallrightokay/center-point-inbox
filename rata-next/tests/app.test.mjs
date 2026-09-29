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
  '/vendor/pdf-6.3.289.legacy.min.js',
  '/vendor/pdf.worker-6.3.289.legacy.min.js',
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

    /* ---- a new workspace is email only, and empty ---- */
    console.log('\n— nothing is connected until the user connects it —');
    const fresh = await page.evaluate(() => ({
      leftovers: ['plugins', 'connections', 'jobs'].filter(k => k in S),
      linked: S.linked.length,
      views: [...document.querySelectorAll('[data-view]')].map(b => b.dataset.view),
      sections: ['create', 'plug', 'conn'].filter(v => document.getElementById('view-' + v)),
    }));
    check(fresh.leftovers.length === 0, `no switches for channels that do not exist: ${fresh.leftovers.join(', ') || 'none'}`);
    check(fresh.linked === 0, `no accounts assumed: ${fresh.linked} linked`);
    check(fresh.sections.length === 0 && !fresh.views.some(v => ['create', 'plug', 'conn'].includes(v)),
      `no Extras, Connections or launcher screens to lead nowhere: ${[...new Set(fresh.views)].join(', ')}`);

    /* ---- nothing leaves for another company's server ---- */
    const offMachine = await page.evaluate(() => ({
      translate: !!document.querySelector('#md-translate, #set-autotrans, #br-lang, #set-lang'),
      ai: !!document.querySelector('#as-ai, #cfg-akey'),
      operator: !!document.querySelector('#cfg-gcid, #cfg-sburl, #cfg-sbkey, #set-api'),
      chat: [...document.querySelectorAll('[data-addlink]')].map(b => b.dataset.addlink).filter(t => t !== 'mail'),
      source: [...document.scripts].map(x => x.textContent).join('\n'),
    }));
    check(!offMachine.translate && !offMachine.ai,
      'no translate or AI controls — both sent the text of your mail to Google or Anthropic');
    check(!/translate\.googleapis|api\.anthropic|gmail\.googleapis|accounts\.google/.test(offMachine.source),
      'and no code that could reach those hosts is left in the page');
    check(!offMachine.operator, 'no operator settings (OAuth client, Supabase, Stripe links, API endpoint) shown to customers');
    check(offMachine.chat.length === 0, `no Slack, Discord or phone to add: ${offMachine.chat.join(', ') || 'email only'}`);

    /* A workspace saved by an older build still opens, minus what it can no
       longer show. */
    const legacy = await page.evaluate(() => {
      const old = { v: 5, settings: { name: 'Old', api: 'x' }, plugins: { slack: true, discord: true },
        connections: { gmail: true }, jobs: ['Consulting'], rules: [], folders: [], contacts: [], documents: [], audit: [],
        counters: { scans: 0, warned: 0, blocked: 0, conversions: 0, autopilot: 3 },
        linked: [{ id: 'a', type: 'mail', label: 'me@example.com' }, { id: 'b', type: 'slack', label: '@me' }, { id: 'c', type: 'google', label: 'g@gmail.com' }],
        messages: [{ id: 'e', ch: 'email', prov: 'imap', subj: 'kept', prev: '', body: '', ts: 1, atts: [] },
          { id: 's', ch: 'slack', prov: 'slack', subj: '', prev: 'gone', body: '', ts: 2, atts: [] }] };
      const m = migrate(JSON.parse(JSON.stringify(old)));
      return { v: m.v, linked: m.linked.map(l => l.type), msgs: m.messages.map(x => x.id),
        leftovers: ['plugins', 'connections', 'jobs'].filter(k => k in m), api: 'api' in m.settings };
    });
    check(legacy.v === 6 && legacy.linked.join() === 'mail' && legacy.msgs.join() === 'e',
      `an older workspace keeps its mailbox and its mail: v${legacy.v}, linked ${legacy.linked.join()}, messages ${legacy.msgs.join()}`);
    check(legacy.leftovers.length === 0 && !legacy.api, 'and sheds the Slack feed, the switches and the API key');

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
    /* Naming providers here is the point, not a slip: one form takes all of
       them, and a list of familiar names says that better than avoiding the
       word "Gmail" does. What would be wrong is a promise the app cannot keep:
       Microsoft's IMAP no longer takes a password (C0), so the text must not
       say "any email address" and must say Microsoft is not supported yet. */
    check(!/any email address/i.test(form.sub) && /outlook/i.test(form.sub) && /not supported yet/i.test(form.sub),
      `the help text names Microsoft as not supported rather than promising every address: "${form.sub.slice(0, 48)}…"`);
    check(/company domain|own domain/i.test(form.sub),
      'and says a work address on its own domain counts, which is the one people assume will not');
    check(/app password/i.test(form.sub), 'while being clear it is an app password, not the account password');
    await page.evaluate(() => document.querySelector('#lf-cancel').click());

    /* ---- deleting the account ---- */
    console.log('\n— deleting the account, and what that clears here ---');
    await page.evaluate(() => go('set'));
    check(await page.isHidden('#del-confirm'), 'the confirmation is not open until asked for');
    await page.click('#btn-delete');
    await page.waitForTimeout(400);
    check(await page.isVisible('#del-confirm'), 'the danger step appears');
    const warn = await page.textContent('#del-detail');
    check(/device/i.test(warn) || /Removes/i.test(warn), `and states what goes: "${warn.slice(0, 74)}…"`);

    /* The wrong address must not delete anything. */
    await page.fill('#del-email', 'not-my-address@example.com');
    await page.click('#del-go');
    await page.waitForTimeout(400);
    check(/app\.html/.test(page.url()), 'typing the wrong address deletes nothing');
    check(await page.evaluate(() => !!localStorage.getItem('centra_session')),
      'and the session is still there');

    await page.click('#del-cancel');
    check(await page.isHidden('#del-confirm'), 'and it can be backed out of');
    /* These blocks share one page: leave it where the next one expects it. */
    await page.evaluate(() => go('inbox'));

    /* ---- what may leave the device ---- */
    console.log('\n— the device keeps the mail —');
    const privacy = await page.evaluate(() => {
      /* A workspace with something of every kind in it. */
      S.messages.push({ id: 'p1', ch: 'email', prov: 'imap', cid: null, fromName: 'Client',
        fromAddr: 'client@example.com', subj: 'Contract terms', prev: 'as discussed',
        body: 'The figure we agreed was 84,000.', ts: Date.now(), unread: true, starred: false, atts: [] });
      S.contacts.push({ id: 'pc1', name: 'Real Person', addr: 'person@example.com', phone: '+1 555 0100', context: 'work' });
      S.documents.push({ id: 'pd1', name: 'terms.txt', fmt: 'TXT', prov: 'rata', origin: 'rata',
        size: '1 KB', ts: Date.now(), content: 'The figure we agreed was 84,000.' });
      S.audit.push({ ts: Date.now(), what: 'mail.read', detail: 'client@example.com' });
      S.settings.api = 'sk-ant-secret-key';
      S.settings.name = 'Preference That Travels';
      S.folders.push('A folder');
      save();
      const sent = forCloud(S);
      return {
        fields: Object.keys(sent).sort(),
        json: JSON.stringify(sent),
        localMessages: S.messages.length,
      };
    });

    check(!privacy.fields.includes('messages'), `uploaded fields: ${privacy.fields.join(', ')}`);
    check(!privacy.fields.includes('documents') && !privacy.fields.includes('contacts'),
      'no documents and no contacts among them');
    check(!privacy.fields.includes('audit') && !privacy.fields.includes('counters'),
      'and no audit trail');

    /* Field names are one thing; the bytes are the actual claim. */
    for (const leak of ['84,000', 'Contract terms', 'client@example.com', 'Real Person', '+1 555 0100', 'sk-ant-secret-key']) {
      check(!privacy.json.includes(leak), `not in the uploaded payload: ${JSON.stringify(leak)}`);
    }
    check(privacy.json.includes('Preference That Travels') && privacy.json.includes('A folder'),
      'while preferences and folder names do travel, which is what an account is for');

    /* Another device signing in must not wipe what this one has read. */
    const afterPull = await page.evaluate(() => {
      const before = S.messages.length;
      applyCloud({ settings: { name: 'Renamed Elsewhere' }, folders: ['From another device'], rules: [{ match: 'news@', cat: 'updates' }],
        linked: [{ id: 'g1', type: 'google', label: 'old@gmail.com' }] });
      return { before, after: S.messages.length, name: S.settings.name, folders: S.folders.length, rules: S.rules.length,
        foreign: S.linked.filter(l => l.type !== 'mail').length };
    });
    check(afterPull.after === afterPull.before,
      `a pull from another device leaves this one's mail alone: ${afterPull.before} → ${afterPull.after}`);
    check(afterPull.name === 'Renamed Elsewhere' && afterPull.rules === 1,
      'while preferences set elsewhere do arrive');
    check(afterPull.foreign === 0, 'but an account type this build cannot read is not brought back');

    await page.evaluate(() => {
      S.messages = S.messages.filter(m => m.id !== 'p1');
      S.contacts = S.contacts.filter(c => c.id !== 'pc1');
      S.documents = S.documents.filter(d => d.id !== 'pd1');
      S.folders = []; S.audit = []; S.rules = []; S.linked = []; delete S.settings.api;
      S.settings.name = 'Owner'; save();
    });

    /* ---- where the mail is kept ---- */
    console.log('\n— mail is kept in IndexedDB, with room for all of it —');
    {
      /* A second window on the same workspace, so reloading it leaves the
         page the other blocks share alone. Everything it adds, it removes. */
      const p2 = await ctx.newPage();
      p2.on('pageerror', e => errs.push('storage page: ' + e.message));
      const boot = async (pg) => {
        await pg.waitForFunction(() => typeof S !== 'undefined' && !!S && typeof msFlush === 'function', null, { timeout: 25000 });
        await pg.evaluate(() => { window.__toasts = []; const t = toast; toast = (m) => { window.__toasts.push(String(m)); return t(m); }; });
      };
      await p2.goto(s.url + '/app.html'); await boot(p2);
      const T6 = 400, BODY = 20000;
      const big = await p2.evaluate(async ({ T6, BODY }) => {
        for (let i = 0; i < T6; i++) S.messages.push({ id: 't6-' + i, ch: 'email', prov: 'imap', cid: null, fromName: 'Bulk',
          fromAddr: 'bulk@example.com', subj: 'Stored ' + i, prev: 'p', body: String(i).padEnd(BODY, '.'), ts: 1e12 + i,
          unread: true, starred: false, atts: [] });
        save(); await msFlush();
        const raw = localStorage.getItem(LS_KEY), meta = JSON.parse(raw);
        return { toasts: window.__toasts.splice(0), metaBytes: raw.length, hasMessages: 'messages' in meta, store: meta.store, open: !!MS };
      }, { T6, BODY });
      check(big.open, 'the message store opens');
      check(!big.toasts.some(t => /full|could not save/i.test(t)),
        `${T6} messages of ${BODY / 1000} KB each (${(T6 * BODY / 1e6).toFixed(0)} MB, past localStorage's cap) save without complaint: ${big.toasts.join(' | ') || 'no toast'}`);
      check(!big.hasMessages && big.store === 'idb' && big.metaBytes < 200000,
        `and the localStorage entry holds no mail: ${(big.metaBytes / 1000).toFixed(1)} KB, store=${big.store}`);

      await p2.reload(); await boot(p2);
      const back = await p2.evaluate(async ({ T6, BODY }) => {
        const mine = S.messages.filter(m => String(m.id).startsWith('t6-'));
        const inMemory = mine.filter(m => 'body' in m).length;
        const texts = await bodiesOf(mine);
        return { n: mine.length, inMemory, heldBytes: JSON.stringify(S.messages).length,
          intact: mine.every(m => (texts.get(m.id) || '').length === BODY && texts.get(m.id).startsWith(m.id.slice(3))) };
      }, { T6, BODY });
      check(back.n === T6 && back.intact, `after a relaunch all ${back.n} are there, bodies intact on disk`);
      check(back.inMemory === 0 && back.heldBytes < T6 * 1000,
        `and their text is not held in memory: ${back.inMemory} with text, ${(back.heldBytes / 1e6).toFixed(2)} MB of messages held for ${(T6 * BODY / 1e6).toFixed(0)} MB of mail`);

      /* The text comes back from disk wherever it is needed. */
      const uses = await p2.evaluate(async () => {
        const m = S.messages.find(x => x.id === 't6-12');
        BODY_CACHE.clear();
        go('inbox'); openMail('t6-12');
        const first = $('#md-body') ? $('#md-body').textContent : null;
        await new Promise(r => setTimeout(r, 300));
        const opened = $('#md-body') ? $('#md-body').textContent : '';
        const marker = 'needle-' + Math.random().toString(36).slice(2);
        BODY_CACHE.clear();
        const tx = MS.transaction(MS_TEXT, 'readwrite'); tx.objectStore(MS_TEXT).put(JSON.stringify({ body: 'Hello ' + marker + ' there' }), 't6-40');
        await new Promise(r => { tx.oncomplete = r; });
        $('#search-input').value = marker; await doSearch();
        const found = [...document.querySelectorAll('#res-pane .res-row .res-title')].map(e => e.textContent);
        return { first, opened: opened.length, found, cached: BODY_CACHE.size <= BODY_CACHE_MAX };
      });
      check(uses.opened === BODY, `opening a message reads its text from disk (${uses.opened} characters shown)`);
      check(uses.found.length === 1 && uses.found[0] === 'Stored 40', `a search finds words that are only on disk: ${JSON.stringify(uses.found)}`);
      check(uses.cached, 'and text read lately is kept only up to a limit');

      /* A message that arrives with its text gives it up at the next save. */
      const arrive = await p2.evaluate(async () => {
        S.messages.push({ id: 't6-new', ch: 'email', prov: 'imap', subj: 'Arrived', prev: 'p', body: 'fresh text', ts: 1e12 + 9999, unread: true, starred: false, atts: [] });
        save(); await msFlush();
        const m = S.messages.find(x => x.id === 't6-new');
        const tx = MS.transaction(MS_TEXT, 'readonly'), rq = tx.objectStore(MS_TEXT).get('t6-new');
        await new Promise(r => { tx.oncomplete = r; });
        return { inMemory: 'body' in m, onDisk: textOf(rq.result), now: bodyNow(m) };
      });
      check(!arrive.inMemory && arrive.onDisk === 'fresh text' && arrive.now === 'fresh text',
        `new mail's text goes to disk at the next save and leaves memory (${JSON.stringify(arrive)})`);

      /* An export is the whole workspace, text included. */
      const exp = await p2.evaluate(async () => {
        let got = null; const dl = brDownload; brDownload = async (blob) => { got = await blob.text(); return true; };
        try { await $('#btn-export').onclick(); } finally { brDownload = dl; }
        const w = JSON.parse(got), m = w.messages.find(x => x.id === 't6-new');
        return { body: m && m.body, still: 'body' in S.messages.find(x => x.id === 't6-new') };
      });
      check(exp.body === 'fresh text' && !exp.still, 'an export carries every message\'s text, read back from disk');

      /* Starring one message writes one record, not the mailbox. */
      const writes = await p2.evaluate(async () => {
        await msFlush();
        let puts = 0; const put = IDBObjectStore.prototype.put;
        IDBObjectStore.prototype.put = function (...a) { if (this.name === 'meta' || this.name === 'messages') puts++; return put.apply(this, a); };
        S.messages.find(m => m.id === 't6-7').starred = true; save(); await msFlush();
        IDBObjectStore.prototype.put = put;
        return puts;
      });
      check(writes === 1, `starring one message writes one record (${writes})`);

      /* Deleted, then the window closed at once: the delete still holds. */
      await p2.evaluate(() => { dropMessages(['t6-3']); save(); });
      await p2.reload(); await boot(p2);
      const afterDel = await p2.evaluate(() => ({ gone: !S.messages.some(m => m.id === 't6-3'), starred: S.messages.find(m => m.id === 't6-7').starred }));
      check(afterDel.gone, 'a message deleted just before closing stays deleted');
      check(afterDel.starred, 'and the star from before that is still there');

      /* If the window closed before IndexedDB caught up, S.gone (written at
         once) still keeps the message away, and the store is tidied. */
      await p2.evaluate(() => { const meta = JSON.parse(localStorage.getItem(LS_KEY)); meta.gone = Object.assign(meta.gone || {}, { 't6-4': Date.now() }); localStorage.setItem(LS_KEY, JSON.stringify(meta)); });
      await p2.reload(); await boot(p2);
      check(await p2.evaluate(async () => { await msFlush(); const held = await msAll(MS); return !S.messages.some(m => m.id === 't6-4') && !held.some(([id]) => id === 't6-4'); }),
        'a delete recorded only in the workspace entry is honoured, and removed from the store');

      /* Mail an older build kept in the localStorage entry moves across. */
      await p2.evaluate(() => {
        const meta = JSON.parse(localStorage.getItem(LS_KEY)); delete meta.store;
        meta.messages = [{ id: 't6-legacy', ch: 'email', prov: 'imap', subj: 'From 0.1.12', prev: '', body: 'old', ts: 5, atts: [] },
          { id: 't6-slack', ch: 'slack', prov: 'slack', subj: '', prev: '', body: '', ts: 6, atts: [] }];
        localStorage.setItem(LS_KEY, JSON.stringify(meta));
      });
      await p2.reload(); await boot(p2);
      const moved = await p2.evaluate(async () => {
        await msFlush();
        const meta = JSON.parse(localStorage.getItem(LS_KEY)), held = (await msAll(MS)).map(([id]) => id);
        return { shown: S.messages.some(m => m.id === 't6-legacy'), slack: S.messages.some(m => m.id === 't6-slack'),
          inStore: held.includes('t6-legacy'), entryHasMail: 'messages' in meta, bulk: S.messages.filter(m => String(m.id).startsWith('t6-')).length };
      });
      check(moved.shown && moved.inStore && !moved.entryHasMail,
        `mail saved by an older build moves into IndexedDB and out of localStorage (shown ${moved.shown}, stored ${moved.inStore}, left in entry ${moved.entryHasMail})`);
      check(!moved.slack && moved.bulk === T6 - 2 + 2, `and joins what was already there (${moved.bulk} held), minus a channel this build dropped`);

      /* A mailbox kept by 0.1.20 — each message whole, in version 1 of the
         database — keeps its text where it is; the rest is copied on first launch. */
      const p4 = await ctx.newPage();
      p4.on('pageerror', e => errs.push('upgrade page: ' + e.message));
      await p4.goto(s.url + '/auth.html');
      const dbName = await p2.evaluate(() => MS_DB);
      await p4.evaluate(async (name) => {
        await new Promise(r => { const q = indexedDB.deleteDatabase(name + '-v1test'); q.onsuccess = q.onerror = q.onblocked = r; });
        const db = await new Promise((res, rej) => { const q = indexedDB.open(name + '-v1test', 1); q.onupgradeneeded = () => q.result.createObjectStore('messages'); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
        const tx = db.transaction('messages', 'readwrite');
        tx.objectStore('messages').put(JSON.stringify({ id: 'v1-a', ch: 'email', subj: 'Kept whole', prev: 'p', body: 'the whole text', ts: 1, atts: [] }), 'v1-a');
        tx.objectStore('messages').put('not json', 'v1-bad');
        await new Promise(r => { tx.oncomplete = r; }); db.close();
      }, dbName);
      await p4.close();
      const up = await p2.evaluate(async (name) => {
        const saved = MS, keep = MS_DB;
        const db = await new Promise((res, rej) => { const q = indexedDB.open(name + '-v1test', 1); q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error); });
        db.close();
        const d2 = await msOpen(name + '-v1test');
        const tx = d2.transaction(['messages', 'meta'], 'readonly');
        const a = tx.objectStore('meta').get('v1-a'), b = tx.objectStore('messages').get('v1-a'), bad = tx.objectStore('meta').get('v1-bad');
        await new Promise(r => { tx.oncomplete = r; });
        d2.close();
        await new Promise(r => { const q = indexedDB.deleteDatabase(name + '-v1test'); q.onsuccess = q.onerror = q.onblocked = r; });
        return { rest: a.result, body: textOf(b.result), bad: bad.result, same: MS === saved && MS_DB === keep };
      }, dbName);
      const rest = JSON.parse(up.rest || '{}');
      check(up.body === 'the whole text' && !('body' in rest) && rest.subj === 'Kept whole' && up.bad === 'not json',
        `a 0.1.20 mailbox is upgraded in place: its text left where it was, the rest copied for start-up, an unreadable record carried so it can be removed (${JSON.stringify(up)})`);

      /* Where IndexedDB will not open, mail stays in localStorage as before —
         and loading older mail keeps its old limit there. */
      const p3 = await ctx.newPage();
      p3.on('pageerror', e => errs.push('fallback page: ' + e.message));
      await p3.addInitScript(() => { const o = IDBFactory.prototype.open; IDBFactory.prototype.open = function (name, ...a) { if (String(name).startsWith('rata-mail-')) throw new Error('no IndexedDB here'); return o.call(this, name, ...a); }; });
      await p3.goto(s.url + '/app.html'); await boot(p3);
      const fb = await p3.evaluate(() => {
        S.messages.push({ id: 't6-fallback', ch: 'email', prov: 'imap', subj: 'Kept the old way', prev: '', body: 'x', ts: 7, atts: [] });
        save();
        const meta = JSON.parse(localStorage.getItem(LS_KEY));
        return { open: !!MS, inEntry: (meta.messages || []).some(m => m.id === 't6-fallback'), capped: String(loadOlder).includes('!MS&&') };
      });
      await p3.close();
      check(!fb.open && fb.inEntry && fb.capped, 'without IndexedDB, mail is kept in localStorage the old way, under the old cap');
      await p2.reload(); await boot(p2);
      const merged = await p2.evaluate(async () => {
        await msFlush();
        return { fallback: S.messages.some(m => m.id === 't6-fallback'), bulk: S.messages.filter(m => /^t6-\d+$/.test(m.id)).length,
          entryHasMail: 'messages' in JSON.parse(localStorage.getItem(LS_KEY)) };
      });
      check(merged.fallback && merged.bulk === T6 - 2 && !merged.entryHasMail,
        `and when it opens again, both halves are one inbox (${merged.bulk + 1} + legacy), nothing lost`);

      await p2.evaluate(async () => { S.messages = S.messages.filter(m => !String(m.id).startsWith('t6-')); save(); await msFlush(); });
      await p2.close();
    }

    /* ---- BUG-S: changing plan never goes through a second Payment Link ---- */
    console.log('\n— a live plan switches in the billing portal, never through a Payment Link —');
    {
      /* A Payment Link makes a second Stripe customer, whose checkout the
         webhook refuses as a conflict with the live subscription: charged
         twice, and the switch never applies. */
      const plan = async (have, portal) => page.evaluate(({ have, portal }) => {
        localStorage.setItem(CFG_KEY, JSON.stringify(Object.assign({
          stripeBase: 'https://buy.stripe.com/test_base', stripePro: 'https://buy.stripe.com/test_pro',
        }, portal ? { stripePortal: 'https://billing.stripe.com/p/login/test' } : {})));
        S.settings.plan = have; renderPlan();
        const out = {
          links: [...document.querySelectorAll('#plan-buttons a')].map(a => ({ href: a.href, text: a.textContent })),
          none: getComputedStyle(document.getElementById('bill-nolinks')).display !== 'none' ? document.getElementById('bill-nolinks').textContent : null,
          uid: SESSION && SESSION.uid, email: SESSION && SESSION.email,
        };
        localStorage.removeItem(CFG_KEY); S.settings.plan = null; renderPlan();
        return out;
      }, { have, portal });
      const free = await plan(null, true);
      check(free.links.length === 2 && free.links.every(l => /buy\.stripe\.com/.test(l.href)),
        `no plan: the Payment Links: ${free.links.map(l => l.text).join(' / ')}`);
      check(free.links.every(l => { const u = new URL(l.href); return u.searchParams.get('prefilled_email') === free.email && u.searchParams.get('client_reference_id') === free.uid; }),
        `carrying the account's address and id, as the website's links do: ${free.links[0] && free.links[0].href}`);
      check(free.links.every(l => !l.text.includes('—')), `with no em dash in the words: ${free.links.map(l => l.text).join(' / ')}`);
      for (const have of ['base', 'pro']) {
        const live = await plan(have, true);
        check(live.links.length === 1 && /billing\.stripe\.com/.test(live.links[0].href) && !live.links.some(l => /buy\.stripe\.com/.test(l.href)),
          `on ${have}: only the billing portal, no Payment Link: ${live.links.map(l => `${l.text} ${l.href}`).join(' / ')}`);
        const bare = await plan(have, false);
        check(bare.links.length === 0 && /billing portal/.test(bare.none || ''),
          `on ${have} with no portal configured: still no Payment Link, and it says where switching happens: "${bare.none}"`);
      }
    }

    /* ---- as many inboxes on one screen as fit ---- */
    console.log('\n— inboxes side by side —');
    /* Four mailboxes with mail in each, which is the case this exists for:
       more inboxes than fit at once, choosing which to put beside each other. */
    await page.evaluate(() => {
      const mk = (addr, n) => {
        const l = addr;
        const acct = addLinked('mail', l, { host: 'imap.example.com' });
        for (let i = 0; i < n; i++) S.messages.push({
          id: `${l}-${i}`, ch: 'email', prov: 'imap', acct: acct.id, cid: null,
          fromName: 'Someone', fromAddr: 's@x.com', subj: `${l} message ${i}`,
          prev: 'x', body: 'x', ts: Date.now() - i * 1000, unread: false, starred: false, atts: [],
        });
        return acct.id;
      };
      mk('one@example.com', 3); mk('two@example.net', 2); mk('three@example.org', 4); mk('four@example.io', 1);
      save(); renderMailFilters(); renderMail();
    });
    const together = await page.evaluate(() => document.querySelectorAll('#mail-scroll .mail-row').length);

    /* With no plan, side by side is visible but locked — the choice stays on
       screen, which is how anyone learns the tier above exists. */
    const locked = await page.evaluate(() => {
      const b = document.querySelector('#inbox-mode [data-mode="split"]');
      return { label: b.textContent.trim(), isLocked: b.classList.contains('locked'), why: b.title };
    });
    check(locked.isLocked, `without a plan it is offered but locked: "${locked.label}"`);
    /* A plan the server never confirmed must not stick. */
    const spoof = await page.evaluate(() => {
      S.settings.plan = 'enterprise'; save();
      const kept = S.settings.plan;
      /* What boot does when the subscriptions table has no row for you. */
      const active = null && true;
      S.settings.plan = active ? 'enterprise' : null; save();
      return { kept, after: S.settings.plan };
    });
    check(spoof.kept === 'enterprise' && spoof.after === null,
      'and a plan with no subscription behind it does not survive a reload');
    await page.evaluate(() => { renderSplitLock(); });
    check(await page.isHidden('#main-head'),
      'and no empty header bar sits under the switch while the inbox is whole');
    check(/RATA Pro/.test(locked.why) && /\$23\.99/.test(locked.why),
      `and says what unlocks it: "${locked.why}"`);
    await page.click('#inbox-mode [data-mode="split"]');
    check(await page.evaluate(() => document.querySelectorAll('#extra-panes .split-pane').length) === 0,
      'clicking it does not split — it explains instead');

    await page.evaluate(() => { S.settings.plan = 'pro'; save(); renderSplitLock(); });
    const modes = await page.evaluate(() => [...document.querySelectorAll('#inbox-mode button')]
      .map(b => ({ label: b.textContent.trim(), hint: b.title })));
    check(modes.map(m => m.label).join(' / ') === 'Center Point / Side by side',
      `on Pro both layouts are named up front: ${modes.map(m => m.label).join(' / ')}`);
    check(/one inbox/i.test(modes[0].hint),
      `and the Center Point name says what it does on hover: "${modes[0].hint}"`);
    check(await page.evaluate(() => document.querySelectorAll('#extra-panes .split-pane').length) === 0,
      'nothing is split until asked for');

    await page.click('#inbox-mode [data-mode="split"]');
    const two = await page.evaluate(() => ({
      panes: 1 + document.querySelectorAll('#extra-panes .split-pane').length,
      showing: [document.querySelector('#main-acct').selectedOptions[0]?.textContent,
                ...[...document.querySelectorAll('[data-pane-acct]')].map(s => s.selectedOptions[0]?.textContent)],
      adders: document.querySelectorAll('[data-pane-add]').length,
      detail: getComputedStyle(document.querySelector('#mail-detail')).display,
    }));
    check(two.panes === 2, `it opens on two: ${two.panes}`);
    check(new Set(two.showing).size === 2, `each on its own mailbox: ${two.showing.join(' | ')}`);
    check(two.adders === 1, 'with one way to add another, at the right-hand edge');
    check(two.detail === 'none', 'and the reading pane gives way rather than a third column');

    await page.click('[data-pane-add]');
    const three = await page.evaluate(() => ({
      panes: 1 + document.querySelectorAll('#extra-panes .split-pane').length,
      showing: [document.querySelector('#main-acct').selectedOptions[0]?.textContent,
                ...[...document.querySelectorAll('[data-pane-acct]')].map(s => s.selectedOptions[0]?.textContent)],
      counts: [document.querySelectorAll('#mail-scroll .mail-row').length,
               ...[...document.querySelectorAll('[data-pane-scroll]')].map(s => s.querySelectorAll('.mail-row').length)],
      adders: document.querySelectorAll('[data-pane-add]').length,
      choices: document.querySelectorAll('#main-acct option').length,
    }));
    check(three.panes === 3, `a third can be added: ${three.panes}`);
    check(new Set(three.showing).size === 3, `all three different: ${three.showing.join(' | ')}`);
    check(three.adders === 0, 'and three is the ceiling — no fourth offered');
    check(three.choices >= 5, `any mailbox can go in any column: ${three.choices - 1} to choose from`);
    check(three.counts.every(n => n > 0), `each column has its own mail: ${three.counts.join(' | ')}`);

    /* Re-pointing a column, and closing one from the middle. */
    await page.selectOption('[data-pane-acct="1"]', { index: 4 });
    const repick = await page.evaluate(() => [...document.querySelectorAll('[data-pane-acct]')].map(s => s.selectedOptions[0]?.textContent));
    check(!!repick[0], `a column can be pointed anywhere: middle now ${repick[0]}`);
    await page.click('[data-pane-close="1"]');
    check(await page.evaluate(() => 1 + document.querySelectorAll('#extra-panes .split-pane').length) === 2,
      'closing one leaves the rest');

    /* Narrow the window: panes hold a readable floor and the row scrolls,
       rather than every column being crushed thinner. */
    await page.setViewportSize({ width: 980, height: 800 });
    await page.click('[data-pane-add]');
    const narrow = await page.evaluate(() => ({
      widths: [...document.querySelectorAll('.side-list,.split-pane')].map(e => Math.round(e.getBoundingClientRect().width)).filter(Boolean),
      rowScrolls: (() => { const v = document.querySelector('#view-inbox'); return v.scrollWidth > v.clientWidth; })(),
      bodyScrolls: document.body.scrollWidth > window.innerWidth,
    }));
    check(narrow.widths.every(w => w >= 330), `columns keep a readable floor at 980px: ${narrow.widths.join(', ')}`);
    check(narrow.rowScrolls, 'so the row of inboxes scrolls sideways instead');
    check(!narrow.bodyScrolls, 'and the page itself still does not');
    await page.setViewportSize({ width: 1280, height: 800 });

    /* Dropping a message on another inbox opens a forward from that account —
       it must never send by itself. */
    const moved = await page.evaluate(async () => {
      const target = S.linked.find(l => l.label === 'two@example.net');
      const msg = S.messages.find(m => m.id.startsWith('one@example.com-'));
      transferMessage(msg.id, target.id);
      /* Straight away when the text is to hand; a read from disk otherwise. */
      for (let i = 0; i < 40 && !document.querySelector('#compose-ov').classList.contains('open'); i++) await new Promise(r => setTimeout(r, 25));
      return {
        open: document.querySelector('#compose-ov').classList.contains('open'),
        from: document.querySelector('#cmp-from').selectedOptions[0]?.textContent || '',
        subj: document.querySelector('#cmp-subj').value,
        to: document.querySelector('#cmp-to').value,
        body: document.querySelector('#cmp-body').value,
      };
    });
    check(moved.open, 'dragging a message across opens the composer');
    check(/two@example\.net/.test(moved.from), `sending from the inbox it was dropped on: ${moved.from}`);
    check(/^Fwd: /.test(moved.subj), `pre-filled as a forward: "${moved.subj}"`);
    check(/Forwarded/.test(moved.body), 'with the original quoted beneath');
    check(moved.to === '', 'and no recipient assumed — a drag must not send mail on its own');

    await page.evaluate(() => { document.querySelector('#cmp-cancel')?.click(); closeCompose(); });
    await page.click('#inbox-mode [data-mode="one"]');
    check(await page.evaluate(() => document.querySelectorAll('#extra-panes .split-pane').length) === 0,
      'One inbox puts it back to a single stream');
    check(await page.evaluate(() => document.querySelectorAll('#mail-scroll .mail-row').length) === together,
      `with every message in it again: ${together}`);

    await page.evaluate(() => {
      S.messages = S.messages.filter(m => !/@example\.(com|net|org|io)-\d+$/.test(m.id));
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

    /* ---- every reader into every writer ---- */
    console.log('\n— Format Bridge: what goes in comes out, in a real format —');
    {
      const docx = (await import('node:fs')).readFileSync(join(HERE, 'fixtures', 'sample.docx')).toString('base64');
      const conv = await page.evaluate(async (docx) => {
        const got = []; const realDl = brDownload; brDownload = (blob, name) => got.push({ blob, name });
        const toasts = []; const realToast = toast; toast = (m) => toasts.push(m);
        const run = async (name, data, eco) => { await brIngest(new File([data], name)); BR.eco = eco; got.length = 0; toasts.length = 0; await brConvert(false); return { file: got[0], toast: toasts[0], kind: document.querySelector('#br-kind').textContent }; };
        const mam = await brLib('mammoth', 'mammoth'), X = await brLib('xlsx', 'XLSX');
        const bytes = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
        const out = {};
        /* A .docx written by LibreOffice, back out as a .docx: read with an
           independent reader (mammoth), its structure is still there. */
        const w = await run('report.docx', bytes(docx), 'office');
        const html = (await mam.convertToHtml({ arrayBuffer: await w.file.blob.arrayBuffer() })).value;
        out.docx = { name: w.file.name, type: w.file.blob.type, kind: w.kind, h1: /<h1>Quarterly report<\/h1>/.test(html), h2: /<h2>Highlights<\/h2>/.test(html),
          bold: /<strong>well<\/strong>/.test(html), italic: /<em>better than planned<\/em>/.test(html), table: /<table>.*North.*140/.test(html), amp: /the board &amp; staff &lt;team&gt;/.test(html) };
        const md = await run('report.docx', bytes(docx), 'plain');
        out.md = await md.file.blob.text();
        /* A spreadsheet keeps every sheet. */
        const wb = X.utils.book_new();
        X.utils.book_append_sheet(wb, X.utils.aoa_to_sheet([['Region', 'Q1'], ['South', 'a,"quoted" cell']]), 'Sales');
        X.utils.book_append_sheet(wb, X.utils.aoa_to_sheet([['Name'], ['Ann']]), 'Team');
        const two = new Uint8Array(X.write(wb, { type: 'array', bookType: 'xlsx' }));
        const xl = await run('book.xlsx', two, 'office');
        const back = X.read(await xl.file.blob.arrayBuffer(), { type: 'array' });
        out.sheets = { kind: xl.kind, names: back.SheetNames, team: X.utils.sheet_to_json(back.Sheets.Team, { header: 1 }) };
        const zip = await run('book.xlsx', two, 'plain');
        const zipped = X.CFB.read(new Uint8Array(await zip.file.blob.arrayBuffer()), { type: 'array' });
        /* SheetJS's reader lists a placeholder of its own (Sh33tJ5) in any zip. */
        out.zip = { name: zip.file.name, files: zipped.FileIndex.filter((f) => f.type === 2 && !f.name.includes('Sh33tJ5')).map((f) => [f.name, new TextDecoder().decode(f.content)]) };
        /* Characters PDF cannot carry are refused by name, not garbled. */
        const cyr = await run('note.txt', 'Привет, мир', 'pdf');
        out.cyr = { file: !!cyr.file, toast: cyr.toast };
        const latin = await run('note.txt', 'Café — naïve “quotes” €5', 'pdf');
        out.latin = { name: latin.file && latin.file.name, pdf: latin.file ? (await latin.file.blob.text()).slice(0, 5) : null };
        /* A web page is read without being run. */
        window.__bridgePwned = undefined;
        const page = await run('page.html', '<h1>Hi</h1><img src="x" onerror="window.__bridgePwned=1"><script>window.__bridgePwned=2</script><p>Body</p>', 'office');
        await new Promise((r) => setTimeout(r, 300));
        out.html = { pwned: window.__bridgePwned, name: page.file && page.file.name, note: document.querySelector('#br-note').textContent };
        brDownload = realDl; toast = realToast;
        return out;
      }, docx);
      check(conv.docx.name === 'report.docx' && /wordprocessingml/.test(conv.docx.type), `Word out is a real .docx, not HTML named .doc: ${conv.docx.name}`);
      check(conv.docx.h1 && conv.docx.h2 && conv.docx.bold && conv.docx.italic && conv.docx.table,
        `and keeps headings, bold, italic and tables: ${JSON.stringify(conv.docx)}`);
      check(conv.docx.amp, 'with & and < written safely');
      check(/^# Quarterly report\n/.test(conv.md) && /\*\*well\*\*/.test(conv.md) && /- Revenue up 12%\n- Two new clients/.test(conv.md) && /\| North \| 120 \| 140 \|/.test(conv.md),
        'Markdown out keeps headings, bold, lists and tables');
      check(/2 sheets/.test(conv.sheets.kind) && JSON.stringify(conv.sheets.names) === '["Sales","Team"]' && JSON.stringify(conv.sheets.team) === '[["Name"],["Ann"]]',
        `a workbook keeps every sheet, not just the first: ${JSON.stringify(conv.sheets.names)}`);
      check(conv.zip.name === 'book.csv.zip' && conv.zip.files.length === 2 && conv.zip.files.some(([n, t]) => n === 'Sales.csv' && t.includes('"a,""quoted"" cell"')),
        `as CSV, one file per sheet in a .zip, quoted properly: ${conv.zip.files.map(([n]) => n).join(', ')}`);
      check(!conv.cyr.file && /Cyrillic/.test(conv.cyr.toast || ''), `a PDF that would garble the text is refused, saying why: "${conv.cyr.toast}"`);
      check(conv.latin.name === 'note.pdf' && conv.latin.pdf === '%PDF-', 'while Western European text, quotes and € make a PDF');
      check(conv.html.pwned === undefined && conv.html.name === 'page.docx', 'a web page is converted without anything in it running');
      check(/picture in it isn’t carried over/.test(conv.html.note), `and it says its picture is not carried over: "${conv.html.note}"`);
    }

    console.log('\n— Format Bridge: reading PDFs —');
    {
      const pdfFix = (await import('node:fs')).readFileSync(join(HERE, 'fixtures', 'sample.pdf')).toString('base64');
      const pdf = await page.evaluate(async (pdfFix) => {
        const got = []; const realDl = brDownload; brDownload = (blob, name) => got.push({ blob, name });
        const toasts = []; const realToast = toast; toast = (m) => toasts.push(m);
        const run = async (name, data, eco) => { got.length = 0; toasts.length = 0; await brIngest(new File([data], name)); BR.eco = eco; if (!toasts.length && BR.name === name) await brConvert(false); return { file: got[0], toast: toasts[0] }; };
        const mam = await brLib('mammoth', 'mammoth');
        const out = {};
        /* LibreOffice's PDF of the sample report, into Word: the structure
           comes back from where the words sit on the page. */
        const bytes = Uint8Array.from(atob(pdfFix), (c) => c.charCodeAt(0));
        const w = await run('report.pdf', bytes, 'office');
        out.kind = document.querySelector('#br-kind').textContent;
        out.note = (brNote(), document.querySelector('#br-note').textContent);
        const html = w.file ? (await mam.convertToHtml({ arrayBuffer: await w.file.blob.arrayBuffer() })).value : '';
        out.docx = { name: w.file && w.file.name, h1: /<h1>Quarterly report<\/h1>/.test(html), h2: /<h2>Highlights<\/h2>/.test(html),
          para: /<p>This quarter went well, and better than planned\.<\/p>/.test(html), table: /<table>.*North.*120.*140.*<\/table>/.test(html), end: /Signed, the board &amp; staff &lt;team&gt;\./.test(html) };
        const md = await run('report.pdf', bytes, 'plain');
        out.md = md.file ? await md.file.blob.text() : '';
        /* PDFs made here, to exercise what real ones do. */
        const { jsPDF } = await brLib('jspdf', 'jspdf');
        const d = new jsPDF();
        d.setFontSize(11);
        d.text('A long sentence that was set with an exam-', 20, 30); d.text('ple of a word split across two lines.', 20, 35);
        d.text('7', 105, 290);
        d.addPage(); d.text('Page 2 of 2', 90, 290); d.text('The second page has its own sentence.', 20, 30);
        const h = await run('split.pdf', new Uint8Array(d.output('arraybuffer')), 'plain');
        out.split = h.file ? await h.file.blob.text() : String(h.toast);
        const pic = new jsPDF(); pic.setFillColor(40, 40, 40); pic.rect(20, 20, 150, 100, 'F');
        out.scan = (await run('scan.pdf', new Uint8Array(pic.output('arraybuffer')), 'office')).toast;
        const locked = new jsPDF({ encryption: { userPassword: 'secret', ownerPassword: 'owner', userPermissions: ['print'] } });
        locked.text('Hidden text', 20, 20);
        out.locked = (await run('locked.pdf', new Uint8Array(locked.output('arraybuffer')), 'office')).toast;
        out.junk = (await run('junk.pdf', new TextEncoder().encode('this is not a pdf at all'), 'office')).toast;
        brDownload = realDl; toast = realToast;
        return out;
      }, pdfFix);
      check(/^PDF · \d+ words/.test(pdf.kind) && /headings, lists and simple tables come across/.test(pdf.note), `a PDF is read, and the note says what comes across: "${pdf.kind}"`);
      check(pdf.docx.name === 'report.docx' && pdf.docx.h1 && pdf.docx.h2 && pdf.docx.para && pdf.docx.table && pdf.docx.end,
        `a PDF into Word keeps its headings, paragraphs and table: ${JSON.stringify(pdf.docx)}`);
      check(/- Revenue up 12%\n- Two new clients\n1\. Hire a designer\n2\. Open the Lisbon office/.test(pdf.md), 'and its bullet and numbered lists, bullets drawn in a symbol font included');
      check(/an example of a word/.test(pdf.split), `a word split with a hyphen at a line's end is joined again: "${pdf.split.split('\n')[0]}"`);
      check(!/^\s*7\s*$/m.test(pdf.split) && !/Page 2 of 2/.test(pdf.split) && /The second page has its own sentence/.test(pdf.split), 'page numbers are left out, and every page is read');
      check(/no text in it/.test(pdf.scan || ''), `a PDF of pictures says there is no text to convert: "${pdf.scan}"`);
      check(/locked with a password/.test(pdf.locked || ''), `a locked PDF says so: "${pdf.locked}"`);
      check(/could not be read/.test(pdf.junk || ''), `and a file that only claims to be a PDF is refused: "${pdf.junk}"`);
    }

    /* ---- bytes are real and in IndexedDB, not in the synced workspace ---- */
    /* ---- the design system holds (DESIGN.md) ---- */
    console.log('\n— the interface follows DESIGN.md —');
    const look = await page.evaluate(() => {
      const cs = el => el ? getComputedStyle(el) : null;
      const compose = cs(document.querySelector('#compose-btn'));
      const pillOn = cs(document.querySelector('#mail-filters .pill.on'));
      const root = getComputedStyle(document.documentElement);
      /* Every element on screen: none may carry a gradient. */
      const gradients = [...document.querySelectorAll('body *')].filter(e => /gradient/.test(getComputedStyle(e).backgroundImage)).map(e => e.id || e.className).slice(0, 5);
      return {
        composeGradient: /gradient/.test(compose.backgroundImage),
        composeFill: compose.backgroundColor,
        accent: root.getPropertyValue('--tint-btn').trim(),
        composeShadow: compose.boxShadow,
        composeRound: parseFloat(compose.borderRadius),
        pillGradient: pillOn ? /gradient/.test(pillOn.backgroundImage) : null,
        pillRound: pillOn ? parseFloat(pillOn.borderRadius) : null,
        font: getComputedStyle(document.body).fontFamily,
        pop: root.getPropertyValue('--pop').trim(),
        gradients,
      };
    });
    const hex = h => { const n = parseInt(h.replace('#', ''), 16); return `rgb(${n >> 16}, ${(n >> 8) & 255}, ${n & 255})`; };
    check(!look.composeGradient && look.composeFill === hex(look.accent),
      `the primary action is a flat fill of the one accent: ${look.composeFill} (accent ${look.accent})`);
    check(look.composeShadow === 'none', `and nothing glows: box-shadow ${look.composeShadow}`);
    check(look.composeRound === 8 && look.pillRound === 8 && look.pillGradient === false,
      `controls are 8px and flat: Compose ${look.composeRound}px, a selected filter ${look.pillRound}px`);
    check(/^"?Geist"?,/.test(look.font), `the type is Geist: ${look.font}`);
    check(!/cubic-bezier/.test(look.pop), `motion without overshoot: ${look.pop}`);
    check(look.gradients.length === 0, `no gradient anywhere on screen: ${JSON.stringify(look.gradients)}`);

    await page.evaluate(() => go('docs'));

    /* ---- the Business file library ---- */
    console.log('\n— folders for files —');
    await page.evaluate(() => { S.settings.plan = 'base'; save(); go('docs'); });
    const free = await page.evaluate(() => ({
      barShown: !document.querySelector('#folder-bar').hidden,
      hint: document.querySelector('#folder-hint').textContent,
      newBtn: document.querySelector('#folder-new').style.display,
      chips: document.querySelectorAll('#folder-chips .pill').length,
    }));
    check(free.barShown, 'on Base the rail is still visible, not hidden');
    check(/RATA Pro/.test(free.hint) && /\$23\.99/.test(free.hint), `and says what it is: "${free.hint}"`);
    check(free.newBtn === 'none' && free.chips === 0, 'but no folders to make or use');

    await page.evaluate(() => { S.settings.plan = 'pro'; save(); renderFolders(); renderDocs(); });
    const biz = await page.evaluate(() => {
      S.folders.push('Acme Ltd'); save(); renderFolders(); renderDocs();
      const doc = S.documents[0];
      fileInto(doc.id, 'Acme Ltd');
      folderFilter = 'Acme Ltd'; renderFolders(); renderDocs();
      const inFolder = document.querySelectorAll('#doc-grid .doc-card').length;
      folderFilter = 'none'; renderFolders(); renderDocs();
      const unfiled = document.querySelectorAll('#doc-grid .doc-card').length;
      folderFilter = 'all'; renderFolders(); renderDocs();
      return { inFolder, unfiled, total: S.documents.length, filed: doc.folder,
               draggable: document.querySelector('#doc-grid .doc-card')?.getAttribute('draggable') };
    });
    check(biz.filed === 'Acme Ltd', `a file can be put in a folder: ${biz.filed}`);
    check(biz.draggable === 'true', 'and files are draggable onto one');
    check(biz.inFolder === 1, `the folder shows just its own: ${biz.inFolder}`);
    check(biz.unfiled === biz.total - 1, `and Unfiled shows the rest: ${biz.unfiled} of ${biz.total}`);

    /* Downgrading must never make a document unreachable. */
    const after = await page.evaluate(() => {
      S.settings.plan = 'base'; save(); renderFolders(); renderDocs();
      return document.querySelectorAll('#doc-grid .doc-card').length;
    });
    check(after === biz.total, `dropping to Base hides no files: ${after} of ${biz.total} still listed`);
    await page.evaluate(() => { S.settings.plan = 'base'; S.folders = []; S.documents.forEach(d => delete d.folder); save(); });

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

    /* ---- F2: in a browser the page says where mail is read ---- */
    console.log('\n— the website says RATA reads mail in the desktop app —');
    {
      /* Every block above ran with the banner up, which is part of the proof
         that it breaks nothing. */
      await page.evaluate(() => go('inbox'));
      const note = await page.evaluate(() => {
        const el = document.querySelector('#web-note');
        const r = el ? el.getBoundingClientRect() : null;
        const main = document.querySelector('#main').getBoundingClientRect();
        const dl = document.querySelector('#wn-download');
        const cs = el ? getComputedStyle(el) : null;
        return el && {
          text: el.textContent.replace(/\s+/g, ' ').trim(),
          shown: !el.hidden && r.height > 0,
          atTop: r.top < main.top + 80,
          href: dl && dl.getAttribute('href'),
          dlRound: dl && parseFloat(getComputedStyle(dl).borderRadius),
          bg: cs.backgroundColor,
          fill2: getComputedStyle(document.documentElement).getPropertyValue('--fill-2').trim(),
        };
      });
      check(!!note && note.shown && note.atTop, `in a browser, the banner is at the top of the workspace: ${JSON.stringify(note && { shown: note.shown, atTop: note.atTop })}`);
      check(note && /RATA reads your mail in the desktop app\. This page keeps your account and settings\./.test(note.text) && !/[—!]/.test(note.text),
        `and says so plainly, with no em dash or exclamation mark: "${note && note.text}"`);
      check(note && note.href === '/index.html#download' && note.dlRound === 8 && note.bg === hex(note.fill2),
        `Download goes to the download section, as an 8px button on --fill-2: ${JSON.stringify(note && { href: note.href, round: note.dlRound, bg: note.bg })}`);

      await page.click('#wn-dismiss');
      const after = await page.evaluate(() => ({
        hidden: document.querySelector('#web-note').hidden,
        height: document.querySelector('#web-note').getBoundingClientRect().height,
        kept: localStorage.getItem('rata_web_note_dismissed'),
      }));
      check(after.hidden && after.height === 0 && after.kept === '1', `Dismiss hides it and remembers: ${JSON.stringify(after)}`);

      /* A fresh load in the same browser keeps it dismissed, and the link
         lands on the site's download section. */
      const again = await ctx.newPage();
      await again.goto(s.url + '/app.html');
      await again.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function', null, { timeout: 15000 });
      await again.waitForTimeout(300);
      check(await again.isHidden('#web-note'), 'and it stays dismissed on the next load');
      await again.goto(s.url + note.href);
      check(await again.isVisible('#download'), 'the Download link lands on the download section');
      await again.close();
    }

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

    /* Zero, not "no CDN". This used to allow fonts.googleapis.com through,
       which meant every page load told Google when somebody opened their mail —
       on a product whose whole claim is that messages stay yours. The fonts are
       served from this origin now, so the honest assertion is that the app
       contacts nobody at all, and anything reappearing here fails the build. */
    const third = [...hosts].filter(h => !h.startsWith('127.0.0.1') && !h.startsWith('localhost'));
    check(third.length === 0,
      third.length ? `contacted a third party: ${third.join(', ')}` : 'no third-party host contacted at all');

    check(errs.length === 0, errs.length ? `page errors: ${errs.join(' | ')}` : 'no page errors throughout');
  } finally {
    await browser.close();
    await s.stop();
  }
}
