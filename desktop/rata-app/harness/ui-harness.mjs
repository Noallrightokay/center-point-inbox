// Drives the real desktop interface (ui/ as sync-ui.sh assembles it) with a
// fake Tauri backend, to check the bridge fixes behave — not just parse.
//
// Run from the repository root, after ./sync-ui.sh and `npm ci` in rata-next:
//   (cd desktop/rata-app/ui && python3 -m http.server 3181 --bind 127.0.0.1 &)
//   node desktop/rata-app/harness/ui-harness.mjs http://127.0.0.1:3181
// Exit status is non-zero when any check fails.
import pw from '../../../rata-next/node_modules/playwright/index.js';
const { chromium } = pw;
const B = process.argv[2];
const STORE_SOFT_CAP_JS = 3500000;
let fails = 0;
const check = (c, m) => { console.log(`${c ? '  PASS' : '  FAIL'}  ${m}`); if (!c) fails++; };

const MOCK = ({ licensed, ms, old }) => {
  // Playwright injects this into every frame; the app is only the top one.
  if (window !== window.top) return;
  window.__mock = {
    licensed,
    mailboxes: [{ email: 'me@example.com', host: 'imap.example.com', port: 993, label: 'Example' }],
    unlinkFails: false,
    setLicenceRejects: false,
    refresh: null,
    calls: [],
    /* Whether this build can sign in with Microsoft (RATA_MS_CLIENT_ID). */
    msConfigured: !!ms,
    /* Domains the fake DNS puts at Microsoft 365. */
    msDomains: [],
  };
  const M = window.__mock;
  const standing = () => M.licensed
    ? { licensed: true, message: M.planMessage || 'Licensed for RATA Pro until 26 October 2026.', plan: M.plan || { key: 'pro', label: 'RATA Pro', mail: null, chat: 3, split: true, ai: true },
        used: M.mailboxes.length, limit: M.plan && M.plan.mail !== undefined ? M.plan.mail : null, renewSoon: false, token: 'v1.test-licence.sig' }
    : old
      /* A licence on disk that has expired: bridge.js offers it for renewal. */
      ? { licensed: false, message: 'This licence expired on 1 March 2026.', token: 'v1.old-licence.sig', renewSoon: false }
      : { licensed: false, message: 'Enter your licence key.', token: null, renewSoon: false };
  window.__TAURI__ = { core: { invoke: async (cmd, args) => {
    M.calls.push([cmd, args]);
    switch (cmd) {
      case 'licence_status': return standing();
      case 'set_licence':
        if (M.setLicenceRejects) throw 'The licence could not be written to disk';
        return standing();
      case 'list_mailboxes': return M.mailboxes;
      case 'unlink_mailbox':
        if (M.unlinkFails) throw 'This computer’s keychain refused access';
        M.mailboxes = M.mailboxes.filter((m) => m.email !== args.email);
        return null;
      case 'refresh_mail':
        if (M.refreshDelay) await new Promise((r) => setTimeout(r, M.refreshDelay));
        return M.refresh || { messages: [], problems: [], skipped: [] };
      case 'older_mail': {
        if (M.olderFails) throw { email: args.email, kind: 'net', error: 'imap.example.com could not be reached' };
        if (args.folder === 'sent') return (M.sentOlder || []).filter((m) => m.uid < args.beforeUid);
        if (args.folder && args.folder.named) return (M.folderOlder || []).filter((m) => m.uid < args.beforeUid);
        const inbox = M.inbox || [];
        const older = inbox.filter((u) => u < args.beforeUid).sort((a, b) => b - a).slice(0, args.limit || 50);
        return older.map((u) => M.mk(u));
      }
      case 'link_mailbox':
        /* The engine's refusal (a Microsoft 365 domain found by its MX, say):
           core::link answers `failed` with the reason. */
        if (M.linkFails) return { outcome: 'failed', error: M.linkFails };
        /* C2: a Microsoft mailbox reached with a password. The password went
           nowhere; the answer says whether this build can sign in instead. */
        if (M.linkMicrosoft) return { outcome: 'microsoft', configured: M.msConfigured,
          error: M.msConfigured ? 'Microsoft 365 mailboxes sign in with Microsoft, not with a password, so RATA did not send the password anywhere. Use Sign in with Microsoft.' : M.linkMicrosoft };
        return { outcome: 'ok', mailbox: { email: args.email, host: 'imap.example.com', port: 993, label: 'Example', source: 'table' } };
      case 'microsoft_ready':
        return M.msConfigured;
      case 'discover_mailbox': {
        const d = String(args.email).split('@')[1] || '';
        const ms = M.msDomains.includes(d);
        return { label: ms ? 'Microsoft 365' : 'Example', microsoft: ms, configured: M.msConfigured,
          ...(ms && !M.msConfigured ? { note: 'That address’s mail is at Microsoft 365, which RATA cannot open yet. Microsoft only lets other apps into Microsoft 365 mailboxes through its own sign-in page (OAuth), and no longer accepts passwords for IMAP. RATA does not have that sign-in yet.' } : {}) };
      }
      /* The sign-in waits for the browser; the harness answers for it with
         __mock.msPending(outcome), or Cancel does. */
      case 'link_microsoft':
        if (!M.msConfigured) return { outcome: 'microsoft', configured: false, error: 'Outlook.com, Hotmail and Live mailboxes cannot be added to RATA yet.' };
        return new Promise((r) => { M.msPending = (o) => { M.msPending = null; r(o); }; });
      case 'cancel_microsoft':
        if (M.msPending) { M.msPending({ outcome: 'cancelled' }); return true; }
        return false;
      case 'reread_mail':
        if (args.folder === 'archive') {
          M.archReads = (M.archReads || []).concat([args]);
          return args.uids.map((u) => ({ id: args.email + '_archive_' + u, folder: 'archive', acct: args.email, acct_label: 'Example',
            from_name: 'Bo', from_addr: 'bo@example.org', to_name: '', to_addr: args.email, subject: 'Archived later ' + u, preview: 'p', body: 'b',
            ts: Date.now() - u * 1000, unread: false, starred: false, uid: u, uidvalidity: args.uidvalidity, message_id: 'l' + u + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }));
        }
        M.rereads = (M.rereads || 0) + 1;
        return args.uids.map((u) => ({ id: args.email + '_' + u, acct: args.email, acct_label: 'Example', from_name: 'Ann',
          from_addr: 'ann@example.org', subject: 'Old ' + u, preview: 'Decoded ' + u, body: 'Decoded body of ' + u + ' — café',
          ts: 1e12 + u, unread: false, starred: false, uid: u, uidvalidity: args.uidvalidity, message_id: 'o' + u + '@example.org',
          reply_to: '', truncated: u === 3 }));
      case 'read_attachment': {
        if (M.readFails) throw { email: args.email, kind: 'too-large', error: 'huge.pdf is too large to convert in RATA (40 MB; the most is 25 MB). Save it and convert it elsewhere.' };
        const f = (M.readable || {})[args.index];
        if (!f) throw { email: args.email, kind: 'gone', error: 'That attachment is no longer in the message.' };
        return f;
      }
      case 'open_message':
        if (M.openFails) throw { email: args.email, kind: 'net', error: 'imap.example.com could not be reached' };
        if (M.openAtts && args.uid === 90) return { text: 'Please see the attached report.', truncated: false, attachments: M.openAtts };
        /* 60 and up are HTML mail. The HTML here has NOT been through the Rust
           sanitiser, on purpose: it tests the frame's own lock. */
        if (args.uid >= 60) return { text: 'Your order shipped. Track it <https://shop.example/t>', truncated: false, attachments: [],
          html: args.uid === 61 ? null : '<h1 style="color:rgb(200,0,0)">Your order shipped</h1><p>Thanks!</p>'
            + '<img id="pix" src="https://tracker.example/open.gif" width="1" height="1">'
            + '<script>parent.window.__pwned = "script"</script>'
            + '<img src="x" onerror="parent.window.__pwned = \'onerror\'">'
            + '<a id="lnk" href="https://phish.example/login">Sign in</a>'
            + '<a id="lnk2" href="https://phish.example/self" target="_self">Also</a>',
          remote_images: args.uid === 60 };
        return { text: 'The whole of message ' + args.uid + ', every paragraph of it.', truncated: false,
          attachments: [{ index: 1, name: 'Q3 figures.pdf', mime: 'application/pdf', size: 245760 },
            { index: 2, name: '<img src=x onerror=window.__pwned=1>.pdf', mime: 'application/pdf', size: 10 }] };
      case 'save_file':
        M.files = (M.files || []).concat([args]);
        if (M.saveFails) throw 'It could not be saved in /home/me/Downloads (disk full).';
        return { path: '/home/me/Downloads/' + args.name, name: args.name, size: atob(args.data).length };
      case 'list_folders':
        if (M.foldersFail) throw { email: args.email, kind: 'net', error: 'imap.example.com could not be reached: its folders could not be listed' };
        return M.folders || [];
      case 'folder_mail': {
        M.folderReads = (M.folderReads || []).concat([args]);
        if (M.folderFails) throw { email: args.email, kind: 'stale', error: 'me@example.com no longer has that folder — it was renamed or removed. Pick it again from Folders.' };
        return (M.inFolder || {})[args.folder && args.folder.named] || [];
      }
      case 'watching':
        return M.live || [];
      case 'notify_mail':
        M.notified = (M.notified || []).concat([args]);
        return null;
      case 'check_update':
        return M.update || { enabled: false, current: '0.1.23', releases: 'https://github.com/Noallrightokay/center-point-inbox/releases' };
      case 'install_update':
        M.installs = (M.installs || 0) + 1;
        if (M.installFails) throw M.installFails;
        return null;
      case 'open_link':
        M.opened = (M.opened || []).concat([args.url]);
        if (!/^https?:\/\//i.test(args.url)) throw 'RATA only opens web addresses (http and https).';
        return null;
      case 'save_attachment': {
        M.saved = (M.saved || []).concat([args]);
        /* As core::save_attachment: a program named as a document (judged
           from the mailbox's copy, M.disguisedAt) needs `confirmed`. */
        const name = (M.openAtts || []).find((a) => a.index === args.index)?.name;
        if ((M.disguisedAt || []).includes(args.index) && args.confirmed !== true)
          throw { email: args.email, kind: 'needs-confirmation', error: name + ' is a program (.exe) named to look like a document. RATA saves it only after you say so.' };
        return name ? { path: '/home/me/Downloads/' + name, name, size: 4 } : { path: '/home/me/Downloads/Q3 figures.pdf', name: 'Q3 figures.pdf', size: 245760 };
      }
      /* F1: as core::save_draft. A new UID for every save; the copy before
         is "replaced" whenever the page names one. */
      case 'save_draft': {
        if (M.draftNoPlace) return { outcome: 'no-place', error: args.draft.from + ' has no Drafts folder, so RATA keeps this draft on this computer only.' };
        if (M.draftFails) throw { email: args.draft.from, kind: 'net', error: 'The draft was not saved to Drafts in ' + args.draft.from + ' — imap.example.com could not be reached. It is kept here and will be saved again.' };
        M.draftUid = (M.draftUid || 100) + 1;
        return { outcome: 'saved', id: args.draft.from + '_drafts_' + M.draftUid, uid: M.draftUid, uidvalidity: 7, draftId: args.draftId, prior: args.prior ? 'replaced' : 'none' };
      }
      case 'send_mail':
        M.sent = M.sent || [];
        M.sent.push(args.draft);
        if (M.sendFails) throw 'smtp.example.com refused the message';
        return { via: 'smtp.example.com', messageId: 'rata' + M.sent.length + '@example.com' };
      case 'change_messages':
        if (M.changeFails) return { ok: false, done: [], gone: [], kind: 'net', error: 'imap.example.com could not be reached' };
        return { ok: true, done: args.uids, gone: [] };
      default: throw 'unmocked ' + cmd;
    }
  } } };
  localStorage.setItem('centra_session', JSON.stringify({ uid: 'local_t', email: 'me@example.com', mode: 'local' }));
};

const browser = await chromium.launch();

/* The app's own content security policy, from its config, applied to every
   page here the way the packaged app applies it — so these checks test the
   interface under the rules it actually ships with. */
import { readFileSync as _read } from 'node:fs';
const APP_CSP = JSON.parse(_read(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8')).app.security.csp;
async function open(licensed, opts = {}) {
  const page = await browser.newPage();
  await page.route('**/app.html', async (route) => {
    const resp = await route.fetch();
    await route.fulfill({ response: resp, headers: { ...resp.headers(), 'content-security-policy': APP_CSP } });
  });
  page.on('pageerror', (e) => { console.log('  PAGE ERROR: ' + e.message); fails++; });
  page.on('dialog', (d) => d.accept());
  /* What mailrata.org answers a renewal with (bridge.js renew()). */
  if (opts.renew) {
    page.__renewals = 0;
    await page.route('https://mailrata.org/api/licence/renew', (route) => {
      const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': 'content-type', 'access-control-allow-methods': 'POST' };
      if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: cors });
      page.__renewals++;
      return route.fulfill({ status: opts.renew.status || 200, headers: cors, contentType: 'application/json', body: JSON.stringify(opts.renew.body) });
    });
  }
  await page.addInitScript(MOCK, { licensed, ms: !!opts.ms, old: !!opts.old });
  await page.goto(B + '/app.html');
  await page.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function', null, { timeout: 20000 });
  /* A licensed app refreshes by itself a moment after start (0.1.27); let
     that finish so it does not land in the middle of a check. */
  if (licensed) await page.waitForFunction(() => lastSync > 0 && !SYNCING, null, { timeout: 10000 }).catch(() => {});
  await page.evaluate(() => {
    window.__toasts = [];
    const t = toast;
    toast = (m) => { window.__toasts.push(String(m)); return t(m); };
  });
  return page;
}
const row = (page) => page.evaluate(() => S.linked.find((l) => l.type === 'mail' && l.label === 'me@example.com') || null);
const toasts = (page) => page.evaluate(() => window.__toasts.splice(0));


console.log('\n— new mail in the background says so on the desktop —');
{
  const pg = await open(true);
  let uid = 60;
  const mk = (folder, extra) => { uid++; return Object.assign({ id: 'me@example.com_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com', subject: 'Subject ' + uid, preview: 'p', body: 'b',
    ts: Date.now() - (100 - uid) * 1000, unread: true, starred: false, uid, uidvalidity: 7, message_id: 'n' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {}); };
  const arrive = (msgs, quiet = true) => pg.evaluate(async ({ msgs, quiet }) => {
    __mock.notified = []; __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail', quiet); return __mock.notified;
  }, { msgs, quiet });
  await pg.evaluate(() => { document.hasFocus = () => false; });
  let n = await arrive([mk('inbox', { from_name: 'Ann', subject: 'Lunch?' })]);
  check(n.length === 1 && n[0].title === 'Ann' && n[0].body === 'Lunch?', `one new message names its sender and subject: ${JSON.stringify(n)}`);
  n = await arrive([mk('inbox', { from_name: 'Ann' }), mk('inbox', { from_name: 'Bo Li', from_addr: 'bo@example.org' }), mk('junk', { from_name: 'Prize desk', subject: 'You won' })]);
  check(n.length === 1 && n[0].title === '2 new messages' && n[0].body === 'From Bo Li, Ann', `several say how many and from whom, newest first — spam is not counted: ${JSON.stringify(n)}`);
  await pg.evaluate(() => { S.settings.notify = 'count'; });
  n = await arrive([mk('inbox', { subject: 'Private matter' })]);
  check(n.length === 1 && n[0].title === 'RATA' && n[0].body === '1 new message', `"Only say new mail arrived" names nobody: ${JSON.stringify(n)}`);
  await pg.evaluate(() => { S.settings.notify = 'off'; });
  check((await arrive([mk('inbox')])).length === 0, 'Off means off');
  await pg.evaluate(() => { S.settings.notify = 'full'; document.hasFocus = () => true; });
  check((await arrive([mk('inbox')])).length === 0, 'while RATA is in front, the toast is enough');
  await pg.evaluate(() => { document.hasFocus = () => false; });
  check((await arrive([mk('inbox')], false)).length === 0, 'a refresh the customer asked for does not notify');
  await pg.evaluate(() => go('set'));
  const row = await pg.evaluate(() => ({ shown: getComputedStyle(document.querySelector('#notify-row')).display !== 'none', value: document.querySelector('#set-notify').value }));
  check(row.shown && row.value === 'full', `Settings offers the choice, sender and subject by default: ${JSON.stringify(row)}`);
  await pg.close();
}

console.log('\n— several messages archived, moved home, or filed at once —');
{
  const pg = await open(true);
  const mk = (who, folder, uid, extra) => Object.assign({ id: who + '_' + (folder === 'inbox' ? '' : (folder.named ? 'fwork' : folder) + '_') + uid, folder, acct: who, acct_label: who,
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: 'Bo', to_addr: 'bo@example.org', subject: 'S' + uid, preview: 'p', body: 'b',
    ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'b' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate((msgs) => {
    __mock.mailboxes.push({ email: 'work@example.net', host: 'imap.example.net', port: 993, label: 'Work' });
    __mock.folders = [{ name: 'Receipts', label: 'Receipts' }, { name: 'Travel', label: 'Travel' }];
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
  }, [mk('me@example.com', 'inbox', 1), mk('me@example.com', 'inbox', 2), mk('me@example.com', 'sent', 3), mk('me@example.com', 'junk', 4), mk('me@example.com', 'archive', 5), mk('work@example.net', 'inbox', 6)]);
  await pg.evaluate(() => serverSync('mail'));
  const pick = (ids) => pg.evaluate((ids) => { go('inbox'); selecting = true; SEL.clear(); ids.forEach((i) => SEL.add(i)); bulkRefresh(); __mock.calls = []; window.__toasts = []; }, ids);
  const acts = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'change_messages').map(([, a]) => ({ email: a.email, folder: a.folder, uids: a.uids, action: a.action })));
  const has = (id) => pg.evaluate((id) => S.messages.some((m) => m.id === id), id);
  check(await pg.evaluate(() => [...document.querySelectorAll('#bulk-bar .native-only')].every((e) => e.style.display !== 'none')), 'the selection bar offers Archive, Move to inbox and Move to in the app');

  await pick(['me@example.com_1', 'me@example.com_2', 'me@example.com_sent_3']);
  await pg.click('#bulk-bar [data-bulk="archive"]');
  await pg.waitForTimeout(250);
  let a = await acts();
  check(a.length === 1 && a[0].action === 'archive' && a[0].uids.join() === '1,2' && a[0].folder === 'inbox', `Archive moves the inbox mail of a selection in one go: ${JSON.stringify(a)}`);
  check(!(await has('me@example.com_1')) && (await has('me@example.com_sent_3')) && /Archived — 2 \(1 could not be\)/.test((await toasts(pg))[0] || ''), 'sent mail in the selection stays, and the toast says so');

  await pick(['me@example.com_junk_4', 'me@example.com_archive_5']);
  await pg.click('#bulk-bar [data-bulk="inbox"]');
  await pg.waitForTimeout(250);
  a = await acts();
  check(a.length === 2 && a.every((x) => x.action === 'inbox') && a.map((x) => x.folder).sort().join() === 'archive,junk', `Move to inbox brings spam and archived mail home, each from its own folder: ${JSON.stringify(a)}`);

  await pg.evaluate((m) => { __mock.refresh = { messages: m, flags: [], problems: [], skipped: [] }; }, [mk('me@example.com', 'inbox', 7), mk('me@example.com', 'inbox', 8)]);
  await pg.evaluate(() => serverSync('mail'));
  await pick(['me@example.com_7', 'work@example.net_6']);
  await pg.click('#bulk-bar [data-bulk="file"]');
  await pg.waitForTimeout(250);
  check(/one mailbox/.test(await pg.textContent('#bulk-filemenu')), 'Move to asks for messages from one mailbox when a selection spans two');
  await pg.evaluate(() => document.querySelector('#bulk-filemenu').classList.remove('open'));
  await pick(['me@example.com_7', 'me@example.com_8']);
  await pg.click('#bulk-bar [data-bulk="file"]');
  await pg.waitForFunction(() => document.querySelectorAll('#bulk-filemenu [data-to]').length === 2, null, { timeout: 5000 }).catch(() => {});
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#bulk-filemenu [data-to="Travel"]');
  await pg.waitForTimeout(250);
  a = await acts();
  check(a.length === 1 && JSON.stringify(a[0].action) === '{"move":{"named":"Travel"}}' && a[0].uids.join() === '7,8' && a[0].email === 'me@example.com',
    `Move to files the whole selection into that folder on one connection: ${JSON.stringify(a)}`);
  check(!(await has('me@example.com_7')) && /Moved to Travel — 2/.test((await toasts(pg))[0] || ''), 'and it leaves the list at once');
  await pg.close();
}

console.log('\n— drafts begun elsewhere are finished here —');
{
  const pg = await open(true);
  const mk = (folder, uid, extra) => Object.assign({ id: 'me@example.com_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Me', from_addr: 'me@example.com', to_name: 'Bo Li', to_addr: 'bo@example.org', to_all: ['bo@example.org'], cc: [], in_reply_to: '',
    subject: 'S' + uid, preview: 'p', body: 'b', ts: Date.now() - uid * 1000, unread: true, starred: false, uid, uidvalidity: 7, message_id: 'd' + uid + '@example.com',
    reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  const d21 = mk('drafts', 21, { subject: 'Plan', body: 'Half a thought', to_all: ['bo@example.org', 'cy@example.org'], cc: ['dee@example.org', 'bo@example.org'], bcc: ['Boss@example.net', 'dee@example.org'], in_reply_to: 'orig@example.org' });
  const d22 = mk('drafts', 22, { subject: '(no subject)', body: '', to_all: [], to_addr: '', to_name: '', attachments: [{ index: 1, name: 'Q3 figures.pdf', mime: 'application/pdf', size: 245760 }] });
  const inbox = mk('inbox', 20, { from_name: 'Ann', from_addr: 'ann@example.org', subject: 'Hello' });
  await pg.evaluate(async (msgs) => {
    document.hasFocus = () => false; __mock.notified = [];
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [], drafts: [{ email: 'me@example.com', ids: ['me@example.com_drafts_21', 'me@example.com_drafts_22'] }] };
    await serverSync('mail', true);
  }, [d21, d22, inbox]);
  const st = await pg.evaluate(() => {
    go('inbox'); mailFilter = 'all'; renderMailFilters(); renderMail();
    const chips = [...document.querySelectorAll('#mail-filters .pill')].map((p) => p.dataset.f);
    const inboxIds = mailPool().map((m) => m.id);
    mailFilter = 'drafts'; renderMailFilters(); renderMail();
    const rows = [...document.querySelectorAll('#mail-scroll .mail-row .m-from')].map((e) => e.textContent);
    return { chips, inboxIds, rows, notified: __mock.notified.length, unread: S.messages.filter((m) => m.draft && m.unread).length,
      linked: S.messages.filter((m) => m.draft && (m.cid || m.toId)).length };
  });
  check(st.chips.includes('drafts') && st.inboxIds.join() === 'me@example.com_20', `drafts get their own chip and stay out of the inbox: ${JSON.stringify(st.chips)} ${JSON.stringify(st.inboxIds)}`);
  check(JSON.stringify(st.rows) === '["Draft to Bo Li","Draft"]', `a draft is listed by who it is for: ${JSON.stringify(st.rows)}`);
  check(st.notified === 1 && st.unread === 0 && st.linked === 0, `drafts never notify, count as unread or join a person's thread: ${JSON.stringify(st)}`);

  await pg.evaluate(() => openMail('me@example.com_drafts_21'));
  const pane = await pg.evaluate(() => ({ cont: !!document.querySelector('#md-continue'), reply: !!document.querySelector('#md-reply'), fwd: !!document.querySelector('#md-forward'),
    note: (document.querySelector('#mail-detail .md-note') || {}).textContent || '', to: document.querySelector('.md-fromtext span').textContent }));
  check(pane.cont && !pane.reply && !pane.fwd && /not sent/.test(pane.note) && /bo@example\.org, cy@example\.org, dee@example\.org/.test(pane.to),
    `a draft opens with Continue, says it is not sent and who it is for: ${JSON.stringify(pane)}`);
  await pg.click('#md-continue');
  await pg.waitForTimeout(200);
  const cmp = await pg.evaluate(() => ({ to: document.querySelector('#cmp-to').value, cc: document.querySelector('#cmp-cc').value, ccShown: !document.querySelector('#cmp-cc').hidden,
    bcc: document.querySelector('#cmp-bcc').value, bccShown: !document.querySelector('#cmp-bcc').hidden, subj: document.querySelector('#cmp-subj').value,
    body: document.querySelector('#cmp-body').value, title: document.querySelector('#cmp-title').textContent, from: document.querySelector('#cmp-from').value,
    want: (S.linked.find((l) => l.label === 'me@example.com') || {}).id }));
  check(cmp.to === 'bo@example.org, cy@example.org' && cmp.cc === 'dee@example.org' && cmp.ccShown && cmp.bcc === 'boss@example.net' && cmp.bccShown && cmp.subj === 'Plan'
    && cmp.body === 'Half a thought' && cmp.title === 'Draft reply' && cmp.from === cmp.want,
    `Continue puts everyone, the subject, the words and the mailbox back: ${JSON.stringify(cmp)}`);
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  let sent = await pg.evaluate(() => ({ send: __mock.calls.filter(([c]) => c === 'send_mail').map(([, a]) => a.draft),
    act: __mock.calls.filter(([c]) => c === 'change_messages').map(([, a]) => ({ folder: a.folder, uids: a.uids, action: a.action })),
    still: S.messages.some((m) => m.id === 'me@example.com_drafts_21') }));
  check(sent.send.length === 1 && sent.send[0].inReplyTo === 'orig@example.org' && sent.send[0].to === 'bo@example.org, cy@example.org' && sent.send[0].cc === 'dee@example.org'
    && sent.send[0].bcc === 'boss@example.net', `it is sent to everyone, copied as it was, in the thread it answers: ${JSON.stringify(sent.send)}`);
  check(sent.act.length === 1 && sent.act[0].folder === 'drafts' && sent.act[0].uids.join() === '21' && sent.act[0].action === 'trash' && !sent.still,
    `and once sent, the draft goes from Drafts to the Trash: ${JSON.stringify(sent.act)}`);

  await pg.evaluate(() => { __mock.calls = []; openMail('me@example.com_drafts_22'); });
  await pg.waitForTimeout(200);
  await pg.click('#md-continue');
  await pg.waitForTimeout(300);
  const c2 = await pg.evaluate(() => ({ opened: __mock.calls.filter(([c]) => c === 'open_message').map(([, a]) => a.folder), subj: document.querySelector('#cmp-subj').value,
    body: document.querySelector('#cmp-body').value, files: document.querySelector('#cmp-files').textContent, title: document.querySelector('#cmp-title').textContent }));
  check(c2.opened.includes('drafts') && c2.subj === '' && /The whole of message 22/.test(c2.body) && /Q3 figures\.pdf/.test(c2.files) && c2.title === 'Draft',
    `a draft with a file is fetched whole and keeps its file, with no "(no subject)": ${JSON.stringify(c2)}`);
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#cmp-discard');
  const kept = await pg.evaluate(() => ({ calls: __mock.calls.filter(([c]) => c === 'change_messages').length, has: S.messages.some((m) => m.id === 'me@example.com_drafts_22'),
    title: document.querySelector('#cmp-title').textContent, files: document.querySelector('#cmp-files').textContent,
    hidden: ['#cmp-cc', '#cmp-bcc'].every((q) => document.querySelector(q).hidden) }));
  check(kept.calls === 0 && kept.has && kept.title === 'New message' && kept.files === '' && kept.hidden, `Discard while finishing a draft leaves the draft where it is: ${JSON.stringify(kept)}`);
  await pg.evaluate(() => closeCompose());
  await pg.evaluate(() => { __mock.calls = []; openMail('me@example.com_drafts_22'); });
  await pg.click('#md-continue');
  await pg.waitForTimeout(300);
  await pg.fill('#cmp-to', 'bo@example.org');
  await pg.fill('#cmp-subj', 'Figures');
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  sent = await pg.evaluate(() => __mock.calls.filter(([c]) => c === 'send_mail').map(([, a]) => a.draft.forward));
  check(sent.length === 1 && sent[0] && sent[0].folder === 'drafts' && sent[0].uid === 22 && sent[0].indexes.join() === '1,2',
    `its attachments are fetched from Drafts at send time: ${JSON.stringify(sent)}`);

  await pg.evaluate(async (msgs) => { __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [], drafts: [{ email: 'me@example.com', ids: ['me@example.com_drafts_23', 'me@example.com_drafts_24'] }] }; await serverSync('mail', true); },
    [mk('drafts', 23, { subject: 'First try' }), mk('drafts', 24, { subject: 'Second' })]);
  await pg.evaluate(async () => { __mock.refresh = { messages: [], flags: [], problems: [], skipped: [] }; await serverSync('mail', true); });
  const before = await pg.evaluate(() => S.messages.filter((m) => m.draft).map((m) => m.id).sort());
  await pg.evaluate(async () => { openMail('me@example.com_drafts_23'); __mock.refresh = { messages: [], flags: [], problems: [], skipped: [], drafts: [{ email: 'me@example.com', ids: ['me@example.com_drafts_24'] }] }; await serverSync('mail', true); });
  const after = await pg.evaluate(() => ({ ids: S.messages.filter((m) => m.draft).map((m) => m.id), gone: !!(S.gone || {})['me@example.com_drafts_23'], open: document.querySelector('#mail-detail').classList.contains('open'), sel: selMail }));
  check(before.join() === 'me@example.com_drafts_23,me@example.com_drafts_24', `a refresh without the Drafts list removes nothing: ${JSON.stringify(before)}`);
  check(after.ids.join() === 'me@example.com_drafts_24' && !after.gone && !after.open && after.sel === null, `a draft no longer in Drafts leaves, and its open pane closes: ${JSON.stringify(after)}`);
  await pg.close();
}

console.log('\n— drafts written here are saved to the mailbox (F1) —');
{
  const pg = await open(true);
  const dialogs = [];
  pg.on('dialog', (d) => dialogs.push(d.message()));
  const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
  const saves = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'save_draft').map(([, a]) => a));
  const acts = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'change_messages').map(([, a]) => ({ folder: a.folder, uids: a.uids, action: a.action })));
  const drafts = () => pg.evaluate(() => S.messages.filter((m) => m.draft).map((m) => m.id).sort());
  const reset = () => pg.evaluate(() => { __mock.calls = []; window.__toasts = []; });
  const settle = () => pg.waitForTimeout(300);
  /* A long interval while the close checks run, so the timer stays out of them. */
  await pg.evaluate(() => { window.__RATA_DRAFT_EVERY = 60e3; newDraft(); openCompose(); });
  await pg.fill('#cmp-to', 'bo@example.org');
  await pg.fill('#cmp-subj', 'Plan');
  await pg.fill('#cmp-body', 'Half a thought');
  await reset();
  await pg.click('#cmp-close');
  await settle();
  let s = await saves();
  let t = await toasts(pg);
  check(s.length === 1 && s[0].draft.from === 'me@example.com' && s[0].draft.to === 'bo@example.org' && s[0].draft.subject === 'Plan' && s[0].draft.body === 'Half a thought'
    && UUID.test(s[0].draftId) && s[0].rev === 1 && s[0].prior === null, `closing a draft with something in it saves it to Drafts once: ${JSON.stringify(s)}`);
  check(t.filter((x) => x === 'Saved to Drafts').length === 1, `and says so once: ${JSON.stringify(t)}`);
  const id1 = s[0].draftId;
  const listed = await pg.evaluate(() => {
    go('inbox'); mailFilter = 'drafts'; renderMailFilters(); renderMail();
    const m = S.messages.find((x) => x.id === 'me@example.com_drafts_101');
    return { rows: [...document.querySelectorAll('#mail-scroll .mail-row .m-from')].map((e) => e.textContent), m: m && { draft: m.draft, uid: m.uid, draftId: m.draftId, subj: m.subj } };
  });
  check(listed.m && listed.m.draft && listed.m.uid === 101 && listed.m.draftId === id1 && listed.rows.includes('Draft to bo@example.org'),
    `it is listed under Drafts as the mailbox now has it: ${JSON.stringify(listed)}`);

  await reset();
  await pg.click('#compose-btn');
  await pg.click('#cmp-close');
  await settle();
  check((await saves()).length === 0 && (await toasts(pg)).length === 0, 'closing again with nothing new saves nothing and says nothing');

  await pg.click('#compose-btn');
  await pg.fill('#cmp-body', 'Half a thought, and the rest of it');
  await reset();
  await pg.click('#cmp-close');
  await settle();
  s = await saves();
  let d = await drafts();
  check(s.length === 1 && s[0].draftId === id1 && s[0].rev === 2 && s[0].prior && s[0].prior.uid === 101 && s[0].prior.uidvalidity === 7 && s[0].prior.draft_id === id1,
    `saving again names the copy before, to replace it: ${JSON.stringify(s)}`);
  check(d.join() === 'me@example.com_drafts_102' && !(await pg.evaluate(() => !!(S.gone || {})['me@example.com_drafts_101'])),
    `the copy before leaves the list, not as a deletion: ${JSON.stringify(d)}`);

  /* A refresh does not count RATA's own save as read yet, and takes it in as
     the same draft when it comes back. */
  const known = await pg.evaluate(async () => {
    __mock.calls = [];
    __mock.refresh = { messages: [{ id: 'me@example.com_drafts_102', folder: 'drafts', acct: 'me@example.com', acct_label: 'Example', from_name: '', from_addr: 'me@example.com',
      to_name: 'bo@example.org', to_addr: 'bo@example.org', to_all: ['bo@example.org'], cc: [], bcc: [], in_reply_to: '', subject: 'Plan', preview: 'p', body: 'Half a thought, and the rest of it',
      ts: Date.now(), unread: false, starred: false, uid: 102, uidvalidity: 7, message_id: '', reply_to: '', truncated: false, attachments: [], html: false, draft_id: S.messages.find((m) => m.uid === 102).draftId }],
      flags: [], problems: [], skipped: [], drafts: [{ email: 'me@example.com', ids: ['me@example.com_drafts_102'] }] };
    await serverSync('mail', true);
    const k = __mock.calls.find(([c]) => c === 'refresh_mail')[1].known || [];
    const m = S.messages.filter((x) => x.id === 'me@example.com_drafts_102');
    return { drafts: k.filter((x) => x.folder === 'drafts'), n: m.length, own: m[0] && !!m[0].ownDraft };
  });
  check(known.drafts.length === 0 && known.n === 1 && !known.own, `a refresh asks for it, and it stays one draft: ${JSON.stringify(known)}`);

  /* Every two minutes while it is being written, when it has changed. */
  await pg.evaluate(() => { window.__RATA_DRAFT_EVERY = 250; });
  await pg.click('#compose-btn');
  await reset();
  await pg.waitForTimeout(700);
  check((await saves()).length === 0, 'the timer saves nothing that has not changed');
  await pg.fill('#cmp-body', 'Half a thought, and the rest of it. And a PS.');
  await pg.waitForTimeout(700);
  s = await saves();
  t = await toasts(pg);
  check(s.length === 1 && s[0].prior && s[0].prior.uid === 102 && s[0].draft.body.endsWith('PS.') && t.length === 0,
    `while writing, the timer saves the change once, without a word: ${JSON.stringify({ s: s.length, t })}`);

  /* Sent: the copy in Drafts goes to the Trash. */
  await reset();
  await pg.click('#cmp-send');
  await pg.waitForTimeout(400);
  let a = await acts();
  d = await drafts();
  check((await pg.evaluate(() => __mock.calls.filter(([c]) => c === 'send_mail').length)) === 1 && a.length === 1 && a[0].folder === 'drafts' && a[0].uids.join() === '103' && a[0].action === 'trash' && d.length === 0,
    `once it is sent, its copy in Drafts goes to the Trash: ${JSON.stringify({ a, d })}`);
  check((await saves()).length === 0, 'and nothing is saved after the send');

  /* Discarded: the copy in Drafts goes to the Trash too. */
  await pg.evaluate(() => { window.__RATA_DRAFT_EVERY = 60e3; newDraft(); openCompose(); });
  await pg.fill('#cmp-to', 'cy@example.org');
  await pg.fill('#cmp-subj', 'Maybe');
  await pg.click('#cmp-close');
  await settle();
  await pg.click('#compose-btn');
  await reset();
  dialogs.length = 0;
  await pg.click('#cmp-discard');
  await settle();
  a = await acts();
  d = await drafts();
  check(/moved to the Trash/.test(dialogs[0] || '') && a.length === 1 && a[0].folder === 'drafts' && a[0].uids.join() === '104' && a[0].action === 'trash' && d.length === 0,
    `Discard asks, and moves RATA's copy to the Trash: ${JSON.stringify({ dialogs, a, d })}`);
  await pg.click('#cmp-close');

  /* Finishing a draft RATA saved (on this or another computer) saves over it. */
  const other = '6b1f0c2d-8e3a-4f5b-9c7d-0a1b2c3d4e5f';
  const mk = (uid, extra) => Object.assign({ id: 'me@example.com_drafts_' + uid, folder: 'drafts', acct: 'me@example.com', acct_label: 'Example', from_name: 'Me', from_addr: 'me@example.com',
    to_name: 'Bo Li', to_addr: 'bo@example.org', to_all: ['bo@example.org'], cc: [], bcc: [], in_reply_to: '', subject: 'S' + uid, preview: 'p', body: 'Draft ' + uid,
    ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 7, message_id: '', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate(async (msgs) => {
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [], drafts: [{ email: 'me@example.com', ids: msgs.map((m) => m.id) }] };
    await serverSync('mail', true);
  }, [mk(50, { draft_id: other }), mk(51)]);
  await pg.evaluate(() => openMail('me@example.com_drafts_50'));
  await pg.click('#md-continue');
  await pg.waitForTimeout(200);
  await pg.fill('#cmp-body', 'Draft 50, finished here');
  await reset();
  await pg.click('#cmp-close');
  await settle();
  s = await saves();
  a = await acts();
  d = await drafts();
  check(s.length === 1 && s[0].draftId === other && s[0].prior && s[0].prior.uid === 50 && s[0].prior.draft_id === other && a.length === 0 && !d.includes('me@example.com_drafts_50'),
    `continuing a draft RATA saved restores its reference, and the next save replaces it: ${JSON.stringify({ s, a, d })}`);

  /* A draft from another device, finished here: saved as RATA's own, and the
     one it came from goes to the Trash, not deleted for good. */
  await pg.evaluate(() => openMail('me@example.com_drafts_51'));
  await pg.click('#md-continue');
  await pg.waitForTimeout(200);
  await pg.fill('#cmp-body', 'Draft 51, finished here');
  await reset();
  await pg.click('#cmp-close');
  await settle();
  s = await saves();
  a = await acts();
  d = await drafts();
  check(s.length === 1 && UUID.test(s[0].draftId) && s[0].draftId !== other && s[0].prior === null && a.length === 1 && a[0].folder === 'drafts' && a[0].uids.join() === '51' && a[0].action === 'trash' && !d.includes('me@example.com_drafts_51'),
    `a draft from elsewhere is saved as RATA's own and its old copy goes to the Trash: ${JSON.stringify({ s, a, d })}`);

  /* A draft carrying files from its copy in Drafts: the save carries them,
     and from then on they come from the copy just saved, since the one they
     came from is gone. */
  await pg.evaluate(async (m) => {
    __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [], drafts: [{ email: 'me@example.com', ids: S.messages.filter((x) => x.draft).map((x) => x.id).concat([m.id]) }] };
    await serverSync('mail', true);
    openMail(m.id);
  }, mk(52, { attachments: [{ index: 1, name: 'Q3 figures.pdf', mime: 'application/pdf', size: 245760 }] }));
  await pg.waitForTimeout(200);
  await pg.click('#md-continue');
  await pg.waitForTimeout(300);
  await pg.fill('#cmp-body', 'Figures attached');
  await reset();
  await pg.click('#cmp-close');
  await settle();
  s = await saves();
  const fwdNow = await pg.evaluate(() => CMP_FWD && { folder: CMP_FWD.folder, uid: CMP_FWD.uid, i: CMP_FWD.atts.map((a) => a.i) });
  const newUid = await pg.evaluate(() => __mock.draftUid);
  check(s.length === 1 && s[0].draft.forward && s[0].draft.forward.folder === 'drafts' && s[0].draft.forward.uid === 52 && s[0].draft.forward.indexes.join() === '1,2'
    && fwdNow && fwdNow.folder === 'drafts' && fwdNow.uid === newUid && fwdNow.i.join() === '0,1',
    `a draft's files are carried into the saved copy, and taken from it after: ${JSON.stringify({ fwd: s[0] && s[0].draft.forward, fwdNow })}`);
  await pg.click('#compose-btn');
  await reset();
  await pg.click('#cmp-send');
  await pg.waitForTimeout(400);
  const sentFwd = await pg.evaluate(() => __mock.calls.filter(([c]) => c === 'send_mail').map(([, a]) => a.draft.forward)[0]);
  check(sentFwd && sentFwd.uid === newUid && sentFwd.indexes.join() === '0,1', `and sent from there: ${JSON.stringify(sentFwd)}`);

  /* No Drafts folder: kept here, said once, never asked again this session. */
  await pg.evaluate(() => { __mock.draftNoPlace = true; newDraft(); openCompose(); });
  await pg.fill('#cmp-subj', 'Nowhere to put it');
  await reset();
  await pg.click('#cmp-close');
  await settle();
  s = await saves();
  t = await toasts(pg);
  await pg.evaluate(() => { window.__RATA_DRAFT_EVERY = 200; });
  await pg.click('#compose-btn');
  await pg.fill('#cmp-body', 'Still nowhere');
  await pg.waitForTimeout(600);
  await pg.click('#cmp-close');
  await settle();
  const again = await saves();
  check(s.length === 1 && t.some((x) => /no Drafts folder/.test(x)) && again.length === 1,
    `a mailbox with no Drafts folder is asked once, and never again by the timer or a close: ${JSON.stringify({ n: again.length, t })}`);
  await pg.close();
}

