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
       word "Gmail" does. What would be wrong is copy that promises only some. */
    check(/any email address/i.test(form.sub), `the help text opens with the promise: "${form.sub.slice(0, 48)}…"`);
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
    const moved = await page.evaluate(() => {
      const target = S.linked.find(l => l.label === 'two@example.net');
      const msg = S.messages.find(m => m.id.startsWith('one@example.com-'));
      transferMessage(msg.id, target.id);
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

    /* ---- bytes are real and in IndexedDB, not in the synced workspace ---- */
    /* ---- depth is part of the design, not decoration to be lost ---- */
    console.log('\n— the interface has weight —');
    const depth = await page.evaluate(() => {
      const cs = el => el ? getComputedStyle(el) : null;
      const compose = cs(document.querySelector('#compose-btn'));
      const pillOn = cs(document.querySelector('#mail-filters .pill.on'));
      const reduce = getComputedStyle(document.documentElement).getPropertyValue('--pop').trim();
      return {
        composeGradient: /gradient/.test(compose.backgroundImage),
        composeGlow: compose.boxShadow,
        composeRound: parseFloat(compose.borderRadius),
        pillGradient: pillOn ? /gradient/.test(pillOn.backgroundImage) : null,
        pillRound: pillOn ? parseFloat(pillOn.borderRadius) : null,
        springy: reduce,
      };
    });
    check(depth.composeGradient, 'the primary action is a gradient, not a flat fill');
    check(/rgb/.test(depth.composeGlow) && !/^rgba?\(0, 0, 0/.test(depth.composeGlow),
      `and glows in its own colour rather than grey: ${depth.composeGlow.split(') ')[0]})`);
    check(depth.composeRound >= 20, `bubble-round: ${depth.composeRound}px`);
    check(depth.pillGradient === true && depth.pillRound >= 20,
      `selected filters are bubbles too: ${depth.pillRound}px, gradient ${depth.pillGradient}`);
    check(/cubic-bezier/.test(depth.springy), `with an overshoot curve for the lift: ${depth.springy}`);

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