console.log('\n— Cc, Bcc, and replying to everyone —');
{
  const pg = await open(true);
  const mk = (uid, extra) => Object.assign({ id: 'me@example.com_' + uid, folder: 'inbox', acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: 'Me', to_addr: 'me@example.com', to_all: ['me@example.com'], cc: [], in_reply_to: '',
    subject: 'Plans ' + uid, preview: 'p', body: 'Shall we?', ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 7,
    message_id: 'r' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate(async (msgs) => {
    __mock.mailboxes.push({ email: 'work@example.net', host: 'imap.example.net', port: 993, label: 'Work' });
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail');
    S.messages.push({ id: 'old-1', ch: 'email', prov: 'imap', acct: S.messages[0].acct, mailbox: 'me@example.com', fromName: 'Ann', fromAddr: 'ann@example.org', subj: 'Stored before', prev: 'p', body: 'b', ts: 1, unread: false, starred: false, atts: [], uid: 1, uidvalidity: 7 });
  }, [mk(31, { to_all: ['me@example.com', 'Xan@example.org'], cc: ['yu@example.org', 'work@example.net', 'ann@example.org'] }), mk(32), mk(33, { reply_to: 'list@example.org', to_all: ['list@example.org'] })]);
  const btn = (id) => pg.evaluate((id) => { openMail(id); return !!document.querySelector('#md-replyall'); }, id);
  check(await btn('me@example.com_31'), 'a message that went to others offers Reply all');
  check(!(await btn('me@example.com_32')) && !(await btn('old-1')), 'one that went only to me does not, nor mail stored before RATA knew who else it went to');
  await pg.evaluate(() => openMail('me@example.com_31'));
  await pg.click('#md-replyall');
  await pg.waitForTimeout(200);
  const c = await pg.evaluate(() => ({ to: document.querySelector('#cmp-to').value, cc: document.querySelector('#cmp-cc').value, shown: !document.querySelector('#cmp-cc').hidden,
    title: document.querySelector('#cmp-title').textContent, subj: document.querySelector('#cmp-subj').value }));
  check(c.to === 'ann@example.org' && c.cc === 'xan@example.org, yu@example.org' && c.shown && c.subj === 'Re: Plans 31' && c.title === 'Reply to Ann and 2 more',
    `Reply all copies everyone else, never my own mailboxes or the sender twice: ${JSON.stringify(c)}`);
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  const sent = await pg.evaluate(() => __mock.calls.filter(([x]) => x === 'send_mail').map(([, a]) => a.draft)[0]);
  check(sent && sent.to === 'ann@example.org' && sent.cc === 'xan@example.org, yu@example.org' && sent.inReplyTo === 'r31@example.org', `and it is sent that way, in the thread: ${JSON.stringify(sent)}`);
  await pg.evaluate(() => openMail('me@example.com_33'));
  await pg.click('#md-replyall');
  await pg.waitForTimeout(200);
  const l = await pg.evaluate(() => ({ to: document.querySelector('#cmp-to').value, cc: document.querySelector('#cmp-cc').value }));
  check(l.to === 'list@example.org' && l.cc === 'ann@example.org', `to a list, the list is To and the sender is copied: ${JSON.stringify(l)}`);
  await pg.evaluate(() => { newDraft(); openCompose(); });
  const hidden = await pg.evaluate(() => document.querySelector('#cmp-cc').hidden && !document.querySelector('#cmp-cc-btn').hidden);
  await pg.click('#cmp-cc-btn');
  await pg.fill('#cmp-to', 'bo@example.org');
  await pg.fill('#cmp-cc', 'cy@example.org');
  await pg.click('#cmp-bcc-btn');
  await pg.fill('#cmp-bcc', 'boss@example.net');
  await pg.fill('#cmp-subj', 'Hi');
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  const hand = await pg.evaluate(() => __mock.calls.filter(([x]) => x === 'send_mail').map(([, a]) => a.draft)[0]);
  check(hidden && hand && hand.to === 'bo@example.org' && hand.cc === 'cy@example.org' && hand.bcc === 'boss@example.net', `Cc and Bcc are out of the way until asked for, then sent: ${JSON.stringify(hand)}`);
  check(await pg.evaluate(() => { newDraft(); return ['#cmp-cc', '#cmp-bcc'].every((q) => document.querySelector(q).value === '' && document.querySelector(q).hidden); }), 'and a new message starts without either');
  await pg.close();
}

console.log('\n— new mail as it arrives —');
{
  const pg = await open(true);
  let uid = 400;
  const mk = () => { uid++; return { id: 'me@example.com_' + uid, folder: 'inbox', acct: 'me@example.com', acct_label: 'Example', from_name: 'Ann', from_addr: 'ann@example.org',
    to_name: '', to_addr: 'me@example.com', to_all: ['me@example.com'], cc: [], subject: 'Pushed ' + uid, preview: 'p', body: 'b', ts: Date.now(), unread: true, starred: false,
    uid, uidvalidity: 7, message_id: 'w' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }; };
  const refreshes = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'refresh_mail').length);
  await pg.evaluate((m) => { document.hasFocus = () => false; __mock.calls = []; __mock.notified = []; __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [] }; }, mk());
  await pg.evaluate(() => { window.__rataMail({ email: 'me@example.com' }); window.__rataMail({ email: 'me@example.com' }); window.__rataMail({ email: 'me@example.com' }); });
  await pg.waitForTimeout(400);
  check((await refreshes()) === 0, 'a wake waits a moment, so a burst of mail is one refresh');
  await pg.waitForFunction(() => __mock.calls.some(([c]) => c === 'refresh_mail') && !SYNCING, null, { timeout: 5000 }).catch(() => {});
  await pg.waitForTimeout(300);
  const one = await pg.evaluate(() => ({ n: __mock.calls.filter(([c]) => c === 'refresh_mail').length, has: S.messages.some((m) => m.subj && m.subj.startsWith('Pushed')), notified: __mock.notified.length }));
  check(one.n === 1 && one.has && one.notified === 1, `then one quiet refresh brings the mail and notifies as the timer's would: ${JSON.stringify(one)}`);
  await pg.evaluate((m) => { __mock.calls = []; __mock.refreshDelay = 900; __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [] }; serverSync('mail', true); }, mk());
  await pg.waitForTimeout(100);
  await pg.evaluate(() => window.__rataMail({ email: 'me@example.com' }));
  await pg.waitForFunction(() => __mock.calls.filter(([c]) => c === 'refresh_mail').length >= 2 && !SYNCING, null, { timeout: 8000 }).catch(() => {});
  await pg.waitForTimeout(1500);
  check((await refreshes()) === 2, `a wake during a refresh gets exactly one refresh of its own after it: ${await refreshes()}`);
  await pg.evaluate(() => { __mock.refreshDelay = 0; });
  await pg.close();
  const un = await open(false);
  await un.evaluate(() => { __mock.calls = []; window.__rataMail({ email: 'me@example.com' }); });
  await un.waitForTimeout(1800);
  check(await un.evaluate(() => !__mock.calls.some(([c]) => c === 'refresh_mail')), 'without a licence a wake fetches nothing');
  await un.close();
}

console.log('\n— fewer sign-ins: only the mailbox that woke, only what is due —');
{
  const pg = await open(true);
  const onlyOf = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'refresh_mail').map(([, a]) => a.only));
  await pg.evaluate(async () => {
    __mock.mailboxes.push({ email: 'work@example.net', host: 'imap.example.net', port: 993, label: 'Work' });
    __mock.refresh = null; await serverSync('mail');
  });
  const workStamp = () => pg.evaluate(() => (S.linked.find((l) => l.label === 'work@example.net') || {}).lastSync || 0);
  const before = await workStamp();
  await pg.evaluate(() => { __mock.calls = []; window.__rataMail({ email: 'Me@example.com' }); });
  await pg.waitForFunction(() => __mock.calls.some(([c]) => c === 'refresh_mail') && !SYNCING, null, { timeout: 5000 }).catch(() => {});
  let o = await onlyOf();
  check(JSON.stringify(o) === '[["me@example.com"]]', `new mail in one mailbox reads that mailbox only: ${JSON.stringify(o)}`);
  check((await workStamp()) === before, 'and the other is not marked as synced when it was not asked');
  /* The timer: a live mailbox waits half an hour, the other five minutes. */
  await pg.evaluate(() => { const t = Date.now() - 6 * 60e3; LAST_TRY.set('me@example.com', t); LAST_TRY.set('work@example.net', t); __mock.live = ['me@example.com']; __mock.calls = []; });
  await pg.evaluate(() => autoTick());
  o = await onlyOf();
  check(JSON.stringify(o) === '[["work@example.net"]]', `after five minutes the timer reads only the mailbox without a live connection: ${JSON.stringify(o)}`);
  await pg.evaluate(() => { const t = Date.now() - 31 * 60e3; LAST_TRY.set('me@example.com', t); LAST_TRY.set('work@example.net', t); __mock.calls = []; });
  await pg.evaluate(() => autoTick());
  o = await onlyOf();
  check(o.length === 1 && o[0] == null, `after half an hour both are due, and one refresh reads every mailbox: ${JSON.stringify(o)}`);
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.evaluate(() => autoTick());
  check((await onlyOf()).length === 0, 'and nothing is due straight after');
  /* Coming back to the window reads everything, even just after a wake. */
  await pg.evaluate(async () => { __mock.calls = []; lastFull = Date.now() - 2 * 60e3; await serverSync('mail', true, ['me@example.com']); __mock.calls = []; window.dispatchEvent(new Event('focus')); });
  await pg.waitForFunction(() => __mock.calls.some(([c]) => c === 'refresh_mail') && !SYNCING, null, { timeout: 5000 }).catch(() => {});
  o = await onlyOf();
  check(o.length === 1 && o[0] == null, `coming back to RATA reads every mailbox, whatever woke just before: ${JSON.stringify(o)}`);
  /* A mailbox's own Sync now. */
  await pg.evaluate(() => { go('set'); renderLinked(); __mock.calls = []; });
  const id = await pg.evaluate(() => S.linked.find((l) => l.label === 'work@example.net').id);
  await pg.evaluate((id) => document.querySelector(`[data-lksync="${id}"]`).click(), id);
  await pg.waitForFunction(() => __mock.calls.some(([c]) => c === 'refresh_mail') && !SYNCING, null, { timeout: 5000 }).catch(() => {});
  o = await onlyOf();
  check(JSON.stringify(o) === '[["work@example.net"]]', `a mailbox's Sync now reads that mailbox: ${JSON.stringify(o)}`);
  await pg.close();
}

console.log('\n— signatures —');
{
  const pg = await open(true);
  await pg.evaluate(async (m) => {
    __mock.mailboxes.push({ email: 'work@example.net', host: 'imap.example.net', port: 993, label: 'Work' });
    __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [] }; await serverSync('mail');
  }, { id: 'me@example.com_71', folder: 'inbox', acct: 'me@example.com', acct_label: 'Example', from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com',
    to_all: ['me@example.com'], cc: [], subject: 'Lunch', preview: 'p', body: 'Lunch on Friday?', ts: Date.now(), unread: false, starred: false, uid: 71, uidvalidity: 7,
    message_id: 's71@example.org', reply_to: '', truncated: false, attachments: [], html: false, bcc: [] });
  await pg.evaluate(() => { go('set'); renderSettings(); });
  const rows = await pg.evaluate(() => [...document.querySelectorAll('#sig-list [data-sig]')].map((t) => t.dataset.sig));
  check(await pg.evaluate(() => getComputedStyle(document.querySelector('#sig-sec')).display !== 'none') && rows.join() === 'me@example.com,work@example.net',
    `Settings has a signature for each mailbox: ${JSON.stringify(rows)}`);
  await pg.fill('[data-sig="me@example.com"]', 'Me Smith\nACME, Accounts');
  await pg.waitForTimeout(600);
  check(await pg.evaluate(() => S.settings.sigs['me@example.com'] === 'Me Smith\nACME, Accounts'), 'what is typed is kept');
  const cloud = await pg.evaluate(() => JSON.stringify(forCloud(S).settings || {}));
  check(!cloud.includes('Me Smith') && !cloud.includes('sigs'), `and never leaves this computer with the settings that sync: ${cloud.slice(0, 120)}`);
  const ids = await pg.evaluate(() => ({ me: S.linked.find((l) => l.label === 'me@example.com').id, work: S.linked.find((l) => l.label === 'work@example.net').id }));
  await pg.evaluate((id) => { newDraft(); openCompose(); document.querySelector('#cmp-from').value = id; document.querySelector('#cmp-from').dispatchEvent(new Event('change')); }, ids.me);
  let c = await pg.evaluate(() => ({ body: document.querySelector('#cmp-body').value, discard: document.querySelector('#cmp-discard').hidden }));
  check(c.body === '\n\n-- \nMe Smith\nACME, Accounts' && c.discard, `a new message starts with the signature, and counts as empty: ${JSON.stringify(c)}`);
  await pg.evaluate(() => { S.settings.sigs['work@example.net'] = 'Work me'; });
  await pg.evaluate((id) => { const f = document.querySelector('#cmp-from'); f.value = id; f.dispatchEvent(new Event('change')); }, ids.work);
  c = await pg.evaluate(() => document.querySelector('#cmp-body').value);
  check(c === '\n\n-- \nWork me', `changing From swaps it for that mailbox's: ${JSON.stringify(c)}`);
  await pg.evaluate(() => { delete S.settings.sigs['work@example.net']; });
  await pg.evaluate((id) => { const f = document.querySelector('#cmp-from'); f.value = id; f.dispatchEvent(new Event('change')); }, ids.me);
  await pg.evaluate(() => { document.querySelector('#cmp-body').value = 'Hi Bo,\n\nSee you.' + document.querySelector('#cmp-body').value; });
  await pg.evaluate((id) => { const f = document.querySelector('#cmp-from'); f.value = id; f.dispatchEvent(new Event('change')); }, ids.work);
  c = await pg.evaluate(() => document.querySelector('#cmp-body').value);
  check(c === 'Hi Bo,\n\nSee you.', `to a mailbox without one, it goes, and the words stay: ${JSON.stringify(c)}`);
  await pg.evaluate(() => { closeCompose(); go('inbox'); openMail('me@example.com_71'); });
  await pg.click('#md-reply');
  await pg.waitForTimeout(200);
  c = await pg.evaluate(() => document.querySelector('#cmp-body').value);
  check(/^\n\n-- \nMe Smith\nACME, Accounts\n\nOn .*, Ann wrote:\n> Lunch on Friday\?$/.test(c), `a reply has it above the quote: ${JSON.stringify(c)}`);
  await pg.evaluate(() => { closeCompose(); go('inbox'); openMail('me@example.com_71'); });
  await pg.click('#md-forward');
  await pg.waitForTimeout(300);
  c = await pg.evaluate(() => document.querySelector('#cmp-body').value);
  check(c.startsWith('\n\n-- \nMe Smith\nACME, Accounts\n\n---------- Forwarded message'), `so does a forward: ${JSON.stringify(c.slice(0, 80))}`);
  await pg.evaluate(() => { S.settings.sigReplies = false; closeCompose(); go('inbox'); openMail('me@example.com_71'); });
  await pg.click('#md-reply');
  await pg.waitForTimeout(200);
  c = await pg.evaluate(() => document.querySelector('#cmp-body').value);
  check(/^\n\nOn .*, Ann wrote:/.test(c), `with "In replies and forwards too" off, a reply has none: ${JSON.stringify(c.slice(0, 40))}`);
  c = await pg.evaluate(() => { closeCompose(); newDraft(); openCompose(); return document.querySelector('#cmp-body').value; });
  check(c === '\n\n-- \nMe Smith\nACME, Accounts', `but a new message still does: ${JSON.stringify(c)}`);
  await pg.close();
}

console.log('\n— Gmail\'s archive: mail archived later arrives, mail moved out leaves —');
{
  const pg = await open(true);
  const mk = (uid, extra) => Object.assign({ id: 'me@example.com_archive_' + uid, folder: 'archive', acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com', subject: 'Archived ' + uid, preview: 'p', body: 'b',
    ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 9, message_id: 'a' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  const sync = (msgs, archives) => pg.evaluate(async ({ msgs, archives }) => {
    __mock.archReads = []; __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [], archives };
    await serverSync('mail', true);
    return { held: S.messages.filter((m) => folderOf(m) === 'archive').map((m) => m.uid).sort((a, b) => a - b), reads: __mock.archReads };
  }, { msgs, archives });
  let r = await sync([mk(40), mk(50)], [{ email: 'me@example.com', uidvalidity: 9, floor: 1, uids: [40, 50] }]);
  check(r.held.join() === '40,50' && r.reads.length === 0, `the archive arrives, and nothing more is fetched when all of it is held: ${JSON.stringify(r)}`);
  // Since then: 40 went back to the inbox elsewhere, 45 (older than 50, so no
  // refresh of "newer" would find it) was archived, 30 is older than anything held.
  r = await sync([], [{ email: 'me@example.com', uidvalidity: 9, floor: 1, uids: [30, 45, 50] }]);
  check(r.held.join() === '45,50', `mail moved out of the archive leaves, and mail archived since arrives: ${JSON.stringify(r.held)}`);
  check(r.reads.length === 1 && r.reads[0].uids.join() === '45' && r.reads[0].uidvalidity === 9 && r.reads[0].email === 'me@example.com',
    `only what RATA covers is fetched, not older mail Load older mail is for: ${JSON.stringify(r.reads)}`);
  // Below the listing's floor nothing is judged: it was not listed.
  await pg.evaluate(() => { const m = S.messages.find((x) => x.uid === 45); m.uid = 5; m.id = 'me@example.com_archive_5'; });
  r = await sync([], [{ email: 'me@example.com', uidvalidity: 9, floor: 20, uids: [45, 50] }]);
  check(r.held.includes(5), `a held message below the listing's floor stays: ${JSON.stringify(r.held)}`);
  // Another generation of All Mail is not this one.
  r = await sync([], [{ email: 'me@example.com', uidvalidity: 10, floor: 1, uids: [] }]);
  check(r.held.join() === '5,45,50' && r.reads.length === 0, `a listing under another UIDVALIDITY changes nothing: ${JSON.stringify(r)}`);
  const toast = (await toasts(pg)).filter((t) => /new message/.test(t));
  check(toast.length === 0, `archived mail never announces itself as new: ${JSON.stringify(toast)}`);
  await pg.close();
}

console.log('\n— Microsoft mailboxes are named as not supported, before a password is asked for —');
{
  const pg = await open(true);
  await pg.evaluate(() => { go('set'); openMailForm(); __mock.calls = []; });
  const form = () => pg.evaluate(() => ({
    note: $('#lf-ms').textContent,
    noteShown: getComputedStyle($('#lf-ms')).display !== 'none',
    passShown: getComputedStyle($('#lf-pass')).display !== 'none',
    canLink: !$('#lf-save').disabled && getComputedStyle($('#lf-save')).display !== 'none',
    goShown: getComputedStyle($('#lf-ms-go')).display !== 'none',
    links: __mock.calls.filter((c) => c[0] === 'link_mailbox').length,
  }));
  let f = await form();
  check(!f.noteShown && f.passShown && f.canLink, `an empty form asks for the password as before: ${JSON.stringify(f)}`);
  await pg.fill('#lf-input', 'someone@outlook.com');
  f = await form();
  check(f.noteShown && /cannot be added to RATA yet/.test(f.note) && /OAuth/.test(f.note) && !f.passShown && !f.canLink && !f.goShown,
    `in a build without Microsoft sign-in, an @outlook.com address says Microsoft is not supported yet, and why, with no password box and no sign-in button: ${JSON.stringify(f)}`);
  await pg.press('#lf-input', 'Enter');
  await pg.waitForTimeout(100);
  f = await form();
  check(f.links === 0, `Enter on it sends nothing to link: ${f.links} link calls`);
  await pg.fill('#lf-input', 'Someone@Hotmail.co.uk');
  f = await form();
  check(f.noteShown && !f.passShown, `a regional Hotmail address is Microsoft too: ${JSON.stringify(f)}`);
  await pg.fill('#lf-input', 'someone@gmail.com');
  f = await form();
  check(!f.noteShown && f.passShown && f.canLink, `changing to a Gmail address brings the password box back: ${JSON.stringify(f)}`);
  // A company domain is only known to be at Microsoft 365 by its DNS, which
  // the engine reads; it refuses before the password goes anywhere, and the
  // form shows its reason and drops the password it will not use.
  await pg.evaluate(() => { __mock.linkFails = 'That address’s mail is at Microsoft 365, which RATA cannot open yet. Microsoft only lets other apps into Microsoft 365 mailboxes through its own sign-in page (OAuth), and no longer accepts passwords for IMAP. RATA does not have that sign-in yet.'; });
  await pg.fill('#lf-input', 'ann@acme.example');
  await pg.fill('#lf-pass', 'abcd efgh ijkl mnop');
  await pg.click('#lf-save');
  await pg.waitForFunction(() => $('#lf-save').textContent === 'Link');
  f = await form();
  const pass = await pg.evaluate(() => $('#lf-pass').value);
  check(f.links === 1 && f.noteShown && /Microsoft 365/.test(f.note) && !f.passShown && pass === '' && !f.canLink,
    `the engine's Microsoft 365 refusal stays in the form, not a passing toast: ${JSON.stringify(f)}`);
  check(!(await pg.evaluate(() => S.linked.some((l) => l.label === 'ann@acme.example'))), 'and nothing is linked');
  await pg.click('#lf-cancel');
  // Relinking a Microsoft mailbox an older build linked says the same.
  await pg.evaluate(() => { __mock.linkFails = null; openMailForm('old@live.com', 'That mailbox refused the sign-in.'); });
  f = await form();
  check(f.noteShown && /OAuth/.test(f.note) && !f.passShown && !f.canLink, `Relink on an old @live.com mailbox says why instead of asking again: ${JSON.stringify(f)}`);
  await pg.close();
}

console.log('\n— Sign in with Microsoft (C2/C3) —');
{
  const pg = await open(true, { ms: true });
  await pg.evaluate(() => { go('set'); openMailForm(); __mock.calls = []; window.__toasts = []; });
  await pg.waitForFunction(() => MS_READY === true, null, { timeout: 5000 });
  const form = () => pg.evaluate(() => ({
    note: $('#lf-ms').textContent,
    noteShown: getComputedStyle($('#lf-ms')).display !== 'none',
    calm: $('#lf-ms').classList.contains('calm'),
    sub: $('#lf-sub').textContent,
    open: $('#linkform').style.display !== 'none',
    passShown: getComputedStyle($('#lf-pass')).display !== 'none',
    canLink: !$('#lf-save').disabled && getComputedStyle($('#lf-save')).display !== 'none',
    goShown: getComputedStyle($('#lf-ms-go')).display !== 'none',
    goClass: $('#lf-ms-go').className,
    goText: $('#lf-ms-go').textContent,
    inputOff: $('#lf-input').disabled,
    ms: __mock.calls.filter((c) => c[0] === 'link_microsoft').map((c) => c[1]),
    links: __mock.calls.filter((c) => c[0] === 'link_mailbox').length,
    cancels: __mock.calls.filter((c) => c[0] === 'cancel_microsoft').length,
  }));
  const sub = await pg.evaluate(() => $('#lf-sub').textContent);
  check(/sign in with Microsoft instead/.test(sub) && !/not supported yet/.test(sub), `the form says Microsoft mailboxes sign in with Microsoft, not that they are unsupported: "${sub.slice(-70)}"`);
  await pg.fill('#lf-input', 'someone@outlook.com');
  let f = await form();
  check(f.goShown && f.goText === 'Sign in with Microsoft' && !f.passShown && !f.canLink && f.noteShown && f.calm && /instead of an app password/.test(f.note),
    `an @outlook.com address shows Sign in with Microsoft in place of the password box: ${JSON.stringify(f)}`);
  check(f.goClass === 'btn' && !/—/.test(f.note), `a secondary button, and the words have no em dash: ${JSON.stringify({ c: f.goClass, n: f.note })}`);

  // Pressing it: the app signs in; the form waits for the browser.
  await pg.click('#lf-ms-go');
  await pg.waitForFunction(() => LF_WAIT === true && !!__mock.msPending, null, { timeout: 3000 });
  f = await form();
  check(f.ms.length === 1 && f.ms[0].email === 'someone@outlook.com' && /Finish signing in in your browser/.test(f.note) && !f.goShown && f.inputOff && f.open,
    `pressing it asks the app to sign in, and the form waits for the browser: ${JSON.stringify(f)}`);
  check(f.links === 0, 'and no password link is ever attempted');
  await pg.click('#lf-cancel');
  await pg.waitForFunction(() => !LF_WAIT, null, { timeout: 3000 });
  f = await form();
  const cancelToasts = (await toasts(pg)).filter((t) => /Microsoft|sign/i.test(t));
  check(f.cancels === 1 && !f.open && cancelToasts.length === 0 && !(await pg.evaluate(() => S.linked.some((l) => l.label === 'someone@outlook.com'))),
    `Cancel stops the listener and closes the form, with nothing linked and nothing to say: ${JSON.stringify({ c: f.cancels, open: f.open, t: cancelToasts })}`);

  // A sign-in that ends badly says why in the form, and can be tried again.
  await pg.evaluate(() => { openMailForm(); __mock.calls = []; });
  await pg.fill('#lf-input', 'someone@outlook.com');
  await pg.click('#lf-ms-go');
  await pg.waitForFunction(() => !!__mock.msPending, null, { timeout: 3000 });
  await pg.evaluate(() => __mock.msPending({ outcome: 'failed', error: 'You signed in to Microsoft as other@outlook.com, which cannot open someone@outlook.com. Sign in as someone@outlook.com, or add other@outlook.com instead.' }));
  await pg.waitForFunction(() => !LF_WAIT, null, { timeout: 3000 });
  f = await form();
  check(f.open && /You signed in to Microsoft as other@outlook.com/.test(f.sub) && f.goShown && !f.inputOff,
    `a failed sign-in is said in the form and the button comes back: ${JSON.stringify(f)}`);

  // Success adds the mailbox.
  await pg.evaluate(() => { __mock.calls = []; window.__toasts = []; });
  await pg.click('#lf-ms-go');
  await pg.waitForFunction(() => !!__mock.msPending, null, { timeout: 3000 });
  await pg.evaluate(() => {
    const mb = { email: 'someone@outlook.com', host: 'outlook.office365.com', port: 993, label: 'Outlook', source: 'microsoft', auth: 'oauth' };
    __mock.mailboxes.push(mb);
    __mock.msPending({ outcome: 'ok', mailbox: mb });
  });
  await pg.waitForFunction(() => S.linked.some((l) => l.label === 'someone@outlook.com' && l.status === 'live') && !SYNCING, null, { timeout: 5000 }).catch(() => {});
  const added = await pg.evaluate(() => {
    const l = S.linked.find((x) => x.label === 'someone@outlook.com');
    return { l, open: $('#linkform').style.display !== 'none', rows: $('#linked-list').textContent };
  });
  const okToast = (await toasts(pg)).find((t) => /linked with Microsoft/.test(t));
  check(added.l && added.l.auth === 'oauth' && !added.open && /signs in with Microsoft/.test(added.rows) && !!okToast,
    `a finished sign-in adds the mailbox, marked as signing in with Microsoft: ${JSON.stringify({ l: added.l, open: added.open, toast: okToast })}`);
  check(!JSON.stringify(await pg.evaluate(() => S.linked)).match(/token|EwB|refresh/i), 'nothing token-like reaches the page\'s state');

  // A company domain at Microsoft 365, known only by DNS, gets the button
  // before any password box.
  await pg.evaluate(() => { __mock.msDomains = ['contoso.example']; openMailForm(); __mock.calls = []; });
  await pg.fill('#lf-input', 'ann@contoso.example');
  await pg.waitForFunction(() => getComputedStyle($('#lf-ms-go')).display !== 'none', null, { timeout: 4000 }).catch(() => {});
  f = await form();
  const found = await pg.evaluate(() => __mock.calls.filter((c) => c[0] === 'discover_mailbox').map((c) => c[1].email));
  check(found.includes('ann@contoso.example') && f.goShown && !f.passShown, `a company domain the app finds at Microsoft 365 by DNS switches to the button: ${JSON.stringify({ found, f })}`);
  await pg.fill('#lf-input', 'ann@gmail.com');
  await pg.waitForTimeout(700);
  f = await form();
  check(!f.goShown && f.passShown && f.canLink, `an address that is not Microsoft's keeps the password box: ${JSON.stringify(f)}`);

  // A password typed before discovery answered: the engine refuses to send it
  // to Microsoft, and the form switches.
  await pg.evaluate(() => { __mock.linkMicrosoft = true; openMailForm(); __mock.calls = []; });
  await pg.fill('#lf-input', 'bo@fabrikam.example');
  await pg.fill('#lf-pass', 'abcd efgh ijkl mnop');
  await pg.click('#lf-save');
  await pg.waitForFunction(() => $('#lf-save').textContent === 'Link');
  f = await form();
  const passLeft = await pg.evaluate(() => $('#lf-pass').value);
  check(f.links === 1 && f.goShown && !f.passShown && passLeft === '' && f.calm, `the engine's "Microsoft takes no password" switches the form to the button and drops the password: ${JSON.stringify(f)}`);
  await pg.evaluate(() => { __mock.linkMicrosoft = false; $('#lf-cancel').click(); });

  // Microsoft stops accepting the sign-in: the mailbox is parked as "Sign in
  // to Microsoft again", never "app password", and the button reruns it.
  await pg.evaluate(async () => {
    window.__toasts = [];
    __mock.refresh = { messages: [], problems: [{ email: 'someone@outlook.com', kind: 'microsoft', error: 'Sign in to Microsoft again to keep reading someone@outlook.com. Microsoft said: AADSTS70008 (invalid_grant)' }], skipped: [] };
    await serverSync('mail');
    go('set'); renderLinked();
  });
  const parked = await pg.evaluate(() => {
    const l = S.linked.find((x) => x.label === 'someone@outlook.com');
    const b = document.querySelector(`[data-lkms="${l.id}"]`);
    const rowText = b ? b.closest('.set-row').textContent : '';
    return { needs: l.needsRelink, signIn: l.signIn, button: b ? b.textContent : null, cls: b ? b.className : null, rowText, relink: !!document.querySelector(`[data-lkfix="${l.id}"]`) };
  });
  const parkToast = (await toasts(pg)).join(' | ');
  check(parked.needs && parked.button === 'Sign in to Microsoft again' && /SIGN IN TO MICROSOFT AGAIN/.test(parked.rowText) && !/APP PASSWORD/.test(parked.rowText) && !parked.relink,
    `a refused sign-in shows "Sign in to Microsoft again" with its button, not "app password": ${JSON.stringify(parked)}`);
  check(/Sign in to Microsoft again for someone@outlook.com/.test(parkToast) && !/app password/.test(parkToast), `and the sync says so in those words: ${parkToast}`);
  await pg.evaluate(() => { __mock.calls = []; const l = S.linked.find((x) => x.label === 'someone@outlook.com'); document.querySelector(`[data-lkms="${l.id}"]`).click(); });
  await pg.waitForFunction(() => !!__mock.msPending, null, { timeout: 3000 }).catch(() => {});
  f = await form();
  check(f.open && f.ms.length === 1 && f.ms[0].email === 'someone@outlook.com' && /Finish signing in in your browser/.test(f.note),
    `its button reruns the sign-in for that mailbox, straight to the browser: ${JSON.stringify(f)}`);
  await pg.evaluate(() => {
    __mock.refresh = null;
    __mock.msPending({ outcome: 'ok', mailbox: { email: 'someone@outlook.com', host: 'outlook.office365.com', port: 993, label: 'Outlook', source: 'microsoft', auth: 'oauth' } });
  });
  await pg.waitForFunction(() => { const l = S.linked.find((x) => x.label === 'someone@outlook.com'); return l && !l.needsRelink && !SYNCING; }, null, { timeout: 5000 }).catch(() => {});
  const back = await pg.evaluate(() => { const l = S.linked.find((x) => x.label === 'someone@outlook.com'); return { needs: !!l.needsRelink, signIn: l.signIn || null, status: l.status }; });
  check(!back.needs && !back.signIn && back.status === 'live', `signing in again clears the parked state: ${JSON.stringify(back)}`);
  await pg.close();
}

console.log('\n— a program named to look like a document is labelled, and asked about before saving —');
{
  /* UI-1: invoice.pdf.exe shows as invoice.pdf in Windows. The engine marks
     it (`disguised`), the page labels it and asks, and Rust refuses to save it
     without `confirmed`. A document, or a program that says so, is not asked
     about. */
  const pg = await open(true);
  const atts = [
    { index: 1, name: 'invoice.pdf.exe', mime: 'application/octet-stream', size: 4, disguised: true },
    { index: 2, name: 'invoice.pdf', mime: 'application/pdf', size: 9, disguised: false },
    { index: 3, name: 'setup.exe', mime: 'application/octet-stream', size: 4, disguised: false },
  ];
  await pg.evaluate(async (atts) => {
    __mock.openAtts = atts; __mock.disguisedAt = [1];
    __mock.refresh = { messages: [{ id: 'me@example.com_90', folder: 'inbox', acct: 'me@example.com', acct_label: 'Example',
      from_name: 'Billing', from_addr: 'billing@example.org', to_name: '', to_addr: 'me@example.com', subject: 'Your invoice', preview: 'Please see the attached report.',
      body: 'Please see the attached report.', ts: Date.now(), unread: true, starred: false, uid: 90, uidvalidity: 7, message_id: 'inv90@example.org',
      reply_to: '', truncated: false, attachments: atts, html: false }], flags: [], problems: [], skipped: [] };
    await serverSync('mail', true);
    go('inbox'); openMail(S.messages.find((m) => m.uid === 90).id);
  }, atts);
  await pg.waitForFunction(() => __mock.calls.some(([c]) => c === 'open_message') && document.querySelectorAll('#mail-detail [data-att]').length === 3, null, { timeout: 5000 }).catch(() => {});
  const shown = await pg.evaluate(() => [...document.querySelectorAll('#mail-detail [data-att]')].map((b) => ({ text: b.textContent, warn: b.querySelector('.att-warn')?.textContent || null })));
  check(shown.length === 3 && shown[0].warn === 'a program (.exe), not a PDF' && shown[1].warn === null && shown[2].warn === null,
    `only invoice.pdf.exe is labelled, and the label says what it is: ${JSON.stringify(shown)}`);
  const saves = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'save_attachment').map(([, a]) => ({ index: a.index, confirmed: a.confirmed })));
  const ask = async () => {
    await pg.click('#mail-detail [data-att="0"]');
    await pg.waitForSelector('#warn-ov.open', { timeout: 3000 }).catch(() => {});
    return pg.evaluate(() => ({ open: document.querySelector('#warn-ov').classList.contains('open'), title: document.querySelector('#warn-title').textContent,
      name: document.querySelector('#warn-name').textContent, lead: document.querySelector('#warn-lead').textContent, go: document.querySelector('#warn-go').textContent,
      goDanger: document.querySelector('#warn-go').classList.contains('danger'), focus: document.activeElement && document.activeElement.id }));
  };
  let q = await ask();
  check(q.open && q.title === 'This file is a program (.exe) named to look like a document. Save it anyway?' && q.name === 'invoice.pdf.exe' && q.go === 'Save anyway' && q.goDanger && q.focus === 'warn-cancel',
    `Save asks first, with Cancel focused and only "Save anyway" in danger colour: ${JSON.stringify(q)}`);
  check(!/—|!/.test(q.title + q.lead + q.go), `the question has no em dash or exclamation mark: ${q.lead}`);
  if (process.env.UI1_SHOTS) await pg.screenshot({ path: process.env.UI1_SHOTS + '/ui1-question.png' });
  await pg.keyboard.press('Escape');
  let closed = await pg.evaluate(() => !document.querySelector('#warn-ov').classList.contains('open'));
  check(closed && (await saves()).length === 0, `Escape cancels and saves nothing: ${JSON.stringify(await saves())}`);
  q = await ask();
  await pg.keyboard.press('Enter');
  closed = await pg.evaluate(() => !document.querySelector('#warn-ov').classList.contains('open'));
  check(q.open && closed && (await saves()).length === 0, `Enter cancels too (Cancel is the default): ${JSON.stringify(await saves())}`);
  q = await ask();
  await pg.click('#warn-cancel');
  check(q.open && (await saves()).length === 0, `Cancel saves nothing: ${JSON.stringify(await saves())}`);
  q = await ask();
  await pg.click('#warn-go');
  await pg.waitForFunction(() => window.__toasts.some((t) => /^Saved to /.test(t)), null, { timeout: 3000 }).catch(() => {});
  let s = await saves();
  check(s.length === 1 && s[0].index === 1 && s[0].confirmed === true, `"Save anyway" saves, saying the customer confirmed: ${JSON.stringify(s)}`);
  check((await toasts(pg)).includes('Saved to /home/me/Downloads/invoice.pdf.exe'), 'and says where it went');
  // A document and a program that says what it is: no question.
  await pg.click('#mail-detail [data-att="1"]');
  await pg.click('#mail-detail [data-att="2"]');
  await pg.waitForFunction(() => __mock.calls.filter(([c]) => c === 'save_attachment').length === 3, null, { timeout: 3000 }).catch(() => {});
  s = await saves();
  const asked = await pg.evaluate(() => document.querySelector('#warn-ov').classList.contains('open'));
  check(!asked && s.length === 3 && s[1].index === 2 && s[1].confirmed === false && s[2].index === 3 && s[2].confirmed === false,
    `invoice.pdf and setup.exe save without asking: ${JSON.stringify(s)}`);
  if (process.env.UI1_SHOTS) {
    await pg.evaluate(() => { __mock.calls.length = 0; });
    await pg.screenshot({ path: process.env.UI1_SHOTS + '/ui1-label.png' });
  }
  // Listed by a build that did not mark it: the page learns from Rust's
  // refusal and asks, rather than failing or saving.
  await pg.evaluate(() => {
    const m = S.messages.find((x) => x.uid === 90);
    OPENED.delete(m.id); m.atts.forEach((a) => { delete a.disguised; });
    __mock.openFails = true; __mock.calls.length = 0; openMail(m.id);
  });
  await pg.waitForFunction(() => document.querySelectorAll('#mail-detail [data-att]').length === 3, null, { timeout: 3000 }).catch(() => {});
  const unlabelled = await pg.evaluate(() => document.querySelector('#mail-detail [data-att="0"] .att-warn'));
  await pg.click('#mail-detail [data-att="0"]');
  await pg.waitForSelector('#warn-ov.open', { timeout: 3000 }).catch(() => {});
  q = await pg.evaluate(() => ({ open: document.querySelector('#warn-ov').classList.contains('open'), title: document.querySelector('#warn-title').textContent }));
  s = await saves();
  check(unlabelled === null && q.open && /Save it anyway\?$/.test(q.title) && s.length === 1 && s[0].confirmed === false,
    `an unmarked disguise is refused by Rust, and the page asks then: ${JSON.stringify({ q, s })}`);
  await pg.click('#warn-go');
  await pg.waitForFunction(() => __mock.calls.filter(([c]) => c === 'save_attachment').length === 2, null, { timeout: 3000 }).catch(() => {});
  s = await saves();
  check(s.length === 2 && s[1].confirmed === true, `and "Save anyway" then saves it: ${JSON.stringify(s)}`);
  // Convert asks the same way (the button shows only for kinds the Bridge
  // reads, which a disguised name never ends in, so it is called directly).
  await pg.evaluate(() => { __mock.calls.length = 0; const m = S.messages.find((x) => x.uid === 90); window.__conv = convertAttachment(m, { i: 1, n: 'invoice.pdf.exe', disguised: true }); });
  await pg.waitForSelector('#warn-ov.open', { timeout: 3000 }).catch(() => {});
  const cq = await pg.evaluate(() => ({ title: document.querySelector('#warn-title').textContent, go: document.querySelector('#warn-go').textContent }));
  await pg.keyboard.press('Escape');
  await pg.evaluate(() => window.__conv);
  const reads = await pg.evaluate(() => __mock.calls.filter(([c]) => c === 'read_attachment').length);
  check(/Open it in the Format Bridge anyway\?$/.test(cq.title) && cq.go === 'Open anyway' && reads === 0, `Convert asks too, and Escape reads nothing: ${JSON.stringify({ cq, reads })}`);
  await pg.close();
}

console.log('\n— a licence too old to renew itself says what to do —');
{
  /* SEC-4: the website refuses to renew a licence expired more than
     RENEW_GRACE_DAYS ago, with reason 'too-old'. The licence box shows the
     server's sentence, not only the app's "expired". */
  const why = (pg) => pg.waitForSelector('#rata-licence-why', { timeout: 5000 }).then(() => pg.evaluate(() => document.querySelector('#rata-licence-why').textContent)).catch(() => null);
  const tooOld = 'This licence expired more than 90 days ago, so it cannot renew itself. Sign in at mailrata.org to get a new one.';
  let pg = await open(false, { old: true, renew: { body: { licensed: false, reason: 'too-old', message: tooOld } } });
  let said = await why(pg);
  check(pg.__renewals === 1 && said === tooOld, `too-old: the licence box says to sign in for a new one: ${JSON.stringify({ renewals: pg.__renewals, said })}`);
  await pg.close();
  /* A server having a bad morning is not an answer: the app's own words stay. */
  pg = await open(false, { old: true, renew: { status: 500, body: { error: 'The licence service is not configured.' } } });
  said = await why(pg);
  check(pg.__renewals === 1 && said === 'This licence expired on 1 March 2026.', `a server error keeps the app's own words: ${JSON.stringify({ renewals: pg.__renewals, said })}`);
  await pg.close();
}

console.log('\n— the website banner never shows in the app —');
{
  /* F2: "RATA reads your mail in the desktop app" is the website's sentence.
     In the app it would be false and in the way, licensed or not, whatever
     this browser's storage says and however often it is asked. */
  for (const licensed of [true, false]) {
    const pg = await open(licensed);
    const seen = await pg.evaluate(() => {
      const el = document.querySelector('#web-note');
      const before = { stored: localStorage.getItem('rata_web_note_dismissed') };
      webNote(); go('set'); go('inbox');
      const r = el.getBoundingClientRect();
      return { ...before, hidden: el.hidden, display: getComputedStyle(el).display, height: r.height };
    });
    check(seen.stored === null && seen.hidden && seen.display === 'none' && seen.height === 0,
      `${licensed ? 'licensed' : 'unlicensed'}: the banner is not drawn, though it was never dismissed: ${JSON.stringify(seen)}`);
    await pg.close();
  }
}

await browser.close();
console.log(fails ? `\n${fails} FAILED` : '\nALL PASSED');
process.exit(fails ? 1 : 0);
