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

const MOCK = ({ licensed, ms, old, lic }) => {
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
  /* BUG-L: a licence with a clock, as core.rs judges one. `lic.token` is
     what is on disk, `lic.now` this computer's clock (seconds), and
     `lic.genuine` every token the fake key signed, with its expiry. An
     expired licence refreshes nothing (refresh_mail answers `unlicensed`),
     and set_licence keeps a working or still-renewable licence against a
     key that does not work, answering `refused`. `lic.every` shortens the
     bridge's six-hour renewal timer. */
  M.lic = lic || null;
  if (lic && lic.every) window.__RATA_RENEW_EVERY = lic.every;
  const judge = (t) => {
    if (!t) return { reason: 'missing' };
    const exp = M.lic.genuine[t];
    if (exp === undefined) return { reason: 'malformed' };
    return exp >= M.lic.now ? { ok: true, exp } : { reason: 'expired', exp };
  };
  const worth = (j) => j.ok || (j.reason === 'expired' && M.lic.now - j.exp <= 90 * 86400);
  const clocked = () => {
    const j = judge(M.lic.token);
    if (j.ok) return { licensed: true, message: 'Licensed for RATA Pro until day ' + j.exp + '.', plan: { key: 'pro', label: 'RATA Pro', mail: null, chat: 3, split: true, ai: true },
      used: M.mailboxes.length, limit: null, renewSoon: j.exp - M.lic.now < 7 * 86400, token: M.lic.token, reason: null };
    return { licensed: false, plan: null, token: M.lic.token || null, reason: j.reason, renewSoon: j.reason === 'expired', used: M.mailboxes.length, limit: 0,
      message: j.reason === 'expired' ? 'This licence needs refreshing. Open RATA while online and it will renew itself.'
        : j.reason === 'missing' ? 'Enter your licence key to use RATA on this computer. Sign in at mailrata.org to find it.'
        : 'This licence could not be read. Sign in at mailrata.org to get a new one.' };
  };
  const setClocked = (given) => {
    const t = given == null ? null : String(given).replace(/\s+/g, '');
    try { sessionStorage.setItem('rata_set_licence', JSON.stringify(t)); } catch {}
    if (t !== null && M.lic.token) {
      const n = judge(t), h = judge(M.lic.token);
      if (!n.ok && worth(h) && !(n.reason === 'expired' && !h.ok && n.exp >= h.exp)) {
        return Object.assign(clocked(), { refused: { reason: n.reason, message: (n.reason === 'expired' ? 'That key has expired' : 'That key could not be read')
          + ', so RATA kept the licence it already has.' + (h.ok ? '' : ' That one has expired, and RATA renews it by itself when this computer is online.') } });
      }
    }
    M.lic.token = t || null;
    return clocked();
  };
  const standing = () => M.lic ? clocked() : M.licensed
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
        if (M.lic) return setClocked(args.licence);
        return standing();
      case 'list_mailboxes': return M.mailboxes;
      case 'unlink_mailbox':
        if (M.unlinkFails) throw 'This computer’s keychain refused access';
        M.mailboxes = M.mailboxes.filter((m) => m.email !== args.email);
        return null;
      case 'refresh_mail':
        if (M.refreshDelay) await new Promise((r) => setTimeout(r, M.refreshDelay));
        if (M.lic && !judge(M.lic.token).ok) return { messages: [], problems: [], skipped: [], unlicensed: clocked().message };
        return M.refresh || { messages: [], problems: [], skipped: [] };
      case 'older_mail': {
        M.olderAsked = (M.olderAsked || []).concat([args.email]);
        if (M.olderFails) throw { email: args.email, kind: 'net', error: 'imap.example.com could not be reached' };
        /* As core::usable: a mailbox the app does not have, and one parked
           for its password (BUG-C's gap checks). */
        if ((M.olderUnknown || []).includes(args.email)) throw { email: args.email, kind: 'unknown', error: args.email + ' is not linked in RATA.' };
        if ((M.olderParked || []).includes(args.email)) throw { email: args.email, kind: 'auth', error: args.email + ' refused its app password. Relink it in Settings.' };
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
        if ('picture' in f) return f;
        /* As core::sniff_image: what the bytes are, never the name, and
           nothing over core::PICTURE_MAX (5 MB). */
        const head = atob(String(f.data).slice(0, 24));
        const at = (s, o = 0) => head.slice(o, o + s.length) === s;
        const picture = atob(f.data).length > 5 * 1024 * 1024 ? null
          : at('\x89PNG\r\n\x1a\n') ? 'png' : at('\xFF\xD8\xFF') ? 'jpeg' : at('GIF87a') || at('GIF89a') ? 'gif'
            : at('RIFF') && at('WEBP', 8) ? 'webp' : null;
        return { ...f, picture };
      }
      case 'open_message':
        if (M.openFails) throw { email: args.email, kind: 'net', error: 'imap.example.com could not be reached' };
        if (M.openAtts && args.uid === 90) return { text: 'Please see the attached report.', truncated: false, attachments: M.openAtts };
        if (M.openAttsBy && M.openAttsBy[args.uid]) return { text: (M.openTextBy || {})[args.uid] || 'Pictures from the weekend.', truncated: false, attachments: M.openAttsBy[args.uid] };
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
      /* H8: what core::diagnostics builds, in its shape. */
      case 'diagnostics':
        if (M.diagFails) throw 'the mailbox list is busy';
        return M.diag || 'RATA diagnostics\nVersion: 0.1.42 (release build)\nSystem: linux x86_64\nMailboxes: 1\n\nMailbox 1: Example\n  IMAP: imap.example.com:993 (TLS from the start), found by table\n  Last error: none since RATA started';
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
      /* H6: as core::search: the newest of what the server found, and how
         many matched. `M.serverFound[email]` is { messages, matched }. */
      case 'search_mail': {
        M.searches = (M.searches || []).concat([args]);
        if (M.searchFails) throw { email: args.email, kind: 'net', error: args.email + ' would not search the inbox. It said: Search is switched off' };
        const f = (M.serverFound || {})[args.email] || { messages: [], matched: 0 };
        return { messages: f.messages, matched: f.matched };
      }
      case 'change_messages':
        /* What the mailbox answers, when a check says (BUG-M). */
        if (M.change) return M.change(args);
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
  /* `context`: a page in the same browser profile as another (its
     localStorage too), as RATA started again on the same computer. */
  const page = opts.context ? await opts.context.newPage() : await browser.newPage();
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
      /* A function answers each request (BUG-L); `abort` is unreachable. */
      const a = typeof opts.renew === 'function' ? opts.renew(JSON.parse(route.request().postData() || '{}')) : opts.renew;
      if (a.abort) return route.abort('internetdisconnected');
      return route.fulfill({ status: a.status || 200, headers: cors, contentType: 'application/json', body: JSON.stringify(a.body) });
    });
  }
  /* What mailrata.org's AI relay answers (bridge.js '/api/ai'); every
     request body it is sent is kept, as sent, in page.__ai. */
  if (opts.ai) {
    page.__ai = [];
    await page.route('https://mailrata.org/api/ai', (route) => {
      const cors = { 'access-control-allow-origin': '*', 'access-control-allow-headers': 'content-type', 'access-control-allow-methods': 'POST' };
      if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: cors });
      const raw = route.request().postData() || '';
      page.__ai.push(raw);
      const a = opts.ai(JSON.parse(raw));
      return route.fulfill({ status: a.status || 200, headers: cors, contentType: 'application/json', body: JSON.stringify(a.body) });
    });
  }
  await page.addInitScript(MOCK, { licensed, ms: !!opts.ms, old: !!opts.old, lic: opts.lic || null });
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
  /* Send goes at once here, as it did before Undo send (H9), unless a
     check is about the wait. */
  if (!opts.undo) await page.evaluate(() => { S.settings.undoSend = 0; });
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

console.log('\n— mail the server kept is never hidden: a refused Delete, Archive or Move comes back (BUG-M) —');
{
  const pg = await open(true);
  const mk = (folder, uid, extra) => Object.assign({ id: 'me@example.com_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: 'Bo', to_addr: 'bo@example.org', subject: 'S' + uid, preview: 'p', body: 'The text of ' + uid,
    ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'k' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate(async (msgs) => { __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] }; await serverSync('mail', true); },
    [mk('inbox', 1), mk('inbox', 2), mk('inbox', 3), mk('archive', 5, { uidvalidity: 9 }), mk('drafts', 21, { from_name: 'Me', from_addr: 'me@example.com' })]);
  const state = (id) => pg.evaluate(async (id) => {
    const m = S.messages.find((x) => x.id === id);
    return { held: !!m, gone: !!(S.gone || {})[id], text: m ? await bodyOf(m) : null };
  }, id);
  const toasted = async () => { await pg.waitForFunction(() => window.__toasts.length > 0, null, { timeout: 5000 }).catch(() => {}); return (await toasts(pg))[0] || ''; };
  const noTrash = 'This mailbox does not have a Trash folder, so RATA left the messages where they are rather than delete them for good.';

  // Delete, on a server with no Trash: refused, so the message comes back.
  await pg.evaluate((why) => { __mock.change = () => ({ ok: false, done: [], gone: [], kind: 'no-place', error: why }); window.__toasts = []; go('inbox'); openMail('me@example.com_1'); }, noTrash);
  await pg.click('#md-del');
  let t = await toasted();
  let st = await state('me@example.com_1');
  check(st.held && !st.gone && st.text === 'The text of 1', `a Delete the server refused puts the message back, text and all, and nothing is remembered as gone: ${JSON.stringify(st)}`);
  check(t === 'Nothing was changed — me@example.com: ' + noTrash, `and the toast says why: ${t}`);

  // Delete with the network down: the same.
  await pg.evaluate(() => { __mock.change = () => ({ ok: false, done: [], gone: [], kind: 'net', error: 'imap.example.com could not be reached' }); openMail('me@example.com_2'); });
  await pg.click('#md-del');
  t = await toasted();
  st = await state('me@example.com_2');
  check(st.held && !st.gone && t === 'Nothing was changed — me@example.com: imap.example.com could not be reached', `a Delete the server never heard comes back too: ${JSON.stringify(st)} ${t}`);

  // While the mailbox is being told, a refresh does not bring the message
  // back; once it says the message has gone, that is remembered.
  await pg.evaluate(() => { __mock.change = (a) => new Promise((res) => { window.__release = () => res({ ok: true, done: a.uids, gone: [] }); }); openMail('me@example.com_3'); });
  await pg.click('#md-del');
  await pg.waitForFunction(() => typeof window.__release === 'function', null, { timeout: 5000 }).catch(() => {});
  const during = await pg.evaluate(async (m) => {
    __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [] };
    await serverSync('mail', true);
    return { held: S.messages.some((x) => x.id === 'me@example.com_3'), gone: !!(S.gone || {})['me@example.com_3'] };
  }, mk('inbox', 3));
  check(!during.held && !during.gone, `a refresh while the mailbox is being told neither shows it nor counts it gone: ${JSON.stringify(during)}`);
  await pg.evaluate(() => window.__release());
  t = await toasted();
  st = await state('me@example.com_3');
  check(!st.held && st.gone && t === 'Deleted', `once the mailbox has done it, it is gone for good: ${JSON.stringify(st)} ${t}`);

  // Several at once: Archive on a server with no Archive folder.
  const pick = (ids) => pg.evaluate((ids) => { go('inbox'); selecting = true; SEL.clear(); ids.forEach((i) => SEL.add(i)); bulkRefresh(); window.__toasts = []; }, ids);
  await pg.evaluate(() => { __mock.change = () => ({ ok: false, done: [], gone: [], kind: 'no-place', error: 'This mailbox does not have an Archive folder, so RATA left the messages in the inbox.' }); });
  await pick(['me@example.com_1', 'me@example.com_2']);
  await pg.click('#bulk-bar [data-bulk="archive"]');
  t = await toasted();
  let both = [await state('me@example.com_1'), await state('me@example.com_2')];
  check(both.every((x) => x.held && !x.gone) && t === 'Nothing was changed — me@example.com: This mailbox does not have an Archive folder, so RATA left the messages in the inbox.',
    `an Archive of several the server refused brings them all back and says why: ${JSON.stringify(both)} ${t}`);

  // And Delete of several, refused.
  await pg.evaluate((why) => { __mock.change = () => ({ ok: false, done: [], gone: [], kind: 'no-place', error: why }); }, noTrash);
  await pick(['me@example.com_1', 'me@example.com_2']);
  await pg.click('#bulk-bar [data-bulk="del"]');
  t = await toasted();
  both = [await state('me@example.com_1'), await state('me@example.com_2')];
  check(both.every((x) => x.held && !x.gone && /^The text of /.test(x.text)) && t === 'Nothing was changed — me@example.com: ' + noTrash,
    `a Delete of several the server refused brings them all back: ${JSON.stringify(both)} ${t}`);
  await pg.evaluate(() => { selecting = false; SEL.clear(); });

  // Out of Gmail's archive a move is a copy: the message leaves the list,
  // but is not remembered as gone, so the archive listing can bring it back.
  await pg.evaluate(() => { __mock.change = (a) => ({ ok: true, done: a.uids, gone: [], copied: true }); window.__toasts = []; openMail('me@example.com_archive_5'); });
  await pg.click('#md-home');
  t = await toasted();
  st = await state('me@example.com_archive_5');
  check(!st.held && !st.gone && /^Moved to your inbox/.test(t), `Move to inbox out of Gmail's archive leaves without being remembered as gone: ${JSON.stringify(st)} ${t}`);

  // Read and starred from the server never make mail the customer wrote unread.
  const un = await pg.evaluate(async () => {
    __mock.refresh = { messages: [], flags: [{ id: 'me@example.com_drafts_21', unread: true, starred: false }, { id: 'me@example.com_1', unread: true, starred: false }], problems: [], skipped: [] };
    await serverSync('mail', true);
    const f = (id) => S.messages.find((m) => m.id === id).unread;
    return { draft: f('me@example.com_drafts_21'), inbox: f('me@example.com_1') };
  });
  check(un.draft === false && un.inbox === true, `a draft never turns unread on a refresh, while received mail follows the server: ${JSON.stringify(un)}`);
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

console.log('\n— address suggestions in the composer (H3) —');
{
  const pg = await open(true);
  const day = 864e5;
  const mk = (uid, extra) => Object.assign({ id: 'me@example.com_' + uid, folder: 'inbox', acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: 'Me', to_addr: 'me@example.com', to_all: ['me@example.com'], cc: [], in_reply_to: '',
    subject: 'Hello ' + uid, preview: 'p', body: 'Body text that must never be read ' + uid, ts: Date.now() - uid * day, unread: false, starred: false, uid, uidvalidity: 7,
    message_id: 's' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate(async (msgs) => {
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail');
    S.contacts.push({ id: 'c-h3', name: 'Beatrix Ortega', addr: 'bea@example.net' });
  }, [
    mk(1, { from_name: 'Smith, Ann', from_addr: 'ann.smith@example.org', cc: ['annika@example.org'] }),
    mk(2, { from_name: 'Smith, Ann', from_addr: 'ann.smith@example.org' }),
    mk(40, { from_name: 'Annette Old', from_addr: 'annette@example.org' }),
    mk(3, { from_name: 'Bob Lee', from_addr: 'bob@example.org' }),
    /* A spam sender, and a draft's recipient: neither is ever suggested. */
    mk(4, { id: 'me@example.com_junk_4', folder: 'junk', from_name: 'Anna Prize', from_addr: 'anna.prize@spam.example', subject: 'You won', cc: ['annspam@spam.example'] }),
    mk(5, { id: 'me@example.com_drafts_5', folder: 'drafts', from_name: 'Me', from_addr: 'me@example.com', to_name: 'Anne Draft', to_addr: 'anne.draft@example.org', to_all: ['anne.draft@example.org'] }),
  ]);
  const st = () => pg.evaluate(() => {
    const box = document.querySelector('#cmp-sugg'), inp = document.activeElement;
    return { open: !box.hidden, options: [...box.querySelectorAll('[role=option]')].map((o) => o.textContent), role: box.getAttribute('role'),
      expanded: inp && inp.getAttribute('aria-expanded'), active: inp && inp.getAttribute('aria-activedescendant'),
      selected: box.querySelector('[aria-selected=true]')?.id || null, value: inp && inp.value, id: inp && inp.id,
      composer: document.querySelector('#compose-ov').classList.contains('open') };
  });
  await pg.evaluate(() => { newDraft(); openCompose(); });
  await pg.click('#cmp-to');
  await pg.keyboard.type('a');
  let s = await st();
  check(!s.open && s.expanded === 'false', `one letter suggests nothing yet: ${JSON.stringify(s)}`);
  await pg.keyboard.type('n');
  s = await st();
  check(s.open && s.role === 'listbox' && s.expanded === 'true' && s.active === 'cmp-sugg-0' && s.selected === 'cmp-sugg-0' && s.options[0].startsWith('Smith, Ann'),
    `two letters show a match, the most recent and frequent first, and the field says so: ${JSON.stringify(s)}`);
  check(!s.options.some((o) => /prize|spam\.example/i.test(o)), `nothing is suggested from a spam sender or a spam message's Cc: ${JSON.stringify(s.options)}`);
  check(!s.options.some((o) => /anne\.draft/.test(o)), `nor from a draft: ${JSON.stringify(s.options)}`);
  check(s.options.some((o) => o.includes('annika@example.org')) && s.options.some((o) => o.includes('annette@example.org')), `every From, To and Cc of held mail feeds it: ${JSON.stringify(s.options)}`);
  await pg.keyboard.press('Enter');
  s = await st();
  check(!s.open && s.value === '"Smith, Ann" <ann.smith@example.org>, ' && s.expanded === 'false' && s.composer, `Enter fills it as "Name" <addr>, ready for the next: ${JSON.stringify(s)}`);
  await pg.keyboard.type('le');
  s = await st();
  check(s.open && s.options[0].startsWith('Bob Lee'), `a second address after a comma is suggested for, by a word in the middle of its name: ${JSON.stringify(s)}`);
  await pg.keyboard.press('Tab');
  s = await st();
  check(s.value === '"Smith, Ann" <ann.smith@example.org>, "Bob Lee" <bob@example.org>, ' && s.id === 'cmp-to', `Tab chooses too, and keeps the cursor in the field: ${JSON.stringify(s)}`);
  await pg.keyboard.type('ann');
  s = await st();
  check(s.open && !s.options.some((o) => o.includes('ann.smith@')), `someone already in the field is not offered again: ${JSON.stringify(s.options)}`);
  await pg.keyboard.press('ArrowDown');
  s = await st();
  check(s.active === 'cmp-sugg-1' && s.selected === 'cmp-sugg-1', `Down moves the choice: ${JSON.stringify(s)}`);
  await pg.keyboard.press('ArrowUp');
  await pg.keyboard.press('ArrowUp');
  s = await st();
  check(s.active === 'cmp-sugg-' + (s.options.length - 1), `Up from the first wraps to the last: ${JSON.stringify(s)}`);
  const before = s.value;
  await pg.keyboard.press('Escape');
  s = await st();
  check(!s.open && s.value === before && s.composer && s.expanded === 'false', `Escape closes the list without changing the field or closing the composer: ${JSON.stringify(s)}`);
  /* The mouse: a click on a suggestion in Cc. */
  await pg.evaluate(() => { const v = document.querySelector('#cmp-to'); v.value = v.value.replace(/ann$/, ''); });
  await pg.click('#cmp-cc-btn');
  await pg.keyboard.type('Beatr');
  s = await st();
  check(s.open && s.id === 'cmp-cc' && s.options[0].startsWith('Beatrix Ortega'), `People feed it, in Cc too: ${JSON.stringify(s)}`);
  await pg.click('#cmp-sugg-0');
  s = await st();
  check(s.value === '"Beatrix Ortega" <bea@example.net>, ' && s.id === 'cmp-cc' && !s.open, `a click chooses: ${JSON.stringify(s)}`);
  await pg.click('#cmp-bcc-btn');
  await pg.keyboard.type('prize');
  s = await st();
  check(!s.open, `a spam sender is not suggested by name either, in Bcc: ${JSON.stringify(s)}`);
  await pg.fill('#cmp-bcc', '');
  await pg.fill('#cmp-subj', 'Plans');
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  const sent = await pg.evaluate(() => __mock.calls.filter(([x]) => x === 'send_mail').map(([, a]) => a.draft)[0]);
  check(sent && sent.to === '"Smith, Ann" <ann.smith@example.org>, "Bob Lee" <bob@example.org>' && sent.cc === '"Beatrix Ortega" <bea@example.net>' && sent.bcc === '',
    `the quoted name with a comma reaches send_mail whole, without the trailing comma: ${JSON.stringify(sent)}`);
  await pg.evaluate(() => { newDraft(); openCompose(); });
  await pg.click('#cmp-to');
  await pg.keyboard.type('exam');
  s = await st();
  check(s.options.length >= 4 && s.options.at(-1).includes('me@example.com') && s.options.at(-1).includes('Your mailbox') && !s.options.slice(0, -1).some((o) => o.includes('me@example.com')),
    `the customer's own address ranks last: ${JSON.stringify(s.options)}`);
  await pg.keyboard.press('Escape');
  /* Built from addresses and names only: nothing that reads a message's
     text is anywhere in it. */
  const src = await pg.evaluate(() => [suggPool, suggUpdate, suggDraw, suggChoose].map(String).join('\n'));
  check(!/body|bodyOf|bodiesOf|textOf|\.prev\b/.test(src), 'suggestions read no message text');
  /* A name that could break the line is cleaned before it is written. */
  const odd = await pg.evaluate(() => { S.contacts.push({ id: 'c-h3b', name: 'Eve "\u202e" <x>, Q', addr: 'eve@example.net' }); newDraft(); openCompose();
    const inp = document.querySelector('#cmp-to'); inp.focus(); inp.value = 'eve'; inp.setSelectionRange(3, 3); inp.dispatchEvent(new Event('input'));
    suggChoose(0); return inp.value; });
  check(odd === '"Eve x, Q" <eve@example.net>, ', `a stranger's quotes and brackets never reach the line: ${JSON.stringify(odd)}`);
  await pg.close();
}

console.log('\n— keyboard shortcuts (H4) —');
{
  const pg = await open(true);
  const mk = (uid, extra) => Object.assign({ id: 'me@example.com_' + uid, folder: 'inbox', acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Sender ' + uid, from_addr: 's' + uid + '@example.org', to_name: 'Me', to_addr: 'me@example.com', to_all: ['me@example.com'], cc: [], in_reply_to: '',
    subject: 'Keys ' + uid, preview: 'p', body: 'Text ' + uid, ts: Date.now() - uid * 60e3, unread: true, starred: false, uid, uidvalidity: 7,
    message_id: 'k' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  const msgs = [mk(1, { from_name: 'Ann', from_addr: 'ann@example.org', to_all: ['me@example.com', 'bo@example.org'], cc: ['cy@example.org'] })];
  for (let u = 2; u <= 60; u++) msgs.push(mk(u));
  msgs.push(mk(61, { id: 'me@example.com_sent_61', folder: 'sent', from_name: 'Me', from_addr: 'me@example.com', to_name: 'Dee', to_addr: 'dee@example.org', to_all: ['dee@example.org'], unread: false }));
  await pg.evaluate(async (msgs) => {
    __mock.folders = [{ name: 'Receipts', label: 'Receipts' }];
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail');
    S.settings.keys = undefined;
    go('inbox'); mailFilter = 'all'; renderMailFilters(); renderMail();
    document.activeElement && document.activeElement.blur();
  }, msgs);
  const press = async (k) => { await pg.keyboard.press(k); await pg.waitForTimeout(80); };
  const st = () => pg.evaluate(() => {
    const sc = document.querySelector('#mail-scroll'), pool = (VL.get(sc) || {}).pool || [];
    const kb = document.querySelector('#mail-scroll .mail-row.kb'), r = kb && kb.getBoundingClientRect(), v = sc.getBoundingClientRect();
    return { at: KEY_AT, idx: pool.findIndex((m) => m.id === KEY_AT), sel: selMail, view: currentView, pool: pool.map((m) => m.id),
      kb: kb ? kb.dataset.id : null, kbnav: document.body.classList.contains('kbnav'),
      inView: !!r && r.top >= Math.max(v.top, 0) - 1 && r.bottom <= Math.min(v.bottom, innerHeight) + 1,
      outline: kb ? getComputedStyle(kb).outlineStyle : null,
      composing: document.querySelector('#compose-ov').classList.contains('open'), kind: DRAFT.kind, ticked: [...SEL], selecting,
      sheet: document.querySelector('#keys-ov').classList.contains('open'), focus: document.activeElement ? (document.activeElement.id || document.activeElement.tagName) : null,
      acts: __mock.calls.filter(([c]) => c === 'change_messages').map(([, a]) => a.action + ':' + a.uids.join()) };
  });
  const clear = () => pg.evaluate(() => { __mock.calls = []; window.__toasts = []; });

  /* The list: j/k and the arrows move, and nothing opens or is marked read. */
  await clear();
  await press('j');
  let s = await st();
  check(s.idx === 0 && s.kb === s.pool[0] && s.kbnav && s.outline === 'solid' && s.sel === null, `j puts the keyboard on the first message, outlined, without opening it: ${JSON.stringify({ idx: s.idx, kb: s.kb, outline: s.outline, sel: s.sel })}`);
  await press('j'); await press('ArrowDown');
  s = await st();
  check(s.idx === 2 && s.kb === s.pool[2], `j and Down move down: ${s.idx}`);
  await press('k'); await press('ArrowUp'); await press('k');
  s = await st();
  check(s.idx === 0 && s.sel === null && s.acts.length === 0, `k and Up move up, stopping at the top, and nothing is opened or marked read: ${JSON.stringify({ idx: s.idx, sel: s.sel, acts: s.acts })}`);
  for (let n = 0; n < 45; n++) await pg.keyboard.press('j');
  await pg.waitForTimeout(150);
  s = await st();
  check(s.idx === 45 && s.kb === s.pool[45] && s.inView, `far down the list the row is drawn and scrolled into view: ${JSON.stringify({ idx: s.idx, kb: s.kb, inView: s.inView })}`);
  await pg.evaluate(() => { KEY_AT = null; document.querySelector('#mail-scroll').scrollTop = 0; scrollTo(0, 0); });
  await press('j');

  /* x ticks, and turns Select on as the Select button does. */
  await press('x');
  s = await st();
  check(s.selecting && s.ticked.join() === s.pool[0] && s.kb === s.pool[0], `x ticks the message and turns Select on: ${JSON.stringify({ selecting: s.selecting, ticked: s.ticked })}`);
  await press('j'); await press('x');
  s = await st();
  check(s.ticked.length === 2 && s.ticked.includes(s.pool[1]), `x on the next ticks it too: ${JSON.stringify(s.ticked)}`);
  await press('x');
  s = await st();
  check(s.ticked.join() === s.pool[0], `x again unticks: ${JSON.stringify(s.ticked)}`);
  await pg.click('#bulk-bar [data-bulk="clear"]');
  await pg.evaluate(() => document.activeElement.blur());
  await press('k');

  /* Enter opens, as a click does. */
  await press('Enter');
  s = await st();
  const first = s.pool[0];
  check(s.sel === first && (await pg.evaluate(() => !!document.querySelector('#md-reply'))), `Enter opens the message: ${s.sel}`);

  /* On an open message: each key presses its button. */
  await clear();
  await press('s');
  let m = await pg.evaluate((id) => S.messages.find((x) => x.id === id).starred, first);
  s = await st();
  check(m && s.acts.join() === 'star:1', `s stars it, on the server too: ${JSON.stringify(s.acts)}`);
  await press('s');
  m = await pg.evaluate((id) => S.messages.find((x) => x.id === id).starred, first);
  check(!m && (await st()).acts.join() === 'star:1,unstar:1', 's again unstars it');
  await clear();
  await press('u');
  m = await pg.evaluate((id) => S.messages.find((x) => x.id === id).unread, first);
  check(m && (await st()).acts.join() === 'unread:1', 'u marks it unread, on the server too');
  await press('r');
  await pg.waitForTimeout(250);
  s = await st();
  check(s.composing && s.kind === 'reply' && (await pg.inputValue('#cmp-to')).includes('ann@example.org'), `r replies: ${JSON.stringify({ composing: s.composing, kind: s.kind })}`);
  await pg.evaluate(() => document.activeElement.blur());
  await press('Escape');
  s = await st();
  check(!s.composing && s.kind === 'reply' && s.sel === first, `Escape closes the composer, keeping the reply, and nothing else: ${JSON.stringify({ composing: s.composing, kind: s.kind, sel: s.sel })}`);
  await pg.evaluate(() => newDraft());
  await press('a');
  await pg.waitForTimeout(250);
  s = await st();
  const cc = await pg.inputValue('#cmp-cc');
  check(s.composing && /cy@example\.org/.test(cc) && /bo@example\.org/.test(cc), `a replies to everyone: ${JSON.stringify({ composing: s.composing, cc })}`);
  await press('Escape');
  await pg.evaluate(() => newDraft());
  await press('f');
  await pg.waitForTimeout(250);
  s = await st();
  check(s.composing && s.kind === 'forward', `f forwards: ${JSON.stringify({ composing: s.composing, kind: s.kind })}`);
  await press('Escape');
  await pg.evaluate(() => newDraft());
  /* Reply all only where it is offered. */
  await pg.evaluate(() => openMail('me@example.com_2'));
  await press('a');
  await pg.waitForTimeout(200);
  s = await st();
  check(!s.composing && !(await pg.evaluate(() => !!document.querySelector('#md-replyall'))), 'a does nothing where Reply all is not offered (no one else to answer)');
  /* Move to opens its menu, and Escape closes the menu before the message. */
  await press('m');
  await pg.waitForTimeout(250);
  s = await st();
  const menu = await pg.evaluate(() => document.querySelector('#md-filemenu').classList.contains('open'));
  check(menu && s.focus === 'md-file', `m opens Move to and puts the focus on it: ${JSON.stringify({ menu, focus: s.focus })}`);
  await pg.evaluate(() => document.activeElement.blur());
  await press('Escape');
  s = await st();
  check(!(await pg.evaluate(() => document.querySelector('#md-filemenu').classList.contains('open'))) && s.sel === 'me@example.com_2', 'Escape closes the menu first, and the message stays open');
  /* e archives, # and Delete delete: the button's own way, and the
     keyboard moves on to what took the message's place. */
  await pg.evaluate(() => { openMail('me@example.com_3'); KEY_AT = KEY_SEL = selMail; });
  await clear();
  const before = (await st()).pool;
  await press('e');
  await pg.waitForTimeout(150);
  s = await st();
  check(s.acts.join() === 'archive:3' && !s.pool.includes('me@example.com_3') && s.at === before[before.indexOf('me@example.com_3') + 1] && s.sel === null,
    `e archives it through the Archive button, and the keyboard is on the next message: ${JSON.stringify({ acts: s.acts, at: s.at })}`);
  await pg.evaluate(() => openMail('me@example.com_4'));
  await clear();
  await press('#');
  await pg.waitForTimeout(150);
  s = await st();
  check(s.acts.join() === 'trash:4' && !s.pool.includes('me@example.com_4'), `# deletes it to the Trash through the Delete button: ${JSON.stringify(s.acts)}`);
  await pg.evaluate(() => openMail('me@example.com_5'));
  await clear();
  await press('Delete');
  await pg.waitForTimeout(150);
  s = await st();
  check(s.acts.join() === 'trash:5', `so does Delete: ${JSON.stringify(s.acts)}`);
  /* Through the button, BUG-M's rule holds: a Trash the server refuses
     puts the message back. */
  await pg.evaluate(() => { openMail('me@example.com_7'); __mock.changeFails = true; });
  await clear();
  await press('#');
  await pg.waitForTimeout(300);
  const back = await pg.evaluate(() => ({ held: S.messages.some((x) => x.id === 'me@example.com_7'), said: window.__toasts.slice() }));
  check(back.held && back.said.some((t) => /Nothing was changed/.test(t)), `a delete the server refuses, by key, puts the message back and says so: ${JSON.stringify(back)}`);
  await pg.evaluate(() => { __mock.changeFails = false; });
  /* Sent mail has no Archive button, so e does nothing. */
  await clear();
  await pg.evaluate(() => { mailFilter = 'sent'; renderMail(); openMail('me@example.com_sent_61'); });
  await press('e');
  await pg.waitForTimeout(150);
  s = await st();
  check(s.acts.length === 0 && s.sel === 'me@example.com_sent_61' && !(await pg.evaluate(() => !!document.querySelector('#md-archive'))), `e does nothing where Archive does not show: ${JSON.stringify(s.acts)}`);
  await pg.evaluate(() => { mailFilter = 'all'; renderMailFilters(); renderMail(); });

  /* Escape closes the open message. */
  await press('Escape');
  s = await st();
  check(s.sel === null && (await pg.evaluate(() => !document.querySelector('#md-reply'))), `Escape closes the open message: ${s.sel}`);

  /* Anywhere: c, /, g i. */
  await press('c');
  s = await st();
  check(s.composing && s.kind === 'new', `c opens the composer: ${JSON.stringify({ composing: s.composing, kind: s.kind })}`);
  /* Typing in the composer types, whatever the letter. */
  await pg.click('#cmp-subj');
  await pg.keyboard.type('jk#x/');
  await pg.click('#cmp-body');
  await pg.keyboard.type('j?cegs');
  await pg.keyboard.press('Delete');
  s = await st();
  check(s.composing && (await pg.inputValue('#cmp-subj')) === 'jk#x/' && (await pg.inputValue('#cmp-body')).startsWith('j?cegs') && !s.sheet && s.view === 'inbox',
    `typing j (and every other shortcut) in the composer types it: ${JSON.stringify({ subj: await pg.inputValue('#cmp-subj'), sheet: s.sheet })}`);
  const spell = await pg.evaluate(() => ['#cmp-subj', '#cmp-body'].map((q) => { const el = document.querySelector(q); return { spell: el.spellcheck && el.getAttribute('spellcheck'), lang: el.lang }; }).concat([{ want: userLang() }]));
  check(spell[0].spell === 'true' && spell[1].spell === 'true' && spell[0].lang === spell[2].want && spell[1].lang === spell[2].want && spell[2].want.length >= 2,
    `the subject and body are spell-checked, in the customer's language: ${JSON.stringify(spell)}`);
  await press('Escape');
  s = await st();
  check(!s.composing && (await pg.inputValue('#cmp-subj')) === 'jk#x/', 'Escape in the composer closes it and keeps the draft');
  await pg.evaluate(() => newDraft());
  await pg.evaluate(() => document.activeElement.blur());
  await press('/');
  s = await st();
  check(s.view === 'search' && s.focus === 'search-input' && (await pg.inputValue('#search-input')) === '', `/ goes to search and puts the cursor in it, without typing a slash: ${JSON.stringify({ view: s.view, focus: s.focus })}`);
  await pg.keyboard.type('jk');
  s = await st();
  check((await pg.inputValue('#search-input')) === 'jk' && s.view === 'search', 'typing j in search types a j');
  await pg.evaluate(() => document.activeElement.blur());
  await press('g'); await press('i');
  s = await st();
  check(s.view === 'inbox', `g then i goes to the inbox: ${s.view}`);
  await pg.evaluate(() => go('set'));
  await press('g');
  await pg.waitForTimeout(1100);
  await press('i');
  s = await st();
  check(s.view === 'set', `but not a second apart: ${s.view}`);
  await pg.evaluate(() => go('inbox'));

  /* Ctrl, Cmd and Alt are the system's. */
  await pg.evaluate(() => { openMail('me@example.com_6'); KEY_AT = KEY_SEL = selMail; });
  await clear();
  const was = await st();
  for (const k of ['Control+c', 'Control+j', 'Meta+c', 'Alt+r', 'Control+Delete']) await press(k);
  s = await st();
  m = await pg.evaluate(() => S.messages.find((x) => x.id === 'me@example.com_6'));
  check(!s.composing && s.at === was.at && s.sel === 'me@example.com_6' && s.view === 'inbox' && s.acts.length === 0 && m && !m.starred && !s.sheet,
    `Ctrl+C (and Ctrl, Cmd or Alt with any key) does nothing extra: ${JSON.stringify({ composing: s.composing, at: s.at, acts: s.acts })}`);

  /* The sheet lists exactly the table, and the table is the card's. */
  await press('?');
  const sheet = await pg.evaluate(() => ({
    open: document.querySelector('#keys-ov').classList.contains('open'), role: document.querySelector('#keys-ov').getAttribute('role'), focus: document.activeElement.id,
    rows: [...document.querySelectorAll('#keys-list .keys-row')].map((r) => ({ i: +r.dataset.key, what: r.querySelector('.keys-what').textContent, caps: [...r.querySelectorAll('kbd')].map((k) => k.textContent) })),
    table: KEYS.map((k, i) => ({ i, what: k.what, caps: k.keys.flatMap((x) => x.split(' ').map((p) => (KEY_CAPS[p] || [p])[0])), keys: k.keys })),
  }));
  const byI = (a) => JSON.stringify([...a].sort((x, y) => x.i - y.i).map(({ i, what, caps }) => ({ i, what, caps })));
  check(sheet.open && sheet.role === 'dialog' && sheet.focus === 'keys-close', `? shows the sheet, as a dialog with the focus on Close: ${JSON.stringify({ open: sheet.open, focus: sheet.focus })}`);
  check(sheet.rows.length === sheet.table.length && byI(sheet.rows) === byI(sheet.table), `the sheet lists exactly the table, every row once: ${sheet.rows.length} rows, ${sheet.table.length} in the table`);
  const card = ['j', 'ArrowDown', 'k', 'ArrowUp', 'Enter', 'x', 'r', 'a', 'f', 'e', '#', 'Delete', 's', 'u', 'm', 'c', '/', 'g i', 'Escape', '?'];
  const keys = sheet.table.flatMap((t) => t.keys);
  check(keys.length === card.length && card.every((k) => keys.includes(k)), `the table holds the card's keys and no others: ${JSON.stringify(keys)}`);
  await press('j');
  check((await st()).at === s.at, 'keys do nothing else while the sheet is open');
  await press('Escape');
  s = await st();
  check(!s.sheet && s.sel === 'me@example.com_6', 'Escape closes the sheet, and only the sheet');

  /* Never while a question is on screen. */
  await pg.evaluate(() => askToOpen('https://example.com/x', 'example.com'));
  await press('c'); await press('j'); await press('?');
  s = await st();
  check(!s.composing && s.at === was.at && !s.sheet, 'nothing while the link question is open');
  await press('Escape');
  s = await st();
  check(!(await pg.evaluate(() => document.querySelector('#link-ov').classList.contains('open'))) && s.sel === 'me@example.com_6', 'Escape closes the link question, and not the message behind it');
  await pg.evaluate(() => { window.__warned = askDisguised({ n: 'invoice.pdf.exe' }, true); document.activeElement.blur(); });
  await press('c'); await press('?');
  s = await st();
  check(!s.composing && !s.sheet, 'nothing while the disguised-program question is open');
  await press('Escape');
  check(await pg.evaluate(async () => (await window.__warned) === false && !document.querySelector('#warn-ov').classList.contains('open')), 'Escape cancels it, as before');
  await pg.evaluate(() => { go('set'); openDelete(); document.activeElement.blur(); });
  await press('c'); await press('?'); await press('g'); await press('i');
  s = await st();
  check(!s.composing && !s.sheet && s.view === 'set', 'nothing while the delete-account question is open');
  await pg.click('#del-cancel');

  /* H3's suggestions keep their keys: Escape closes only the list. */
  await pg.evaluate(() => { go('inbox'); newDraft(); openCompose(); });
  await pg.click('#cmp-to');
  await pg.keyboard.type('se');
  const listed = await pg.evaluate(() => !document.querySelector('#cmp-sugg').hidden);
  await press('ArrowDown');
  const moved = await pg.evaluate(() => document.querySelector('#cmp-to').getAttribute('aria-activedescendant'));
  await press('Escape');
  s = await st();
  check(listed && moved === 'cmp-sugg-1' && s.composing && s.at === was.at && (await pg.evaluate(() => document.querySelector('#cmp-sugg').hidden)),
    `with the suggestions open, Down moves in them and Escape closes only them: ${JSON.stringify({ listed, moved, composing: s.composing })}`);
  await press('Escape');
  s = await st();
  check(!s.composing && (await pg.inputValue('#cmp-to')) === 'se', 'the next Escape closes the composer, keeping what was typed');
  await pg.evaluate(() => newDraft());

  /* The setting. */
  await pg.evaluate(() => go('set'));
  const row = await pg.evaluate(() => ({ shown: getComputedStyle(document.querySelector('#keys-row')).display !== 'none', on: document.querySelector('#set-keys').checked }));
  check(row.shown && row.on, `Settings has the row, on by default: ${JSON.stringify(row)}`);
  await pg.click('#set-keys');
  check(await pg.evaluate(() => S.settings.keys === false), 'unticking it turns them off');
  await pg.evaluate(() => { go('inbox'); document.activeElement.blur(); });
  await press('j'); await press('c'); await press('?'); await press('s');
  s = await st();
  m = await pg.evaluate(() => S.messages.find((x) => x.id === 'me@example.com_6'));
  check(s.at === was.at && !s.composing && !s.sheet && !m.starred, `off, the keys do nothing: ${JSON.stringify({ at: s.at, composing: s.composing, sheet: s.sheet })}`);
  await pg.evaluate(() => openCompose());
  await press('Escape');
  check(!(await st()).composing, 'but Escape still closes the composer, as it always has');
  await pg.evaluate(() => go('set'));
  await pg.click('#keys-show');
  const off = await pg.evaluate(() => ({ open: document.querySelector('#keys-ov').classList.contains('open'), foot: document.querySelector('#keys-foot').textContent }));
  check(off.open && /off/.test(off.foot), `Show shortcuts opens the sheet, which says they are off: ${JSON.stringify(off)}`);
  await press('Escape');
  await pg.click('#set-keys');
  await pg.evaluate(() => { go('inbox'); document.activeElement.blur(); });
  await press('c');
  check((await st()).composing && (await pg.evaluate(() => S.settings.keys === true)), 'ticked again, they work again');
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

console.log('\n— mail that left a folder on another device leaves RATA too (BUG-C) —');
{
  const pg = await open(true);
  const mk = (uid, folder = 'inbox', extra) => Object.assign({ id: 'me@example.com_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: 'Bo', to_addr: 'bo@example.org', subject: 'Here ' + uid, preview: 'p', body: 'b',
    ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'p' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  const sync = (msgs, present, before) => pg.evaluate(async ({ msgs, present, before }) => {
    if (before === 'leaving') LEAVING.add('me@example.com_4');
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [], ...(present ? { present } : {}) };
    await serverSync('mail', true);
    LEAVING.delete('me@example.com_4');
    const of = (f) => S.messages.filter((m) => folderOf(m) === f && m.mailbox === 'me@example.com' && m.uid).map((m) => m.uid).sort((a, b) => a - b).join();
    return { inbox: of('inbox'), sent: of('sent'), gone: Object.keys(S.gone || {}).filter((k) => k.startsWith('me@example.com_')) };
  }, { msgs, present, before });
  const inbox = (floor, next, uids, extra) => Object.assign({ email: 'me@example.com', folder: 'inbox', uidvalidity: 7, floor, next, uids }, extra || {});
  let r = await sync([1, 2, 3, 4, 5].map((u) => mk(u)).concat([mk(11, 'sent'), mk(12, 'sent')]), [inbox(1, 6, [1, 2, 3, 4, 5])]);
  check(r.inbox === '1,2,3,4,5' && r.sent === '11,12', `a listing of everything held changes nothing: ${JSON.stringify(r)}`);
  await pg.evaluate(() => openMail('me@example.com_3'));
  r = await sync([], [inbox(1, 6, [1, 2, 4, 5])]);
  const pane = await pg.evaluate(() => ({ sel: selMail, open: $('#mail-detail').classList.contains('open') }));
  check(r.inbox === '1,2,4,5' && r.gone.length === 0, `a message gone from the listing leaves the inbox, and not through S.gone: ${JSON.stringify(r)}`);
  check(pane.sel === null && !pane.open, `and the reading pane it was open in closes: ${JSON.stringify(pane)}`);
  r = await sync([], [inbox(4, 6, [5])]);
  check(r.inbox === '1,2,5', `below the listing's floor nothing is judged, and one above it that is not listed goes: ${JSON.stringify(r)}`);
  r = await sync([], null);
  check(r.inbox === '1,2,5', `a refresh whose listing the server refused removes nothing: ${JSON.stringify(r)}`);
  r = await sync([], [inbox(1, 6, [], { uidvalidity: 8 })]);
  check(r.inbox === '1,2,5', `a listing under another UIDVALIDITY (the folder rebuilt) drops nothing: ${JSON.stringify(r)}`);
  r = await sync([mk(6), mk(4)], [inbox(1, 6, [1, 2, 5])]);
  check(r.inbox === '1,2,4,5,6', `what this refresh brought itself is never dropped by it, nor anything at or above UIDNEXT: ${JSON.stringify(r)}`);
  r = await sync([], [inbox(1, 7, [1, 2, 5, 6])], 'leaving');
  check(r.inbox === '1,2,4,5,6', `mail on its way out, which the page has told the server about, is left to that: ${JSON.stringify(r)}`);
  r = await sync([], [{ email: 'me@example.com', folder: 'sent', uidvalidity: 7, floor: 1, next: 13, uids: [12] }]);
  check(r.sent === '12' && r.inbox === '1,2,4,5,6', `Sent is judged by its own listing, and the inbox by nothing else: ${JSON.stringify(r)}`);
  r = await sync([], [inbox(1, 7, [], { email: 'work@example.net' })]);
  check(r.inbox === '1,2,4,5,6', `another mailbox's listing touches none of this one's mail: ${JSON.stringify(r)}`);
  /* One found by a search on the server (H6) below the floor stays; the
     next refresh would otherwise skip everything between. */
  await sync([mk(1001), mk(1700)], null);
  await pg.evaluate(() => { for (const id of ['me@example.com_1001', 'me@example.com_1700']) S.messages.find((m) => m.id === id).srvFound = true; });
  r = await sync([], [inbox(1500, 1600, [])]);
  check(r.inbox === '1,2,4,5,6,1001,1700', `messages a server search brought, below the floor or above UIDNEXT, stay: ${JSON.stringify(r)}`);
  await pg.close();
}

console.log('\n— a gap that cannot be filled no longer stops Load older mail (BUG-C) —');
{
  const pg = await open(true);
  const clear = () => pg.evaluate(() => {
    __mock.inbox = [10, 20, 30, 40];
    __mock.mk = (u) => ({ id: 'me@example.com_' + u, folder: 'inbox', acct: 'me@example.com', acct_label: 'me@example.com', from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com',
      subject: 'Page ' + u, preview: 'p', body: 'b', ts: Date.parse('2025-01-01T10:00:00Z') + u * 60000, unread: false, starred: false, uid: u, uidvalidity: 7, message_id: 'g' + u + '@example.org', reply_to: '', truncated: false, attachments: [], html: false });
    S.messages = S.messages.filter((m) => m.mailbox !== 'me@example.com');
    S.linked.forEach((l) => { delete l.historyDone; });
  });
  /* Held as a refresh brings it, through the bridge. */
  const setup = async () => {
    await clear();
    await pg.evaluate(async () => { S.gaps = []; __mock.refresh = { messages: [__mock.mk(40)], flags: [], problems: [], skipped: [] }; await serverSync('mail', true); __mock.olderAsked = []; });
  };
  const held = () => pg.evaluate(() => S.messages.filter((m) => m.mailbox === 'me@example.com' && m.uid).map((m) => m.uid).sort((a, b) => a - b).join());
  /* A mailbox removed in the app while the page still lists it: the app
     says it is not linked, and the gap goes. */
  await setup();
  await pg.evaluate(() => {
    S.linked.push({ id: 'lk_old', type: 'mail', label: 'old@example.net', status: 'live' });
    __mock.olderUnknown = ['old@example.net'];
    S.gaps = [{ email: 'old@example.net', folder: 'inbox', uidvalidity: 7, top: 500, floor: 100 }];
  });
  await pg.evaluate(() => loadOlder());
  let g = await pg.evaluate(() => ({ gaps: S.gaps.length, asked: __mock.olderAsked }));
  check(g.gaps === 0, `a gap whose mailbox the app does not have is dropped: ${JSON.stringify(g)}`);
  check((await held()) === '10,20,30,40', `and Load older mail goes on to page the others in the same press: ${await held()}`);
  /* A gap left from a mailbox no longer in the page at all is not even asked about. */
  await setup();
  await pg.evaluate(() => { S.gaps = [{ email: 'long-gone@example.net', folder: 'inbox', uidvalidity: 7, top: 500, floor: 100 }]; });
  await pg.evaluate(() => loadOlder());
  g = await pg.evaluate(() => ({ gaps: S.gaps.length, asked: __mock.olderAsked }));
  check(g.gaps === 0 && !g.asked.includes('long-gone@example.net') && (await held()) === '10,20,30,40', `nor is a gap of a mailbox that is not linked at all, and it asks nothing for it: ${JSON.stringify(g)}`);
  /* Remove takes the mailbox's gaps with it. */
  await pg.evaluate(() => {
    __mock.mailboxes.push({ email: 'side@example.net', host: 'imap.example.net', port: 993, label: 'Side' });
    S.linked.push({ id: 'lk_side', type: 'mail', label: 'side@example.net', status: 'live' });
    S.gaps = [{ email: 'side@example.net', folder: 'inbox', uidvalidity: 7, top: 500, floor: 100 }, { email: 'me@example.com', folder: 'sent', uidvalidity: 7, top: 50, floor: 10 }];
  });
  await pg.evaluate(() => removeLinked('lk_side'));
  g = await pg.evaluate(() => S.gaps.map((x) => x.email));
  check(g.join() === 'me@example.com', `Remove takes that mailbox's gaps with it and leaves the others: ${JSON.stringify(g)}`);
  /* A mailbox parked for its password keeps its gap for later, and Load
     older mail still pages the rest. */
  await setup();
  await pg.evaluate(() => {
    __mock.mailboxes.push({ email: 'parked@example.net', host: 'imap.example.net', port: 993, label: 'Parked' });
    S.linked.push({ id: 'lk_parked', type: 'mail', label: 'parked@example.net', status: 'error' });
    __mock.olderParked = ['parked@example.net'];
    S.gaps = [{ email: 'parked@example.net', folder: 'inbox', uidvalidity: 7, top: 500, floor: 100 }];
    window.__toasts = [];
  });
  await pg.evaluate(() => loadOlder());
  g = await pg.evaluate(() => ({ gaps: S.gaps.map((x) => x.email), toasts: window.__toasts.slice() }));
  check(g.gaps.join() === 'parked@example.net', `a parked mailbox's gap is kept: ${JSON.stringify(g.gaps)}`);
  check((await held()) === '10,20,30,40' && g.toasts.some((t) => /^3 older loaded, but parked@example\.net/.test(t)),
    `and older mail of the others still loads, saying what did not: ${JSON.stringify(g.toasts)}`);
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

console.log('\n— picture attachments are shown in the message, by what their bytes are —');
{
  /* H11: a picture attachment shows as a thumbnail once the message is open,
     as the type Rust found in its bytes (the mock sniffs as
     core::sniff_image does); anything else named as a picture, a disguised
     name, an SVG, and a picture known to be over 5 MB are listed only. */
  const pg = await open(true);
  const mkAtt = (index, name, size, extra) => Object.assign({ index, name, mime: 'image/png', size, disguised: false }, extra || {});
  const atts = [
    mkAtt(1, 'holiday.jpg', 2000, { mime: 'image/jpeg' }),
    mkAtt(2, 'chart.png', 90),
    mkAtt(3, 'invoice.pdf.exe', 4, { mime: 'application/octet-stream', disguised: true }),
    mkAtt(4, 'logo.svg', 60, { mime: 'image/svg+xml' }),
    mkAtt(5, 'poster.png', 6 * 1024 * 1024),
    mkAtt(6, '<img src=x onerror=window.__pwned=2>.gif', 40, { mime: 'image/gif' }),
    mkAtt(7, 'scan.jpeg', 3000, { mime: 'image/jpeg' }),
    mkAtt(8, 'photo.webp', 3000, { mime: 'image/webp' }),
    mkAtt(9, 'a.png', 900), mkAtt(10, 'b.png', 900), mkAtt(11, 'c.png', 900),
  ];
  await pg.evaluate(async (atts) => {
    /* Real pictures, drawn here, so they decode and can be looked at. */
    const draw = (type, w, h, hue) => {
      const c = document.createElement('canvas'); c.width = w; c.height = h;
      const x = c.getContext('2d');
      x.fillStyle = `hsl(${hue} 45% 62%)`; x.fillRect(0, 0, w, h);
      x.fillStyle = `hsl(${hue + 40} 50% 36%)`; x.fillRect(0, h * 0.62, w, h * 0.38);
      x.fillStyle = '#f4efe2'; x.beginPath(); x.arc(w * 0.72, h * 0.3, h * 0.13, 0, 7); x.fill();
      return c.toDataURL(type).split(',')[1];
    };
    const b64 = (s) => btoa(s);
    const gif = 'R0lGODlhAQABAIAAAP///wAAACwAAAAAAQABAAACAkQBADs=';
    __mock.readable = {
      1: { name: 'holiday.jpg', mime: 'image/jpeg', data: draw('image/png', 960, 600, 200) },
      2: { name: 'chart.png', mime: 'image/png', data: b64('<html><body><img src=x onerror="parent.window.__pwned=\'chart\'"><script>parent.window.__pwned="script"</script></body></html>') },
      3: { name: 'invoice.pdf.exe', mime: 'application/octet-stream', data: b64('MZ..') },
      4: { name: 'logo.svg', mime: 'image/svg+xml', data: b64('<svg xmlns="http://www.w3.org/2000/svg" onload="parent.window.__pwned=1"/>') },
      5: { name: 'poster.png', mime: 'image/png', data: draw('image/png', 10, 10, 10) },
      6: { name: '<img src=x onerror=window.__pwned=2>.gif', mime: 'image/gif', data: gif },
      7: { name: 'scan.jpeg', mime: 'image/jpeg', data: draw('image/jpeg', 480, 640, 30) },
      8: { name: 'photo.webp', mime: 'image/webp', data: draw('image/webp', 640, 480, 140) },
      9: { name: 'a.png', mime: 'image/png', data: draw('image/png', 300, 300, 260) },
      10: { name: 'b.png', mime: 'image/png', data: draw('image/png', 300, 200, 320) },
      11: { name: 'c.png', mime: 'image/png', data: draw('image/png', 200, 300, 90) },
    };
    __mock.openAttsBy = { 95: atts };
    __mock.refresh = { messages: [{ id: 'me@example.com_95', folder: 'inbox', acct: 'me@example.com', acct_label: 'Example',
      from_name: 'Ann Lee', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com', subject: 'Weekend pictures', preview: 'Pictures from the weekend.',
      body: 'Pictures from the weekend.', ts: Date.now(), unread: true, starred: false, uid: 95, uidvalidity: 7, message_id: 'pics95@example.org',
      reply_to: '', truncated: false, attachments: atts, html: false }], flags: [], problems: [], skipped: [] };
    await serverSync('mail', true);
    go('inbox');
  }, atts);
  const reads = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'read_attachment').map(([, a]) => ({ index: a.index, confirmed: a.confirmed })));
  check((await reads()).length === 0, 'nothing is fetched for pictures until the message is opened');
  await pg.evaluate(() => { __mock.calls.length = 0; openMail('me@example.com_95'); });
  await pg.waitForFunction(() => document.querySelectorAll('#mail-detail [data-att]').length === 11
    && document.querySelectorAll('#mail-detail .md-pics img').length === 5 && !document.querySelector('#mail-detail .pic-wait'), null, { timeout: 8000 }).catch(() => {});
  const seen = await pg.evaluate(() => [...document.querySelectorAll('#mail-detail .md-pics img')].map((i) => ({
    alt: i.alt, kind: (i.src.match(/^data:image\/([a-z]+);base64,/) || [])[1] || i.src.slice(0, 30), lazy: i.loading, ok: i.complete && i.naturalWidth > 0,
    w: Math.round(i.getBoundingClientRect().width), h: Math.round(i.getBoundingClientRect().height) })));
  check(seen.length === 5 && seen[0].alt === 'holiday.jpg' && seen[0].kind === 'png',
    `a PNG named holiday.jpg and declared a JPEG shows as the PNG it is: ${JSON.stringify(seen[0])}`);
  check(JSON.stringify(seen.map((s) => s.kind)) === JSON.stringify(['png', 'gif', 'jpeg', 'webp', 'png']) && seen.every((s) => s.ok && s.lazy === 'lazy'),
    `a GIF, a JPEG and a WebP show too, each decoded, each loading="lazy": ${JSON.stringify(seen.map((s) => [s.alt, s.kind, s.ok, s.lazy]))}`);
  check(seen.every((s) => s.w <= 168 && s.h <= 168) && seen[0].w === 168,
    `a thumbnail fits the --thumb square (168 px): ${JSON.stringify(seen.map((s) => [s.w, s.h]))}`);
  const listed = await pg.evaluate(() => ({
    tiles: [...document.querySelectorAll('#mail-detail [data-pic]')].map((b) => +b.dataset.pic),
    cards: [...document.querySelectorAll('#mail-detail [data-att]')].map((b) => b.textContent.includes('chart.png') || b.textContent.includes('poster.png') || b.textContent.includes('logo.svg') || b.textContent.includes('invoice.pdf.exe')).filter(Boolean).length,
    strayImg: document.querySelectorAll('img[src="x"], #mail-detail img:not([src^="data:image/"])').length,
    pwned: window.__pwned || null,
    htmlShown: [...document.querySelectorAll('img')].some((i) => i.src.includes(btoa('<html>').slice(0, 6))),
  }));
  check(!listed.tiles.includes(1) && listed.cards === 4 && !listed.htmlShown && listed.strayImg === 0 && listed.pwned === null,
    `a web page named chart.png is listed and not shown, and no img is made from it or from any name: ${JSON.stringify(listed)}`);
  const r1 = await reads();
  const asked = r1.map((r) => r.index).sort((a, b) => a - b);
  check(JSON.stringify(asked) === JSON.stringify([1, 2, 6, 7, 8, 9]) && r1.every((r) => r.confirmed === false),
    `only the first six named as pictures are fetched, never the disguised program, the SVG or the one over 5 MB, and never "confirmed": ${JSON.stringify(r1)}`);
  const more = await pg.evaluate(() => document.querySelector('#md-pics-more')?.textContent || null);
  check(more === 'Show 2 more', `the rest wait behind a button: ${more}`);
  // The picture, large, in the app: Escape, the button and the backdrop close it.
  const before = pg.url();
  await pg.click('#mail-detail [data-pic="0"]');
  await pg.waitForSelector('#pic-ov.open', { timeout: 3000 }).catch(() => {});
  const big = await pg.evaluate(() => ({ open: document.querySelector('#pic-ov').classList.contains('open'), kind: (document.querySelector('#pic-big').src.match(/^data:image\/([a-z]+);base64,/) || [])[1] || null,
    alt: document.querySelector('#pic-big').alt, name: document.querySelector('#pic-name').textContent, focus: document.activeElement && document.activeElement.id,
    w: Math.round(document.querySelector('#pic-big').getBoundingClientRect().width) }));
  check(big.open && big.kind === 'png' && big.alt === 'holiday.jpg' && big.name === 'holiday.jpg' && big.focus === 'pic-close' && big.w > 168,
    `a click opens it large as the same data: picture, Close focused: ${JSON.stringify(big)}`);
  if (process.env.H11_SHOTS) { await pg.waitForTimeout(400); await pg.screenshot({ path: process.env.H11_SHOTS + '/h11-overlay-light.png' }); }
  await pg.keyboard.press('j');
  await pg.keyboard.press('Escape');
  const after = await pg.evaluate(() => ({ open: document.querySelector('#pic-ov').classList.contains('open'), src: document.querySelector('#pic-big').getAttribute('src'),
    msg: selMail, pane: document.querySelector('#mail-detail').classList.contains('open'), focus: document.activeElement && document.activeElement.dataset.pic }));
  check(!after.open && after.src === null && after.msg === 'me@example.com_95' && after.pane && after.focus === '0',
    `Escape closes the picture and only the picture (j did nothing behind it), and the keyboard goes back to the thumbnail: ${JSON.stringify(after)}`);
  await pg.click('#mail-detail [data-pic="5"]');
  await pg.waitForSelector('#pic-ov.open', { timeout: 3000 }).catch(() => {});
  const gifAlt = await pg.evaluate(() => document.querySelector('#pic-big').alt);
  await pg.click('#pic-close');
  const byButton = await pg.evaluate(() => document.querySelector('#pic-ov').classList.contains('open'));
  await pg.click('#mail-detail [data-pic="6"]');
  await pg.mouse.click(5, 5);
  const byBackdrop = await pg.evaluate(() => document.querySelector('#pic-ov').classList.contains('open'));
  check(gifAlt === '<img src=x onerror=window.__pwned=2>.gif' && !byButton && !byBackdrop && pg.url() === before && pg.context().pages().length === 1,
    `Close and the backdrop close it too; no navigation, no window, and a name like markup is only text: ${JSON.stringify({ gifAlt, byButton, byBackdrop, url: pg.url() === before, pages: pg.context().pages().length })}`);
  await pg.click('#md-pics-more');
  await pg.waitForFunction(() => document.querySelectorAll('#mail-detail .md-pics img').length === 7, null, { timeout: 5000 }).catch(() => {});
  const r2 = await reads();
  const shownAll = await pg.evaluate(() => ({ imgs: document.querySelectorAll('#mail-detail .md-pics img').length, more: !!document.querySelector('#md-pics-more') }));
  check(shownAll.imgs === 7 && !shownAll.more && JSON.stringify(r2.slice(6).map((r) => r.index)) === JSON.stringify([10, 11]),
    `"Show 2 more" fetches and shows the other two, and only them: ${JSON.stringify({ shownAll, r2: r2.map((r) => r.index) })}`);
  // Open again: kept for the session, nothing fetched twice.
  await pg.evaluate(() => { __mock.calls.length = 0; openMail('me@example.com_95'); });
  await pg.waitForTimeout(150);
  const again = await pg.evaluate(() => ({ imgs: document.querySelectorAll('#mail-detail .md-pics img').length, reads: __mock.calls.filter(([c]) => c === 'read_attachment').length }));
  check(again.imgs === 7 && again.reads === 0, `opening the message again shows them without fetching: ${JSON.stringify(again)}`);
  if (process.env.H11_SHOTS) {
    await pg.setViewportSize({ width: 1280, height: 900 });
    await pg.evaluate(() => { PIC_MORE.delete('me@example.com_95'); openMail('me@example.com_95'); document.querySelector('#md-body').scrollIntoView({ block: 'start' }); });
    await pg.waitForTimeout(400);
    await pg.screenshot({ path: process.env.H11_SHOTS + '/h11-message-light.png' });
    await pg.emulateMedia({ colorScheme: 'dark' });
    await pg.waitForTimeout(400);
    await pg.screenshot({ path: process.env.H11_SHOTS + '/h11-message-dark.png' });
    await pg.click('#mail-detail [data-pic="6"]');
    await pg.waitForTimeout(400);
    await pg.screenshot({ path: process.env.H11_SHOTS + '/h11-overlay-dark.png' });
    await pg.keyboard.press('Escape');
    await pg.emulateMedia({ colorScheme: 'light' });
  }
  // A read that fails leaves the file listed and is tried again next time.
  await pg.evaluate(() => {
    const m = S.messages.find((x) => x.uid === 95);
    [...PICS.keys()].forEach((k) => PICS.delete(k)); PIC_MORE.clear();
    __mock.readFails = true; __mock.calls.length = 0; openMail(m.id);
  });
  await pg.waitForFunction(() => !document.querySelector('#mail-detail .md-pics [data-pic]'), null, { timeout: 5000 }).catch(() => {});
  const failed = await pg.evaluate(() => ({ tiles: document.querySelectorAll('#mail-detail [data-pic]').length, cards: document.querySelectorAll('#mail-detail [data-att]').length,
    reads: __mock.calls.filter(([c]) => c === 'read_attachment').length, kept: PICS.size }));
  check(failed.tiles === 0 && failed.cards === 11 && failed.reads === 6 && failed.kept === 0,
    `a read that fails drops the thumbnail, keeps the file listed, and remembers nothing: ${JSON.stringify(failed)}`);
  await pg.evaluate(() => { __mock.readFails = false; });
  await pg.close();
}

console.log('\n— Unsubscribe: the link question, or a message to send —');
{
  /* H7: a list's List-Unsubscribe, as Rust sends it (a web address checked
     as a link, a mailto: rebuilt). Inbox mail only. */
  const pg = await open(true);
  const mk = (who, folder, uid, extra) => Object.assign({ id: who + '_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: who, acct_label: who,
    from_name: 'News', from_addr: 'news@list.example', to_name: '', to_addr: who, to_all: [who], cc: [], in_reply_to: '', subject: 'Issue ' + uid, preview: 'p', body: 'b',
    ts: Date.now() - uid * 1000, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'u' + uid + '@list.example', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  const both = { https: 'https://list.example/u/81?t=x', mailto: 'mailto:leave%40list.example?subject=leave%2081' };
  await pg.evaluate(async (msgs) => {
    __mock.mailboxes.push({ email: 'work@example.net', host: 'imap.example.net', port: 993, label: 'Work' });
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail');
    /* Stored before the field: nothing to go on. */
    S.messages.push({ id: 'old-u', ch: 'email', prov: 'imap', acct: S.messages[0].acct, mailbox: 'me@example.com', fromName: 'News', fromAddr: 'news@list.example', subj: 'Stored before', prev: 'p', body: 'b', ts: 1, unread: false, starred: false, atts: [], uid: 1, uidvalidity: 7, bodyV: 2 });
  }, [mk('me@example.com', 'inbox', 81, { unsubscribe: both }),
    mk('work@example.net', 'inbox', 82, { unsubscribe: { https: null, mailto: 'mailto:leave%40list.example?subject=unsubscribe%20me&body=Please%20remove%0Ame%20now' } }),
    mk('me@example.com', 'sent', 83, { unsubscribe: both }), mk('me@example.com', 'junk', 84, { unsubscribe: both }),
    mk('me@example.com', 'drafts', 85, { unsubscribe: both }), mk('me@example.com', 'archive', 86, { unsubscribe: both }),
    mk('me@example.com', 'inbox', 87)]);
  /* null when the message is not held at all, so "no button" means a pane was drawn without one. */
  const shown = (id) => pg.evaluate((id) => { if (!S.messages.some((m) => m.id === id)) return null; openMail(id); return !!document.querySelector('#md-unsub'); }, id);
  check((await shown('me@example.com_81')) && (await shown('work@example.net_82')), 'inbox mail from a list offers Unsubscribe, a web address or a mailto alike');
  const none = [];
  for (const id of ['me@example.com_sent_83', 'me@example.com_junk_84', 'me@example.com_drafts_85', 'me@example.com_archive_86', 'me@example.com_87', 'old-u']) if ((await shown(id)) !== false) none.push(id);
  check(none.length === 0, `none on Sent, Spam, Drafts or archived mail, mail with no header, or mail stored before it: ${JSON.stringify(none)}`);

  await pg.evaluate(() => { openMail('me@example.com_81'); __mock.calls = []; __mock.opened = []; });
  await pg.click('#md-unsub');
  let st = await pg.evaluate(() => ({ open: document.querySelector('#link-ov').classList.contains('open'), host: document.querySelector('#link-host').textContent,
    url: document.querySelector('#link-url').textContent, calls: __mock.calls.map(([c]) => c), composing: document.querySelector('#compose-ov').classList.contains('open') }));
  check(st.open && st.host === 'list.example' && st.url === 'https://list.example/u/81?t=x' && st.calls.length === 0 && !st.composing,
    `a web address asks first, naming the host, and nothing is opened or sent yet: ${JSON.stringify(st)}`);
  await pg.click('#link-cancel');
  check(await pg.evaluate(() => !document.querySelector('#link-ov').classList.contains('open') && __mock.opened.length === 0 && __mock.calls.length === 0), 'Cancel opens nothing');
  await pg.click('#md-unsub');
  await pg.click('#link-open');
  await pg.waitForTimeout(200);
  st = await pg.evaluate(() => ({ opened: __mock.opened, calls: __mock.calls.map(([c]) => c) }));
  check(JSON.stringify(st.opened) === '["https://list.example/u/81?t=x"]' && st.calls.join() === 'open_link',
    `Open in browser hands exactly that address to the browser, and RATA requests nothing itself: ${JSON.stringify(st)}`);

  await pg.evaluate(() => { openMail('work@example.net_82'); __mock.calls = []; __mock.opened = []; });
  await pg.click('#md-unsub');
  await pg.waitForTimeout(200);
  const c = await pg.evaluate(() => ({ open: document.querySelector('#compose-ov').classList.contains('open'), to: document.querySelector('#cmp-to').value,
    subj: document.querySelector('#cmp-subj').value, body: document.querySelector('#cmp-body').value, from: document.querySelector('#cmp-from').value,
    want: (S.linked.find((l) => l.label === 'work@example.net') || {}).id, asked: document.querySelector('#link-ov').classList.contains('open'), calls: __mock.calls.map(([x]) => x) }));
  check(c.open && c.to === 'leave@list.example' && c.subj === 'unsubscribe me' && c.body === 'Please remove\nme now' && c.from === c.want && !c.asked && c.calls.length === 0,
    `a mailto opens the composer with the address, subject and body the header gave, from the mailbox the list wrote to, and sends nothing: ${JSON.stringify(c)}`);
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  const sent = await pg.evaluate(() => __mock.calls.filter(([x]) => x === 'send_mail').map(([, a]) => a.draft)[0]);
  check(sent && sent.to === 'leave@list.example' && sent.subject === 'unsubscribe me' && sent.body === 'Please remove\nme now' && sent.from === 'work@example.net',
    `only Send sends it, as it was shown: ${JSON.stringify(sent)}`);
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

console.log('\n— the licence renews while RATA is open (BUG-L) —');
{
  /* A copy started more than a week before its licence expires, and left
     open past it: the licence used to renew only at launch, so every refresh
     answered "unlicensed", the quiet refresh dropped it, and mail stopped with
     no word. Now a refresh that answers "unlicensed" renews it and reads
     again, with no relaunch. */
  const DAY = 86400, T0 = 1_800_000_000;
  const genuine = () => ({ 'v1.first.sig': T0 + 8 * DAY, 'v1.second.sig': T0 + 60 * DAY, 'v1.third.sig': T0 + 120 * DAY });
  const NEXT = { 'v1.first.sig': 'v1.second.sig', 'v1.second.sig': 'v1.third.sig' };
  let reach = true;
  const renewal = (b) => !reach ? { abort: true }
    : NEXT[b.licence] ? { body: { licensed: true, licence: NEXT[b.licence], message: 'Renewed.' } } : { status: 500, body: { error: 'x' } };
  const mk = (uid) => ({ id: 'me@example.com_' + uid, folder: 'inbox', acct: 'me@example.com', acct_label: 'Example', from_name: 'Ann', from_addr: 'ann@example.org',
    to_name: '', to_addr: 'me@example.com', subject: 'After renewal ' + uid, preview: 'p', body: 'b', ts: Date.now(), unread: true, starred: false, uid, uidvalidity: 7,
    message_id: 'r' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false });
  const state = (pg) => pg.evaluate(() => ({ disk: __mock.lic.token, licensed: !!(LIC && LIC.licensed), token: LIC && LIC.token,
    refreshes: __mock.calls.filter(([c]) => c === 'refresh_mail').length, box: !!document.getElementById('rata-licence'),
    why: document.querySelector('#rata-licence-why')?.textContent || null, err: document.querySelector('#rata-licence-error')?.textContent || '',
    retry: !!document.querySelector('#rata-licence-retry') && !document.querySelector('#rata-licence-retry').hidden }));

  let pg = await open(true, { lic: { token: 'v1.first.sig', now: T0, genuine: genuine() }, renew: renewal });
  let st = await state(pg);
  check(pg.__renewals === 0 && st.licensed && !st.box, `eight days before expiry nothing is renewed at launch: ${JSON.stringify({ renewals: pg.__renewals, ...st })}`);
  /* The licence expires while RATA is open. */
  await pg.evaluate(async () => { __mock.lic.now = __mock.lic.genuine['v1.first.sig'] + 3600; __mock.calls = []; await autoSync(); });
  st = await state(pg);
  check(pg.__renewals === 1 && st.refreshes === 2 && st.disk === 'v1.second.sig' && st.licensed && st.token === 'v1.second.sig' && !st.box,
    `a refresh that answers "unlicensed" renews the licence and reads again, with no relaunch: ${JSON.stringify({ renewals: pg.__renewals, ...st })}`);
  /* And refreshes carry on: the next brings new mail. */
  const got = await pg.evaluate(async (m) => {
    __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [] };
    await autoSync(); __mock.refresh = null;
    return S.messages.some((x) => x.subj === 'After renewal 5');
  }, mk(5));
  check(got, 'the next refresh brings mail as before');
  await pg.close();

  /* Every six hours (shortened here): a licence that comes into its last
     week while RATA is open renews with no refresh involved, and the page
     hears the new one. Once renewed, it is not asked again. */
  pg = await open(true, { lic: { token: 'v1.first.sig', now: T0, genuine: genuine(), every: 300 }, renew: renewal });
  await pg.evaluate((t) => { __mock.lic.now = t; __mock.calls = []; }, T0 + 2 * DAY);
  await pg.waitForFunction(() => LIC && LIC.token === 'v1.second.sig', null, { timeout: 5000 }).catch(() => {});
  await pg.waitForTimeout(1200);
  st = await state(pg);
  check(pg.__renewals === 1 && st.disk === 'v1.second.sig' && st.token === 'v1.second.sig' && st.licensed && st.refreshes === 0,
    `in its last week the timer renews it, once, and the page hears the new licence: ${JSON.stringify({ renewals: pg.__renewals, ...st })}`);
  await pg.close();

  /* Offline when it lapses: the licence box says so, the timer stops
     refreshing, and the connection coming back renews it and mail resumes.
     Try again does the same by hand. */
  reach = false;
  pg = await open(true, { lic: { token: 'v1.first.sig', now: T0, genuine: genuine() }, renew: renewal });
  await pg.evaluate(async () => { __mock.lic.now = __mock.lic.genuine['v1.first.sig'] + 3600; await autoSync(); });
  st = await state(pg);
  const toastsNow = await toasts(pg);
  check(st.box && !st.licensed && st.retry && /needs refreshing/.test(st.why) && pg.__renewals === 1 && !toastsNow.length,
    `unreachable when it lapses: the licence box says so, with Try again, and no toast over it: ${JSON.stringify({ renewals: pg.__renewals, toasts: toastsNow, ...st })}`);
  await pg.evaluate(async () => { __mock.calls = []; LAST_TRY.clear(); await autoTick(); });
  st = await state(pg);
  check(st.refreshes === 0, `while unlicensed the timer reads no mail: ${st.refreshes}`);
  reach = true;
  await pg.evaluate(() => { __mock.calls = []; window.dispatchEvent(new Event('online')); });
  await pg.waitForFunction(() => !document.getElementById('rata-licence') && __mock.calls.some(([c]) => c === 'refresh_mail'), null, { timeout: 5000 }).catch(() => {});
  st = await state(pg);
  check(!st.box && st.licensed && st.disk === 'v1.second.sig' && st.refreshes >= 1,
    `the connection coming back renews it, closes the box, and mail resumes: ${JSON.stringify({ renewals: pg.__renewals, ...st })}`);
  /* It lapses again, still unreachable; Try again says it could not reach
     mailrata.org, then works once it can. */
  reach = false;
  await pg.evaluate(async () => { __mock.lic.now = __mock.lic.genuine['v1.second.sig'] + 3600; await autoSync(); });
  await pg.click('#rata-licence-retry');
  await pg.waitForFunction(() => /could not reach mailrata\.org/.test(document.querySelector('#rata-licence-error')?.textContent || ''), null, { timeout: 5000 }).catch(() => {});
  st = await state(pg);
  check(st.box && /could not reach mailrata\.org/.test(st.err), `Try again while unreachable says so: ${JSON.stringify(st.err)}`);
  reach = true;
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#rata-licence-retry');
  await pg.waitForFunction(() => !document.getElementById('rata-licence') && __mock.calls.some(([c]) => c === 'refresh_mail'), null, { timeout: 5000 }).catch(() => {});
  st = await state(pg);
  check(!st.box && st.licensed && st.disk === 'v1.third.sig' && st.refreshes >= 1, `and Try again renews it once it can, and mail resumes: ${JSON.stringify(st)}`);
  await pg.close();

  /* The licence box itself, on a licence that expired and can still renew.
     A junk paste is refused and the licence kept (Rust's rule, mocked here
     as core.rs has it), and the box tries to renew, saying what it kept. An
     empty box with a renewable licence renews too. */
  reach = false;
  pg = await open(false, { lic: { token: 'v1.first.sig', now: T0 + 13 * DAY, genuine: genuine() }, renew: renewal });
  await pg.waitForSelector('#rata-licence', { timeout: 5000 }).catch(() => {});
  const before = pg.__renewals;
  await pg.fill('#rata-licence-input', 'hello there');
  await pg.click('#rata-licence-save');
  await pg.waitForFunction(() => /kept the licence/.test(document.querySelector('#rata-licence-error')?.textContent || ''), null, { timeout: 5000 }).catch(() => {});
  st = await state(pg);
  check(st.box && st.disk === 'v1.first.sig' && /kept the licence it already has/.test(st.err) && pg.__renewals === before + 1,
    `a junk paste over a renewable licence keeps it, says so, and tries to renew it: ${JSON.stringify({ renewals: pg.__renewals - before, ...st })}`);
  reach = true;
  await pg.fill('#rata-licence-input', '');
  await pg.click('#rata-licence-save');
  await pg.waitForFunction(() => !document.getElementById('rata-licence'), null, { timeout: 5000 }).catch(() => {});
  st = await state(pg);
  check(!st.box && st.licensed && st.disk === 'v1.second.sig', `Use this licence with the box empty renews the licence it has: ${JSON.stringify(st)}`);
  await pg.close();

  /* A key wrapped by a mail client: line breaks and spaces inside it are
     taken out before it is used. */
  pg = await open(false, { lic: { token: null, now: T0, genuine: genuine() } });
  await pg.waitForSelector('#rata-licence', { timeout: 5000 }).catch(() => {});
  await pg.evaluate(() => { document.querySelector('#rata-licence-input').value = '  v1.sec\r\nond.\n s ig '; });
  await Promise.all([pg.waitForNavigation({ timeout: 5000 }).catch(() => {}), pg.click('#rata-licence-save')]);
  const used = await pg.evaluate(() => sessionStorage.getItem('rata_set_licence'));
  check(used === JSON.stringify('v1.second.sig'), `a wrapped key is used without its white space, and RATA opens licensed: ${used}`);
  await pg.close();

  /* The AI relay says "expired" (this computer's clock is behind, say): the
     licence is renewed once and the request asked again with the new one,
     never more than once. */
  let aiExpired = (b) => b.licence === 'v1.first.sig';
  pg = await open(true, { lic: { token: 'v1.first.sig', now: T0, genuine: genuine() }, renew: renewal,
    ai: (b) => aiExpired(b) ? { status: 401, body: { error: 'Your licence needs renewing — RATA does this itself when it is online.', reason: 'expired' } }
      : { body: { text: 'A short summary.', cut: false, used: 0.1 } } });
  await pg.evaluate(async (m) => {
    S.settings.aiOk = true;
    __mock.refresh = { messages: [m], flags: [], problems: [], skipped: [] };
    await serverSync('mail', true); __mock.refresh = null;
    go('inbox'); openMail(m.id);
    await new Promise((r) => setTimeout(r, 300));
    OPENED.set(m.id, { text: 'Dear reader, the news of the week.', truncated: false, attachments: [] });
    await summarizeMessage(m.id);
  }, mk(7));
  let asked = pg.__ai.map((raw) => JSON.parse(raw).licence);
  let shown = await pg.evaluate(() => document.querySelector('#md-summary .md-ai-text')?.textContent);
  check(pg.__renewals === 1 && asked.join(',') === 'v1.first.sig,v1.second.sig' && shown === 'A short summary.',
    `an "expired" answer from the relay renews the licence and asks again with the new one: ${JSON.stringify({ renewals: pg.__renewals, asked, shown })}`);
  /* A relay that still says "expired" is asked once more, not again and again. */
  aiExpired = () => true;
  pg.__ai.length = 0;
  await pg.evaluate(async () => { await summarizeMessage('me@example.com_7'); });
  asked = pg.__ai.map((raw) => JSON.parse(raw).licence);
  check(asked.join(',') === 'v1.second.sig,v1.third.sig', `and only once: ${JSON.stringify(asked)}`);
  await pg.close();
}

console.log('\n— the AI relay is sent the start of a long message, never all of it —');
{
  /* BUG-A: an opened message runs to 400 000 characters, and the relay
     refuses any request over 80 000 (LIMITS.body) before it cuts the text to
     12 000. The page cuts to exactly 12 000 first (what the privacy page
     promises) and knows for itself that it cut. */
  const RELAY_BODY = 80_000, PAGE_TEXT = 12_000;
  let answer = { body: { text: 'A short summary.', cut: true, used: 0.1 } };
  const pg = await open(true, { ai: () => answer });
  const mk = (uid, extra) => Object.assign({ id: 'me@example.com_' + uid, folder: 'inbox', acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com', subject: 'Newsletter ' + uid, preview: 'p', body: 'b',
    ts: Date.now() - uid * 60_000, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'ai' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate(async (msgs) => {
    S.settings.aiOk = true;
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail', true);
    __mock.refresh = null;
  }, [mk(1)]);
  const sent = () => { const raw = pg.__ai.at(-1) || '{}'; return { len: raw.length, text: (JSON.parse(raw).text || '').length, task: JSON.parse(raw).task }; };
  /* The message opened in full: what OPENED holds is what Summarize and
     Translate send. 100 000 characters, a long newsletter, with quotes and
     line breaks that JSON spells as two and control characters as six. */
  await pg.evaluate(async () => {
    const id = 'me@example.com_1';
    go('inbox'); openMail(id);
    await new Promise((r) => setTimeout(r, 300));
    OPENED.set(id, { text: ('Dear reader, "news" of the week.\n\u0001\u0002 ').repeat(3000).slice(0, 100_000), truncated: false, attachments: [] });
    await summarizeMessage(id);
  });
  let s = sent();
  const summary = await pg.evaluate(() => document.querySelector('#md-summary .md-ai-text')?.textContent);
  check(s.task === 'summarize' && s.text === PAGE_TEXT && s.len < RELAY_BODY && summary === 'A short summary.',
    `Summarize on a 100 000-character message sends ${s.text} characters (${s.len} in all, under ${RELAY_BODY}), and shows the answer: ${JSON.stringify(summary)}`);
  answer = { body: { text: 'Liebe Leser', cut: false, used: 0.1 } };
  await pg.evaluate(() => translateMessage('me@example.com_1'));
  s = sent();
  const bar = await pg.evaluate(() => document.querySelector('.md-trbar span')?.textContent);
  check(s.task === 'translate' && s.text === PAGE_TEXT && s.len < RELAY_BODY && /only the start of this long message/.test(bar || ''),
    `Translate sends ${s.text} characters (${s.len} in all) and says only the start was translated, though the relay did not: ${JSON.stringify(bar)}`);

  /* A briefing the relay could not finish keeps every flag, and says why.
     Twenty-five messages of quotes would be over 80 000 characters as JSON;
     the oldest are left out until the request fits. */
  const many = Array.from({ length: 25 }, (_, i) => mk(10 + i, { body: '"'.repeat(3000), subject: '"'.repeat(400), preview: '"' }));
  await pg.evaluate(async (msgs) => {
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail', true);
    __mock.refresh = null;
    for (const m of S.messages) if (m.uid >= 10) m.flag = { task: 'Old flag ' + m.uid, due: null, important: false, at: 1 };
  }, many);
  answer = { status: 502, body: { error: 'The briefing was cut short. Try again.', reason: 'cut', used: 0.1 } };
  await pg.evaluate(async () => { openAssist(); await runAIBrief(); });
  const raw = pg.__ai.at(-1) || '{}', asked = JSON.parse(raw);
  const after = await pg.evaluate(() => ({ flags: S.messages.filter((m) => m.uid >= 10 && m.flag && /^Old flag/.test(m.flag.task)).length,
    said: document.querySelector('#as-out')?.textContent || '' }));
  check(asked.task === 'tasks' && raw.length < RELAY_BODY && asked.messages.length >= 15 && asked.messages.length < 26 && asked.messages.every((m) => m.text.length <= 1500),
    `the briefing request fits (${raw.length} characters, ${asked.messages.length} messages)`);
  check(after.flags === 25 && /The briefing was cut short\. Try again\./.test(after.said) && /Flags from earlier briefings are kept/.test(after.said),
    `a briefing cut short keeps all 25 flags and says so: ${JSON.stringify({ flags: after.flags, said: after.said.slice(0, 160) })}`);
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

console.log('\n— Settings → Help & diagnostics —');
{
  /* H8: Copy diagnostics shows the block Rust made and puts it on the
     clipboard; where the webview refuses the clipboard, the block is shown
     selected with how to copy it. Help and Report a bug go to the browser
     through open_link, with exactly their addresses. */
  const pg = await open(true);
  await pg.evaluate(() => {
    go('set'); renderSettings(); __mock.calls = []; __mock.opened = [];
    window.__clip = null;
    const ok = async (t) => { window.__clip = t; };
    try { navigator.clipboard.writeText = ok; } catch { Object.defineProperty(navigator, 'clipboard', { value: { writeText: ok }, configurable: true }); }
  });
  const shown = await pg.evaluate(() => getComputedStyle(document.querySelector('#diag-row')).display !== 'none');
  await pg.click('#diag-copy');
  await pg.waitForFunction(() => window.__clip !== null, null, { timeout: 3000 }).catch(() => {});
  let d = await pg.evaluate(() => ({ calls: __mock.calls.filter(([c]) => c === 'diagnostics').length, clip: window.__clip, text: document.querySelector('#diag-text').value,
    visible: getComputedStyle(document.querySelector('#diag-text')).display !== 'none', hint: document.querySelector('#diag-hint').textContent }));
  const want = await pg.evaluate(() => __mock.diag || null) || 'RATA diagnostics';
  check(shown && d.calls === 1 && d.visible && d.text.startsWith(want) && d.clip === d.text && /Paste this into your bug report/.test(d.hint),
    `Copy diagnostics copies the block Rust made and shows it: ${JSON.stringify({ shown, calls: d.calls, visible: d.visible, hint: d.hint })}`);
  check(!d.text.includes('@') && !d.clip.includes('@'), 'no @ appears in the block shown or copied');
  /* A webview that refuses the clipboard: the block is there, selected. */
  await pg.evaluate(() => {
    __mock.diag = 'RATA diagnostics\nVersion: 0.1.42 (release build)\nMailboxes: 2\n\nMailbox 1: Gmail\n  Last error: 29 September 2026 14:03 UTC, 2 min ago (refresh, net): Mailbox 1 did not sync';
    const no = async () => { throw new DOMException('Write permission denied.', 'NotAllowedError'); };
    try { navigator.clipboard.writeText = no; } catch { Object.defineProperty(navigator, 'clipboard', { value: { writeText: no }, configurable: true }); }
    document.querySelector('#diag-text').value = '';
  });
  await pg.click('#diag-copy');
  await pg.waitForFunction(() => document.querySelector('#diag-text').value !== '', null, { timeout: 3000 }).catch(() => {});
  d = await pg.evaluate(() => {
    const ta = document.querySelector('#diag-text');
    return { text: ta.value, focused: document.activeElement === ta, selected: ta.selectionStart === 0 && ta.selectionEnd === ta.value.length && ta.value.length > 0,
      readOnly: ta.readOnly, hint: document.querySelector('#diag-hint').textContent };
  });
  check(d.text === await pg.evaluate(() => __mock.diag) && d.focused && d.selected && d.readOnly && /Copy it with Ctrl\+C or Cmd\+C/.test(d.hint) && !d.text.includes('@'),
    `a refused clipboard shows the block read-only and selected, saying how to copy it: ${JSON.stringify({ focused: d.focused, selected: d.selected, readOnly: d.readOnly, hint: d.hint })}`);
  /* Help and Report a bug. */
  await pg.click('#help-link');
  await pg.waitForFunction(() => (__mock.opened || []).length === 1, null, { timeout: 3000 }).catch(() => {});
  await pg.click('#bug-link');
  await pg.waitForFunction(() => (__mock.opened || []).length === 2, null, { timeout: 3000 }).catch(() => {});
  const opened = await pg.evaluate(() => __mock.opened);
  check(opened[0] === 'https://mailrata.org/help', `Help asks open_link for exactly https://mailrata.org/help: ${JSON.stringify(opened)}`);
  check(opened[1] === 'https://github.com/Noallrightokay/center-point-inbox/issues/new?template=beta-bug.md' && opened.length === 2,
    `Report a bug opens the Beta bug template, once: ${JSON.stringify(opened)}`);
  await pg.close();
}
{
  /* On the website there is nothing to diagnose: Help and Report a bug only. */
  const pg = await browser.newPage();
  pg.on('pageerror', (e) => { console.log('  PAGE ERROR: ' + e.message); fails++; });
  await pg.addInitScript(() => { localStorage.setItem('centra_session', JSON.stringify({ uid: 'local_t', email: 'me@example.com', mode: 'local' })); });
  await pg.goto(B + '/app.html');
  await pg.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function', null, { timeout: 20000 });
  const web = await pg.evaluate(() => {
    go('set'); renderSettings();
    const vis = (s) => { const el = document.querySelector(s); return !!el && getComputedStyle(el).display !== 'none' && el.getBoundingClientRect().height > 0; };
    return { native: !!window.__RATA_NATIVE__, diag: vis('#diag-row'), help: vis('#help-link'), bug: vis('#bug-link'),
      helpHref: document.querySelector('#help-link').getAttribute('href') };
  });
  check(!web.native && !web.diag && web.help && web.bug && web.helpHref === 'https://mailrata.org/help',
    `on the website the section shows Help and Report a bug, and no Copy diagnostics: ${JSON.stringify(web)}`);
  await pg.close();
}

console.log('\n— a search can ask each mailbox’s server too (H6) —');
{
  const pg = await open(true);
  const mk = (who, uid, subject, extra) => Object.assign({ id: who + '_' + uid, folder: 'inbox', acct: who, acct_label: who,
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: who, subject, preview: 'p', body: 'The figures.',
    ts: Date.parse('2025-01-01T10:00:00Z') + uid * 60000, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'h' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate((held) => {
    __mock.mailboxes.push({ email: 'work@example.net', host: 'imap.example.net', port: 993, label: 'Work' });
    __mock.refresh = { messages: held, flags: [], problems: [], skipped: [] };
  }, [mk('me@example.com', 90, 'Lunch plans'), mk('work@example.net', 40, 'Rota')]);
  await pg.evaluate(() => serverSync('mail'));
  /* The server finds two old messages RATA has never held, and says twelve
     matched: the invoice word is in their text only. */
  await pg.evaluate((found) => {
    __mock.serverFound = { 'me@example.com': { messages: found, matched: 12 } };
    __mock.calls = [];
  }, [mk('me@example.com', 7, 'Old statement', { body: 'Your invoice is attached.' }), mk('me@example.com', 5, 'Older statement', { body: 'The invoice for March.' })]);
  await pg.evaluate(() => go('search'));
  await pg.fill('#search-input', 'invoice');
  await pg.click('#search-go');
  await pg.waitForSelector('#srv-bar', { timeout: 5000 }).catch(() => {});
  let bar = await pg.evaluate(() => {
    const b = document.querySelector('#srv-bar');
    return b ? { rows: [...b.querySelectorAll('.srv-row')].map((r) => r.textContent), all: (b.querySelector('[data-srv="*"]') || {}).textContent || '',
      results: [...document.querySelectorAll('#res-pane .res-row')].filter((r) => /statement/.test(r.textContent)).length, asked: __mock.calls.filter(([c]) => c === 'search_mail').length } : null;
  });
  check(bar && bar.rows.length === 2 && bar.rows.every((r) => /Search on the server$/.test(r)) && /me@example\.com/.test(bar.rows[0] + bar.rows[1]) && /work@example\.net/.test(bar.rows[0] + bar.rows[1]),
    `a search that ran here offers Search on the server for each linked mailbox: ${JSON.stringify(bar)}`);
  check(bar && bar.all === 'Search all 2 on the server' && bar.results === 0 && bar.asked === 0, `and all at once, and asks no server until pressed: ${JSON.stringify(bar)}`);
  await pg.click('#srv-bar [data-srv="me@example.com"]');
  await pg.waitForFunction(() => /found/.test((document.querySelector('#srv-bar [data-srv="me@example.com"]') || {}).textContent || ''), null, { timeout: 5000 }).catch(() => {});
  const after = await pg.evaluate(() => {
    const btn = document.querySelector('#srv-bar [data-srv="me@example.com"]');
    const rows = [...document.querySelectorAll('#res-pane .res-row')].map((r) => r.textContent).filter((t) => /statement/.test(t));
    const held = S.messages.filter((m) => m.id === 'me@example.com_7' || m.id === 'me@example.com_5');
    return { said: btn && btn.textContent, disabled: btn && btn.disabled, rows,
      asked: __mock.calls.filter(([c]) => c === 'search_mail').map(([, a]) => a),
      held: held.map((m) => ({ id: m.id, srv: !!m.srvFound, acct: m.acct, mailbox: m.mailbox })),
      oldest: (oldestRef('me@example.com', 'inbox') || {}).uid, known: heldKnown().filter((k) => k.email === 'me@example.com' && k.folder === 'inbox').map((k) => k.since) };
  });
  check(after.said === 'Newest 2 of 12 found' && after.disabled, `the button then says how many came: ${JSON.stringify({ said: after.said, disabled: after.disabled })}`);
  check(after.asked.length === 1 && after.asked[0].email === 'me@example.com' && after.asked[0].folder === 'inbox' && after.asked[0].query === 'invoice',
    `one mailbox's inbox is asked, with the words as typed: ${JSON.stringify(after.asked)}`);
  check(after.rows.length === 2 && after.rows.every((r) => /Old(er)? statement/.test(r) && /me@example\.com/.test(r) && /Found on the server/.test(r)),
    `what came is in the results, labelled with its mailbox and as found on the server: ${JSON.stringify(after.rows)}`);
  check(after.held.length === 2 && after.held.every((m) => m.srv && m.mailbox === 'me@example.com' && m.acct !== 'me@example.com'),
    `and stored like older mail, filed under its mailbox: ${JSON.stringify(after.held)}`);
  check(after.oldest === 90 && after.known.length === 1 && after.known[0] === 90,
    `neither Load older mail nor a refresh counts it, so nothing between is skipped: ${JSON.stringify({ oldest: after.oldest, known: after.known })}`);
  /* All at once asks only the mailbox not yet asked. */
  await pg.evaluate(() => { __mock.calls = []; });
  await pg.click('#srv-bar [data-srv="*"]');
  await pg.waitForFunction(() => /found/.test((document.querySelector('#srv-bar [data-srv="work@example.net"]') || {}).textContent || ''), null, { timeout: 5000 }).catch(() => {});
  const all = await pg.evaluate(() => ({ asked: __mock.calls.filter(([c]) => c === 'search_mail').map(([, a]) => a.email),
    said: (document.querySelector('#srv-bar [data-srv="work@example.net"]') || {}).textContent, allLeft: !!document.querySelector('#srv-bar [data-srv="*"]') }));
  check(all.asked.join() === 'work@example.net' && all.said === 'Nothing found on the server' && !all.allLeft,
    `Search all asks the mailboxes not yet asked, and says when nothing came: ${JSON.stringify(all)}`);
  /* Load older mail reaches the found messages: they become history. */
  await pg.evaluate(() => {
    __mock.inbox = [5, 7, 60, 90];
    __mock.mk = (u) => ({ id: 'me@example.com_' + u, folder: 'inbox', acct: 'me@example.com', acct_label: 'me@example.com', from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com',
      subject: 'Page ' + u, preview: 'p', body: 'b', ts: Date.parse('2025-01-01T10:00:00Z') + u * 60000, unread: false, starred: false, uid: u, uidvalidity: 7, message_id: 'h' + u + '@example.org', reply_to: '', truncated: false, attachments: [], html: false });
  });
  await pg.evaluate(() => loadOlder('me@example.com', true));
  const paged = await pg.evaluate(() => ({ ids: S.messages.filter((m) => m.mailbox === 'me@example.com' && m.folder === 'inbox').map((m) => m.uid).sort((a, b) => a - b),
    marked: S.messages.filter((m) => m.srvFound).map((m) => m.uid), subj7: (S.messages.find((m) => m.id === 'me@example.com_7') || {}).subj, oldest: (oldestRef('me@example.com', 'inbox') || {}).uid }));
  check(paged.ids.join() === '5,7,60,90' && paged.marked.length === 0 && paged.subj7 === 'Old statement' && paged.oldest === 5,
    `Load older mail pages from what it held, and takes the found ones in as history without doubling them: ${JSON.stringify(paged)}`);
  /* One the server found that arrived after the last refresh: a refresh
     that then brings it has read it, and from then on it counts. */
  await pg.evaluate((one) => { __mock.serverFound = { 'me@example.com': { messages: [one], matched: 1 } }; }, mk('me@example.com', 95, 'New statement', { body: 'Another invoice.' }));
  await pg.fill('#search-input', 'another invoice');
  await pg.click('#search-go');
  await pg.waitForSelector('#srv-bar [data-srv="me@example.com"]', { timeout: 5000 }).catch(() => {});
  await pg.click('#srv-bar [data-srv="me@example.com"]');
  await pg.waitForFunction(() => /found/.test((document.querySelector('#srv-bar [data-srv="me@example.com"]') || {}).textContent || ''), null, { timeout: 5000 }).catch(() => {});
  const before = await pg.evaluate(() => ({ marked: S.messages.filter((m) => m.srvFound).map((m) => m.uid), known: heldKnown().filter((k) => k.email === 'me@example.com' && k.folder === 'inbox').map((k) => k.since) }));
  await pg.evaluate((one) => { __mock.refresh = { messages: [one], flags: [], problems: [], skipped: [] }; }, mk('me@example.com', 95, 'New statement', { body: 'Another invoice.' }));
  await pg.evaluate(() => serverSync('mail'));
  const brought = await pg.evaluate(() => ({ marked: S.messages.filter((m) => m.srvFound).map((m) => m.uid), known: heldKnown().filter((k) => k.email === 'me@example.com' && k.folder === 'inbox').map((k) => k.since) }));
  check(before.marked.join() === '95' && before.known.join() === '90' && brought.marked.length === 0 && brought.known.join() === '95',
    `a found message newer than the last refresh counts once a refresh has brought it: ${JSON.stringify({ before, brought })}`);
  /* A refused search says why, beside its mailbox, and can be tried again. */
  await pg.evaluate(() => { __mock.searchFails = true; __mock.calls = []; });
  await pg.fill('#search-input', 'a "quoted" \\ word');
  await pg.click('#search-go');
  await pg.waitForSelector('#srv-bar [data-srv="me@example.com"]', { timeout: 5000 }).catch(() => {});
  const fresh = await pg.evaluate(() => document.querySelector('#srv-bar [data-srv="me@example.com"]').textContent);
  await pg.click('#srv-bar [data-srv="me@example.com"]');
  await pg.waitForSelector('#srv-bar .srv-err', { timeout: 5000 }).catch(() => {});
  const refused = await pg.evaluate(() => ({ err: (document.querySelector('#srv-bar .srv-err') || {}).textContent, said: document.querySelector('#srv-bar [data-srv="me@example.com"]').textContent,
    disabled: document.querySelector('#srv-bar [data-srv="me@example.com"]').disabled, query: (__mock.calls.find(([c]) => c === 'search_mail') || [, {}])[1].query }));
  check(fresh === 'Search on the server' && /Search is switched off/.test(refused.err || '') && refused.said === 'Search on the server again' && !refused.disabled,
    `a new query starts afresh, and a refusal is said beside its mailbox: ${JSON.stringify({ fresh, ...refused })}`);
  check(refused.query === 'a "quoted" \\ word', `the words go to Rust exactly as typed: ${JSON.stringify(refused.query)}`);
  await pg.close();
}
{
  /* The website reads no mail, so it has no server to ask. */
  const pg = await browser.newPage();
  pg.on('pageerror', (e) => { console.log('  PAGE ERROR: ' + e.message); fails++; });
  await pg.addInitScript(() => { localStorage.setItem('centra_session', JSON.stringify({ uid: 'local_t', email: 'me@example.com', mode: 'local' })); });
  await pg.goto(B + '/app.html');
  await pg.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function', null, { timeout: 20000 });
  await pg.evaluate(() => { S.linked.push({ id: 'lk_web', type: 'mail', label: 'me@example.com' }); go('search'); });
  await pg.fill('#search-input', 'invoice');
  await pg.click('#search-go');
  await pg.waitForFunction(() => !/Searching/.test(document.querySelector('#res-pane').textContent), null, { timeout: 5000 }).catch(() => {});
  check(await pg.evaluate(() => !window.__RATA_NATIVE__ && !document.querySelector('#srv-bar')), 'on the website a search offers no server search');
  await pg.close();
}

console.log('\n— Undo send: a message waits a few seconds before it goes (H9) —');
{
  const ctx = await browser.newContext();
  const pg = await open(true, { undo: true, context: ctx });
  const sends = () => pg.evaluate(() => __mock.calls.filter(([c]) => c === 'send_mail').map(([, a]) => a.draft));
  const trashes = () => pg.evaluate(() => __mock.calls.map(([c, a], i) => [i, c, a]).filter(([, c, a]) => c === 'change_messages' && a.action === 'trash').map(([i, , a]) => ({ i, folder: a.folder, uids: a.uids })));
  const order = () => pg.evaluate(() => __mock.calls.map(([c]) => c).filter((c) => c === 'send_mail' || c === 'change_messages'));
  const reset = () => pg.evaluate(() => { __mock.calls = []; window.__toasts = []; __mock.sendFails = false; });
  const outbox = () => pg.evaluate(() => JSON.parse(localStorage.getItem(OB_KEY) || '[]'));
  /* One second of the count is 100 ms here: 5 s is half a second. */
  await pg.evaluate(() => { window.__RATA_SEND_SECOND = 100; window.__RATA_DRAFT_EVERY = 60e3; });
  await pg.evaluate(() => go('set'));
  const setting = await pg.evaluate(() => ({ shown: getComputedStyle(document.querySelector('#undo-row')).display !== 'none', value: document.querySelector('#set-undo').value,
    options: [...document.querySelectorAll('#set-undo option')].map((o) => o.value).join() }));
  check(setting.shown && setting.value === '5' && setting.options === '0,5,10', `Settings offers 0, 5 or 10 seconds, 5 unless changed: ${JSON.stringify(setting)}`);
  await pg.evaluate(() => go('inbox'));

  /* A draft with everything in it: To, Cc, Bcc, subject, text with the
     signature, a picked file, a forward's attachment, the thread it answers,
     the Drafts copy it finishes, From and what it is. */
  const fill = (subj) => pg.evaluate((subj) => {
    const acct = S.linked.find((l) => l.type === 'mail').id;
    const rec = { id: 'me@example.com_drafts_90', folder: 'drafts', draft: true, prov: 'imap', acct, mailbox: 'me@example.com', uid: 90, uidvalidity: 7, subj, ts: Date.now(), unread: false, starred: false, ch: 'email' };
    S.messages = S.messages.filter((m) => m.id !== rec.id); S.messages.push(rec);
    newDraft(); openCompose();
    $('#cmp-to').value = 'bo@example.org'; showCc(true); $('#cmp-cc').value = 'cy@example.org'; showBcc(true); $('#cmp-bcc').value = 'di@example.org';
    $('#cmp-subj').value = subj; $('#cmp-body').value = 'Words for ' + subj + '\n\n-- \nAnn'; CMP_SIG = '-- \nAnn';
    CMP_FILES = [new File(['hello'], 'a.txt', { type: 'text/plain' })];
    CMP_FWD = { email: 'me@example.com', folder: 'inbox', uid: 5, uidvalidity: 7, atts: [{ i: 1, n: 'q.pdf', f: 'PDF', s: '1 KB' }] };
    REPLYING = { id: 'me@example.com_5', messageId: 'orig@example.org' };
    CMP_DRAFT = rec; DRAFT = { kind: 'reply', to: 'Bo' }; CMP_SRV.draftId = 'kept-draft-id';
    renderCmpFiles(); renderDraftHead();
    window.__srvWas = CMP_SRV; window.__fileWas = CMP_FILES[0];
  }, subj);
  const state = () => pg.evaluate(() => ({ open: document.querySelector('#compose-ov').classList.contains('open'), to: $('#cmp-to').value, cc: $('#cmp-cc').value, bcc: $('#cmp-bcc').value,
    ccOn: !$('#cmp-cc').hidden, bccOn: !$('#cmp-bcc').hidden, subj: $('#cmp-subj').value, body: $('#cmp-body').value, from: $('#cmp-from').value, sig: CMP_SIG,
    file: CMP_FILES.length === 1 && CMP_FILES[0] === window.__fileWas, fwd: JSON.stringify(CMP_FWD), replying: REPLYING && REPLYING.messageId, draft: CMP_DRAFT && CMP_DRAFT.id,
    kind: DRAFT.kind, srv: CMP_SRV === window.__srvWas, title: $('#cmp-title').textContent }));
  await fill('Undo me');
  const before = await state();
  await reset();
  await pg.click('#cmp-send');
  const toast1 = await pg.evaluate(() => ({ open: document.querySelector('#compose-ov').classList.contains('open'), shown: document.querySelector('#send-toast').classList.contains('show'),
    role: document.querySelector('#send-toast').getAttribute('role'), count: document.querySelector('#st-count').textContent, undo: !document.querySelector('#st-undo').hidden,
    say: document.querySelector('#st-say').textContent, sent: __mock.calls.filter(([c]) => c === 'send_mail').length }));
  check(!toast1.open && toast1.shown && toast1.role === 'status' && toast1.count === 'Sending in 5 s' && toast1.undo && toast1.sent === 0 && /Undo me/.test(toast1.say),
    `Send closes the composer and says "Sending in 5 s" with Undo, announced, and sends nothing yet: ${JSON.stringify(toast1)}`);
  const rec = (await outbox())[0];
  check(rec && rec.state === 'waiting' && rec.subj === 'Undo me', `the waiting message is kept on this computer at once: ${JSON.stringify(rec && { state: rec.state, subj: rec.subj })}`);
  /* The keyboard: Tab reaches Undo, Enter presses it. */
  await pg.keyboard.press('Tab');
  const focused = await pg.evaluate(() => document.activeElement && document.activeElement.id);
  await pg.keyboard.press('Enter');
  await pg.waitForTimeout(900);
  const after = await state();
  check(focused === 'st-undo', `Tab from the closed composer reaches Undo: ${focused}`);
  const same = Object.keys(before).filter((k) => before[k] !== after[k]);
  check(after.open && same.length === 0, `Undo puts the whole draft back exactly as it was: ${same.length ? JSON.stringify({ before, after }) : 'every field the same'}`);
  check((await sends()).length === 0 && (await trashes()).length === 0 && (await outbox()).length === 0,
    `Undo sends nothing, leaves the Drafts copy where it is, and clears the waiting record: ${JSON.stringify({ s: (await sends()).length, t: await trashes() })}`);
  check(await pg.evaluate(() => !document.querySelector('#send-toast').classList.contains('show') && document.querySelector('#st-undo').hidden), 'and the toast goes');

  /* The count ending sends it once, whole; the Drafts copy goes to the
     Trash only after. Shortcuts pressed meanwhile neither undo nor send. */
  await reset();
  await pg.click('#cmp-send');
  await pg.evaluate(() => document.querySelector('#send-toast').focus());
  await pg.keyboard.press('Enter');
  await pg.keyboard.press('Escape');
  await pg.keyboard.press('j');
  await pg.waitForTimeout(150);
  const early = { s: (await sends()).length, t: (await trashes()).length, waiting: await pg.evaluate(() => OUTBOX.length) };
  check(early.s === 0 && early.t === 0 && early.waiting === 1, `while it counts, Enter on the toast, Escape and j neither send nor undo: ${JSON.stringify(early)}`);
  await pg.waitForTimeout(900);
  let s = await sends();
  const t = await trashes();
  const o = await order();
  check(s.length === 1 && s[0].to === 'bo@example.org' && s[0].cc === 'cy@example.org' && s[0].bcc === 'di@example.org' && s[0].subject === 'Undo me'
    && s[0].body === 'Words for Undo me\n\n-- \nAnn' && s[0].inReplyTo === 'orig@example.org' && s[0].attachments.length === 1 && s[0].attachments[0].name === 'a.txt'
    && s[0].attachments[0].data === 'aGVsbG8=' && s[0].forward && s[0].forward.indexes.join() === '1' && s[0].forward.uid === 5,
    `the count ending sends it exactly once, with everything: ${JSON.stringify(s.map((x) => ({ ...x, attachments: x.attachments.map((a) => a.name) })))}`);
  check(t.length === 1 && t[0].folder === 'drafts' && t[0].uids.join() === '90' && o.indexOf('send_mail') < o.indexOf('change_messages'),
    `its copy in Drafts goes to the Trash after the send succeeded: ${JSON.stringify({ t, o })}`);
  check((await outbox()).length === 0 && (await toasts(pg)).includes('Sent from me@example.com'), 'the record is cleared and it says Sent');

  /* Two in a row: the second waits behind the first; both go, in order. */
  await reset();
  for (const subj of ['First', 'Second']) {
    await pg.evaluate((subj) => { newDraft(); openCompose(); $('#cmp-to').value = 'bo@example.org'; $('#cmp-subj').value = subj; $('#cmp-body').value = subj + ' words'; }, subj);
    await pg.click('#cmp-send');
  }
  const two = await pg.evaluate(() => ({ n: OUTBOX.length, say: document.querySelector('#st-say').textContent }));
  await pg.waitForTimeout(1500);
  s = await sends();
  check(two.n === 2 && /Second/.test(two.say) && s.map((x) => x.subject).join() === 'First,Second', `two sends in a row send two, in order: ${JSON.stringify({ two, sent: s.map((x) => x.subject) })}`);

  /* A failure after the count says why and puts the draft back whole;
     the Drafts copy stays. */
  await fill('Will fail');
  await reset();
  await pg.evaluate(() => { __mock.sendFails = true; });
  await pg.click('#cmp-send');
  await pg.waitForTimeout(900);
  const failed = await state();
  const ft = await toasts(pg);
  check((await sends()).length === 1 && ft.includes('Send failed — smtp.example.com refused the message') && failed.open && failed.subj === 'Will fail' && failed.file && failed.draft === 'me@example.com_drafts_90'
    && (await trashes()).length === 0 && (await outbox()).length === 0,
    `a send that fails after the count says why, keeps the draft and its Drafts copy: ${JSON.stringify({ ft, failed })}`);
  await pg.evaluate(() => { __mock.sendFails = false; });

  /* Undo while another message is being written: the undone one comes back,
     the other is kept for Compose. */
  await reset();
  await pg.click('#cmp-send');
  await pg.evaluate(() => { __mock.calls = []; newDraft(); openCompose(); $('#cmp-to').value = 'gi@example.org'; $('#cmp-subj').value = 'Another'; });
  await pg.click('#st-undo');
  const swap = await pg.evaluate(() => ({ subj: $('#cmp-subj').value, held: HELD.map((h) => h.subj) }));
  await pg.evaluate(() => { newDraft(); closeCompose(); composeNew(); });
  const back = await pg.evaluate(() => $('#cmp-subj').value);
  check(swap.subj === 'Will fail' && swap.held.join() === 'Another' && back === 'Another' && (await sends()).length === 0,
    `Undo while writing another puts the undone one in the composer and keeps the other for Compose: ${JSON.stringify({ swap, back })}`);

  /* The window closing while it counts sends it at once. */
  await pg.evaluate(() => { HELD = []; newDraft(); openCompose(); $('#cmp-to').value = 'bo@example.org'; $('#cmp-subj').value = 'Closing'; S.settings.undoSend = 10; });
  await reset();
  await pg.click('#cmp-send');
  await pg.evaluate(() => dispatchEvent(new PageTransitionEvent('pagehide', { persisted: false })));
  await pg.waitForTimeout(150);
  s = await sends();
  check(s.length === 1 && s[0].subject === 'Closing' && (await outbox()).length === 0, `closing the window sends a waiting message at once, not after 10 s: ${JSON.stringify(s.map((x) => x.subject))}`);

  /* 0 seconds sends at once, from the open composer, as before. */
  await pg.evaluate(() => { S.settings.undoSend = 0; newDraft(); openCompose(); $('#cmp-to').value = 'bo@example.org'; $('#cmp-subj').value = 'Now'; });
  await reset();
  await pg.click('#cmp-send');
  await pg.waitForTimeout(100);
  s = await sends();
  check(s.length === 1 && s[0].subject === 'Now' && !(await pg.evaluate(() => document.querySelector('#send-toast').classList.contains('show'))) && (await outbox()).length === 0,
    `0 seconds sends at once, with no count: ${JSON.stringify(s.map((x) => x.subject))}`);

  /* RATA closed during the count, before anything reached the app (the
     webview gone without a pagehide): the next start sends it, once. */
  await pg.evaluate(() => { S.settings.undoSend = 10; save(); newDraft(); openCompose(); $('#cmp-to').value = 'bo@example.org'; $('#cmp-subj').value = 'After restart';
    CMP_FILES = [new File(['hello'], 'a.txt', { type: 'text/plain' })]; renderCmpFiles(); });
  await reset();
  await pg.click('#cmp-send');
  await pg.waitForTimeout(300);
  const kept = await outbox();
  check(kept.length === 1 && kept[0].state === 'waiting' && Array.isArray(kept[0].data) && kept[0].data[0] === 'aGVsbG8=', `the waiting record holds its files once read: ${JSON.stringify(kept.map((r) => ({ state: r.state, data: r.data })))}`);
  await pg.evaluate(() => { OUTBOX = []; });
  await pg.close();
  const pg2 = await open(true, { undo: true, context: ctx });
  await pg2.waitForTimeout(400);
  const s2 = await pg2.evaluate(() => ({ sent: __mock.calls.filter(([c]) => c === 'send_mail').map(([, a]) => ({ subject: a.draft.subject, files: a.draft.attachments.map((f) => f.name + ':' + f.data) })),
    box: localStorage.getItem(OB_KEY) }));
  check(s2.sent.length === 1 && s2.sent[0].subject === 'After restart' && s2.sent[0].files.join() === 'a.txt:aGVsbG8=' && s2.box === null,
    `the next start sends it once, with its file, and clears the record: ${JSON.stringify(s2)}`);
  await pg2.close();
  const pg3 = await open(true, { undo: true, context: ctx });
  await pg3.waitForTimeout(400);
  check((await pg3.evaluate(() => __mock.calls.filter(([c]) => c === 'send_mail').length)) === 0, 'and the start after that sends nothing: never twice');
  /* One already handed to the app may have gone: never sent again, put back
     with a word to look in Sent. */
  await pg3.evaluate(() => {
    const acct = S.linked.find((l) => l.type === 'mail').id;
    localStorage.setItem(OB_KEY, JSON.stringify([{ id: 'r1', at: Date.now(), state: 'sending', acctId: acct, real: true, to: 'bo@example.org', cc: '', bcc: '', subj: 'Maybe gone', body: 'b',
      replying: null, fwd: null, finished: null, files: [], data: [], look: { to: 'bo@example.org', cc: '', bcc: '', subj: 'Maybe gone', ccOn: false, bccOn: false, sig: null, draft: { kind: 'new' } } }]));
  });
  await pg3.close();
  const pg4 = await open(true, { undo: true, context: ctx });
  await pg4.waitForTimeout(400);
  const s4 = await pg4.evaluate(() => ({ sent: __mock.calls.filter(([c]) => c === 'send_mail').length, open: document.querySelector('#compose-ov').classList.contains('open'),
    subj: $('#cmp-subj').value, box: localStorage.getItem(OB_KEY) }));
  const t4 = await toasts(pg4);
  check(s4.sent === 0 && s4.open && s4.subj === 'Maybe gone' && s4.box === null,
    `one RATA closed on while it was being handed over is not sent again; it is put back: ${JSON.stringify({ s4, t4 })}`);
  await pg4.close();
  await ctx.close();
}
{
  /* The website cannot send, so it never counts. */
  const pg = await browser.newPage();
  pg.on('pageerror', (e) => { console.log('  PAGE ERROR: ' + e.message); fails++; });
  await pg.addInitScript(() => { localStorage.setItem('centra_session', JSON.stringify({ uid: 'local_t', email: 'me@example.com', mode: 'local' })); });
  await pg.goto(B + '/app.html');
  await pg.waitForFunction(() => typeof S !== 'undefined' && S && typeof go === 'function', null, { timeout: 20000 });
  const web = await pg.evaluate(() => { go('set'); return { delay: sendDelay(), row: getComputedStyle(document.querySelector('#undo-row')).display }; });
  check(web.delay === 0 && web.row === 'none', `on the website Send never waits, and Settings does not offer it: ${JSON.stringify(web)}`);
  await pg.close();
}

console.log('\n— the Text view folds quoted text, lightens a signature and cuts long links (H12) —');
{
  const pg = await open(true);
  const long = 'https://docs.example.org/meetings/2026/q4/planning/agenda-and-minutes?ref=mail&id='.padEnd(200, '7');
  const REPLY = [
    'Hi Ann,', '', 'Thursday works. The agenda is here: ' + long, 'and the room is https://example.org/room.', '',
    'Also: <script>window.__h12 = 1</script><img src=x onerror="window.__h12 = 2">', '',
    '> Can you bring the figures?', '> The ones from Q3.', '', 'Yes, I will.', '',
    '-- ', 'Bo Li', 'Example Ltd', '',
    'On Mon, 28 Sep 2026 at 09:00, Ann <ann@example.org> wrote:', '> Does Thursday work?', '>',
    '> Am Mo., 28. Sept. 2026 um 08:00 Uhr schrieb Cy <cy@example.de>:', '>> Passt es am Donnerstag?', '',
    'Earlier, from Dee:', '________________________________', 'From: Dee <dee@example.com>', 'Sent: Sunday, 27 September 2026 18:00', 'To: Bo Li', 'Subject: Planning', '',
    'Let us plan the quarter.',
  ].join('\n');
  const put = (id, body) => pg.evaluate(({ id, body }) => {
    S.messages = S.messages.filter((m) => m.id !== id);
    S.messages.push({ id, ch: 'email', prov: 'imap', acct: S.linked.find((l) => l.type === 'mail').id, mailbox: 'me@example.com', fromName: 'Bo Li', fromAddr: 'bo@example.org', subj: 'Re: Planning ' + id,
      prev: 'p', body, ts: Date.now(), unread: false, starred: false, atts: [], uid: 900 + id.length, uidvalidity: 7, bodyV: 2, html: false });
    openMail(id);
  }, { id, body });
  const view = () => pg.evaluate(() => {
    const el = document.querySelector('#md-body');
    return { seen: el.innerText, all: el.textContent, btns: [...el.querySelectorAll('.md-qbtn')].map((b) => ({ tag: b.tagName, type: b.type, text: b.textContent, exp: b.getAttribute('aria-expanded'),
      ctl: b.getAttribute('aria-controls'), hid: document.getElementById(b.getAttribute('aria-controls'))?.hidden })) };
  });
  await put('h12-reply', REPLY);
  let v = await view();
  check(v.btns.length === 3 && v.btns.every((b) => b.tag === 'BUTTON' && b.type === 'button' && b.text === 'Show quoted text' && b.exp === 'false' && b.hid === true),
    `each quoted part is folded behind its own Show quoted text button, aria-expanded false: ${JSON.stringify(v.btns)}`);
  check(/Thursday works/.test(v.seen) && /Yes, I will\./.test(v.seen) && /Bo Li/.test(v.seen) && /Earlier, from Dee:/.test(v.seen)
    && !/Can you bring|Does Thursday work|schrieb Cy|Passt es|wrote:|Original|From: Dee|Let us plan/.test(v.seen),
    `folded by default: the reply, the signature and the words between show, the ">" quote, the "On … wrote:" and German attribution and Outlook's header do not: ${JSON.stringify(v.seen)}`);
  check(/Can you bring/.test(v.all) && /Passt es/.test(v.all) && /Let us plan/.test(v.all), 'and nothing is lost: the folded text is on the page');
  /* Keyboard: the button takes focus and Enter opens it. */
  await pg.focus('#md-body .md-qbtn >> nth=1');
  await pg.keyboard.press('Enter');
  v = await view();
  check(v.btns[1].exp === 'true' && v.btns[1].text === 'Hide quoted text' && v.btns[1].hid === false && /Does Thursday work\?/.test(v.seen) && /schrieb Cy/.test(v.seen) && !/Let us plan/.test(v.seen),
    `Show quoted text, from the keyboard, reveals that part and says Hide quoted text: ${JSON.stringify(v.btns[1])}`);
  await pg.click('#md-body .md-qbtn >> nth=2');
  v = await view();
  check(/From: Dee/.test(v.seen) && /Let us plan/.test(v.seen) && /_{10}/.test(v.seen), 'the Outlook header folds from its rule to the end, and opens');
  await pg.evaluate(() => openMail('h12-reply'));
  v = await view();
  check(v.btns.map((b) => b.exp).join() === 'false,true,true', `a redraw keeps what was opened open: ${v.btns.map((b) => b.exp).join()}`);
  await pg.click('#md-body .md-qbtn >> nth=1');
  v = await view();
  check(v.btns[1].exp === 'false' && v.btns[1].text === 'Show quoted text' && !/Does Thursday work/.test(v.seen), 'Hide quoted text folds it again');

  /* The signature: there, and in the secondary colour. */
  const sig = await pg.evaluate(() => {
    const s = document.querySelector('#md-body .md-sig'), probe = document.createElement('span');
    probe.style.color = 'var(--slate)'; document.body.append(probe);
    const want = getComputedStyle(probe).color; probe.remove();
    return s && { text: s.textContent, shown: s.offsetHeight > 0, color: getComputedStyle(s).color, body: getComputedStyle(document.querySelector('#md-body')).color, want };
  });
  check(sig && sig.text === '-- \nBo Li\nExample Ltd' && sig.shown && sig.color === sig.want && sig.color !== sig.body,
    `the signature after "-- " is shown, lighter (--slate, not the text colour): ${JSON.stringify(sig)}`);

  /* The long link: cut for showing, whole in its title and what it opens. */
  const ln = await pg.evaluate((long) => [...document.querySelectorAll('#md-body a.md-link')].map((a) => ({ text: a.textContent, href: a.getAttribute('href'), title: a.title })), long);
  const cut = ln.find((a) => a.href === long);
  check(long.length === 200 && cut && cut.text.length <= 64 && cut.text.startsWith('docs.example.org/meetings/') && cut.text.endsWith('…') && cut.title === long,
    `a 200-character address shows as its host and the start of its path, whole in its title: ${JSON.stringify(cut && { text: cut.text, title: cut.title.length })}`);
  check(ln.some((a) => a.href === 'https://example.org/room' && a.text === 'https://example.org/room' && !a.title), 'a short one shows as it is');
  await pg.evaluate(() => { __mock.opened = []; __mock.calls = []; });
  await pg.click(`#md-body a.md-link[href="${long}"]`);
  await pg.waitForFunction(() => (__mock.opened || []).length === 1, null, { timeout: 3000 }).catch(() => {});
  const opened = await pg.evaluate(() => __mock.opened);
  check(opened.length === 1 && opened[0] === long, `and a click opens the whole address through open_link: ${opened.map((u) => u.length)} characters`);

  /* No script in the text runs, and none of its markup becomes an element. */
  const safe = await pg.evaluate(() => ({ ran: window.__h12, els: document.querySelectorAll('#md-body script, #md-body img').length, text: document.querySelector('#md-body').textContent }));
  check(safe.ran === undefined && safe.els === 0 && safe.text.includes('<script>window.__h12 = 1</script><img src=x onerror="window.__h12 = 2">'),
    `no script in the message runs; its markup is shown as text: ${JSON.stringify({ ran: safe.ran, els: safe.els })}`);

  /* What must not fold. */
  await put('h12-only', '> Does Thursday work?\n> Or Friday?\n>\n> Ann');
  v = await view();
  check(v.btns.length === 0 && /Or Friday\?/.test(v.seen), `a message that is only a quote is not folded: ${JSON.stringify(v)}`);
  await put('h12-onlyon', 'On Mon, 28 Sep 2026 at 09:00, Ann <ann@example.org> wrote:\n> Does Thursday work?\n> Or Friday?\n\n-- \nBo');
  v = await view();
  check(v.btns.length === 0 && /Or Friday\?/.test(v.seen), `nor one that is only an attribution, its quote and a signature: ${JSON.stringify(v.btns)}`);
  await put('h12-code', 'Run this:\n\n  $ cat notes\n  > first line\n\nWhen x\n> 0 the sum grows.\nThat is all.');
  v = await view();
  check(v.btns.length === 0 && /> first line/.test(v.seen) && /> 0 the sum grows/.test(v.seen), `a lone ">" line in code or maths is not folded: ${JSON.stringify(v.seen)}`);
  await put('h12-fwd', 'FYI\n\n---------- Forwarded message ---------\nFrom: Ann <ann@example.org>\nDate: Mon, 28 Sep 2026\nSubject: Plan\nTo: me@example.com\n\nThe plan.');
  v = await view();
  check(v.btns.length === 0 && /The plan\./.test(v.seen), 'a forwarded message is the message, not a quote, and stays open');

  /* Each attribution the card names, on its own. */
  const forms = await pg.evaluate(() => Object.fromEntries(Object.entries({
    de: 'Danke!\n\nAm Mo., 28. Sept. 2026 um 08:00 Uhr schrieb Cy <cy@example.de>:\n> Passt es?',
    fr: 'Merci\n\nLe lun. 28 sept. 2026 à 08:00, Cy <cy@example.fr> a écrit :\n> Ça va ?',
    es: 'Gracias\n\nEl lun, 28 sept 2026 a las 8:00, Cy (<cy@example.es>) escribió:\n> ¿Vale?',
    nl: 'Dank\n\nOp ma 28 sep. 2026 om 08:00 schreef Cy <cy@example.nl>:\n> Goed?',
    en: 'Sure.\n\nOn Mon, Sep 28, 2026 at 9:00 AM Ann Example <\nann@example.org> wrote:\n\n> Lunch?',
    orig: 'Ok\n\n-----Original Message-----\nFrom: Dee\nTo: Bo\n\nThe original.',
    outlook: 'See below.\n\nFrom: Dee <dee@example.com>\nSent: Sunday, 27 September 2026 18:00\nTo: Bo\n\nThe original.',
    plain: 'Ok\n\nOn Mon, Ann wrote:\nThe original, with no ">" before it.',
  }).map(([k, t]) => [k, readingParts(t).map((p) => p.kind).join()])));
  check(Object.values(forms).every((k) => k === 'text,quote'), `German, French, Spanish, Dutch, a wrapped English attribution, -----Original Message-----, a From:/Sent: header and an unprefixed original all fold: ${JSON.stringify(forms)}`);

  /* A translation folds the same way. */
  await pg.evaluate(() => { TRANSLATED.set('h12-reply', { text: 'Hallo Ann,\n\nDonnerstag passt.\n\nOn Mon, 28 Sep 2026 at 09:00, Ann <ann@example.org> wrote:\n> Passt Donnerstag?\n> Oder Freitag?', to: 'de', hidden: false }); openMail('h12-reply'); });
  v = await view();
  check(v.btns.length === 1 && v.btns[0].exp === 'false' && /Donnerstag passt/.test(v.seen) && !/Oder Freitag/.test(v.seen), `a translated message folds its translation's quote too: ${JSON.stringify(v.btns)}`);
  await pg.evaluate(() => TRANSLATED.delete('h12-reply'));

  /* The formatted view is left alone. */
  const fmt = await pg.evaluate(() => {
    OPENED.set('h12-reply', { text: 'Hi\n\n> a\n> b', html: '<p>Hi</p><blockquote>&gt; a<br>&gt; b</blockquote>', atts: [] });
    VIEWMODE.set('h12-reply', 'html'); openMail('h12-reply');
    const f = document.querySelector('#md-html');
    const r = { frame: !!f, sandbox: f && f.getAttribute('sandbox'), btns: document.querySelectorAll('#mail-detail .md-qbtn').length };
    VIEWMODE.set('h12-reply', 'text'); openMail('h12-reply');
    r.textBtns = document.querySelectorAll('#md-body .md-qbtn').length;
    OPENED.delete('h12-reply');
    return r;
  });
  check(fmt.frame && fmt.sandbox === 'allow-popups' && fmt.btns === 0 && fmt.textBtns === 1, `the formatted view is untouched; its Text view folds: ${JSON.stringify(fmt)}`);
  await pg.close();
}

console.log('\n— In this conversation: linked by ids, never by subject —');
{
  /* H5: a reply chain across the inbox and Sent, as Rust sends it: A (inbox)
     ← B (Sent, In-Reply-To A) ← C (inbox, no In-Reply-To, only References'
     last id B). A stranger with the same subject, and spam that names A. */
  const pg = await open(true);
  const now = Date.now();
  const mk = (folder, uid, extra) => Object.assign({ id: 'me@example.com_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Ann', from_addr: 'ann@example.org', to_name: '', to_addr: 'me@example.com', to_all: ['me@example.com'], cc: [], in_reply_to: '', subject: 'Re: Lunch', preview: 'p' + uid, body: 'Body ' + uid,
    ts: now - 3600e3, unread: false, starred: false, uid, uidvalidity: 7, message_id: 'm' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  await pg.evaluate(async (msgs) => {
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail');
  }, [
    mk('inbox', 91, { subject: 'Lunch', message_id: 'a@example.org', body: 'Lunch on Thursday?', preview: 'Lunch on Thursday?', ts: now - 3 * 3600e3 }),
    mk('sent', 21, { from_name: 'Me', from_addr: 'me@example.com', to_name: 'Ann', to_addr: 'ann@example.org', message_id: 'b@example.com', in_reply_to: 'a@example.org', body: 'Thursday works.', preview: 'Thursday works.', ts: now - 2 * 3600e3 }),
    mk('inbox', 92, { message_id: 'c@example.org', references_last: 'b@example.com', body: 'Great, see you at noon.', preview: 'Great, see you at noon.', ts: now - 3600e3 }),
    mk('inbox', 93, { from_name: 'Stranger', from_addr: 'x@example.net', message_id: 's@example.net', body: 'Same subject, other talk.', ts: now - 1800e3 }),
    mk('junk', 94, { from_name: 'Ann', from_addr: 'ann@example.org', message_id: 'spam@example.net', in_reply_to: 'a@example.org', body: 'Claim your prize', ts: now - 900e3 }),
  ]);
  const A = 'me@example.com_91', B = 'me@example.com_sent_21', C = 'me@example.com_92';
  const pane = (id) => pg.evaluate((id) => {
    if (id) openMail(id);
    const sec = document.querySelector('#md-convo');
    if (!sec) return null;
    const h = sec.querySelector('h3');
    return { hidden: sec.hidden, heading: h ? h.textContent : null, labelled: sec.getAttribute('aria-labelledby') === (h && h.id),
      rows: [...sec.querySelectorAll('.cv-row')].map((r) => ({ id: r.dataset.cv || null, on: r.getAttribute('aria-current') === 'true', button: r.tagName === 'BUTTON',
        who: r.querySelector('.cv-who').textContent, when: r.querySelector('.cv-when').textContent, words: r.querySelector('.cv-words').textContent })),
      more: !!sec.querySelector('.cv-more'),
      below: !!sec.compareDocumentPosition && !!document.querySelector('#md-body') && !!(document.querySelector('#md-body').compareDocumentPosition(sec) & Node.DOCUMENT_POSITION_FOLLOWING) };
  }, id);
  let p = await pane(A);
  check(p && !p.hidden && /^In this conversation/.test(p.heading) && p.labelled && p.below, `an opened message shows "In this conversation" under its text, with a heading: ${JSON.stringify(p && { hidden: p.hidden, heading: p.heading, labelled: p.labelled, below: p.below })}`);
  check(p && p.rows.length === 3 && p.rows[0].on && !p.rows[0].button && p.rows[1].id === B && p.rows[2].id === C && p.rows[1].button && p.rows[2].button,
    `the three-message chain across the inbox and Sent, oldest first, the open one marked: ${JSON.stringify(p && p.rows)}`);
  check(p && /\(you\)$/.test(p.rows[1].who) && p.rows[1].words === 'Thursday works.' && p.rows[2].who === 'Ann' && p.rows[2].words === 'Great, see you at noon.' && p.rows.every((r) => r.when),
    `each row has the sender, the date and the first words: ${JSON.stringify(p && p.rows)}`);
  check(p && !p.rows.some((r) => r.id === 'me@example.com_93'), 'a stranger with the same subject is not pulled in');
  check(p && !p.rows.some((r) => r.id === 'me@example.com_junk_94'), 'spam that names a message of the chain is not pulled in');
  p = await pane('me@example.com_junk_94');
  check(p && p.hidden && p.rows.length === 0, 'an open spam message shows no conversation');
  p = await pane('me@example.com_93');
  check(p && p.hidden && p.rows.length === 0, 'a message linked to nothing shows no conversation');
  /* From the other end, by References alone. */
  p = await pane(C);
  check(p && p.rows.map((r) => r.id || 'on').join() === `${A},${B},on` && p.rows[2].on, `the chain is found from its last message too, through References: ${JSON.stringify(p && p.rows.map((r) => r.id))}`);

  /* One click opens a row; the keyboard reaches it and Enter opens it too. */
  await pg.evaluate((A) => openMail(A), A);
  await pg.click(`#md-convo [data-cv="${C}"]`);
  let st = await pg.evaluate(() => ({ sel: selMail, cur: (document.querySelector('#md-convo .cv-row.on') || {}).textContent || '', focus: document.activeElement && document.activeElement.classList.contains('on') }));
  check(st.sel === C && /Great, see you/.test(st.cur) && st.focus, `a click opens that message, which the list then marks, with the focus on it: ${JSON.stringify(st)}`);
  await pg.evaluate((B) => document.querySelector(`#md-convo [data-cv="${B}"]`).focus(), B);
  await pg.keyboard.press('Enter');
  st = await pg.evaluate(() => selMail);
  check(st === B, `Enter on a focused row opens it: ${st}`);

  /* Mail stored before the field: only messageId, which is what it links by. */
  await pg.evaluate(() => {
    const acct = S.messages.find((m) => m.id === 'me@example.com_91').acct;
    S.messages.push({ id: 'old-1', ch: 'email', prov: 'imap', acct, mailbox: 'me@example.com', fromName: 'Dee', fromAddr: 'dee@example.org', subj: 'Plan', prev: 'The old plan', body: 'The old plan', ts: 1000, unread: false, starred: false, atts: [], uid: 5, uidvalidity: 7, bodyV: 2, messageId: 'old@example.org' });
    S.messages.push({ id: 'old-2', ch: 'email', prov: 'imap', acct, mailbox: 'me@example.com', fromName: 'Eve', fromAddr: 'eve@example.org', subj: 'Re: Plan', prev: 'Agreed', body: 'Agreed', ts: 2000, unread: false, starred: false, atts: [], uid: 6, uidvalidity: 7, bodyV: 2, messageId: 'e@example.org', inReplyTo: 'old@example.org' });
  });
  p = await pane('old-1');
  check(p && p.rows.length === 2 && p.rows[1].id === 'old-2', `mail stored before the field links by the id it has: ${JSON.stringify(p && p.rows.map((r) => r.id))}`);

  /* A reply sent from RATA joins at once: its own record carries the
     Message-ID it wrote and what it answered. */
  await pg.evaluate((C) => openMail(C), C);
  await pg.click('#md-reply');
  await pg.fill('#cmp-body', 'Noon it is.');
  await pg.click('#cmp-send');
  await pg.waitForFunction(() => (__mock.sent || []).length > 0 && S.messages.some((m) => String(m.id).startsWith('x') && m.sent), null, { timeout: 5000 }).catch(() => {});
  p = await pane(A);
  const mine = await pg.evaluate(() => { const m = S.messages.find((x) => String(x.id).startsWith('x') && x.sent); return m && { id: m.id, inReplyTo: m.inReplyTo, messageId: m.messageId }; });
  check(mine && mine.inReplyTo === 'c@example.org' && mine.messageId && p.rows.length === 4 && p.rows[3].id === mine.id,
    `a reply RATA sent joins the conversation: ${JSON.stringify({ mine, rows: p && p.rows.map((r) => r.id) })}`);
  if (process.env.H5_SHOTS) {
    await pg.evaluate((A) => { openMail(A); document.querySelector('#mail-detail').scrollTop = 1e6; }, A);
    await pg.screenshot({ path: process.env.H5_SHOTS + '/h5-light.png' });
    await pg.emulateMedia({ colorScheme: 'dark' });
    await pg.waitForTimeout(400);
    await pg.screenshot({ path: process.env.H5_SHOTS + '/h5-dark.png' });
    await pg.emulateMedia({ colorScheme: 'light' });
  }

  /* A chain of 300: the walk stops at 200, the nearest first, and says so,
     without holding the page up. */
  const t = await pg.evaluate(() => {
    const acct = S.messages.find((m) => m.id === 'me@example.com_91').acct;
    for (let i = 0; i < 300; i++) S.messages.push({ id: 'k' + i, ch: 'email', prov: 'imap', acct, mailbox: 'me@example.com', fromName: 'Kay', fromAddr: 'kay@example.org', subj: 'Long', prev: 'Part ' + i, ts: 10000 + i * 1000, unread: false, starred: false, atts: [], uid: 1000 + i, uidvalidity: 7, bodyV: 2,
      messageId: 'k' + i + '@example.org', ...(i ? (i % 2 ? { inReplyTo: 'k' + (i - 1) + '@example.org' } : { refsLast: 'k' + (i - 1) + '@example.org' }) : {}) });
    const t0 = performance.now(); openMail('k150'); return performance.now() - t0;
  });
  p = await pane();
  const ids = p ? p.rows.map((r) => r.id || 'k150') : [];
  const nums = ids.map((x) => +x.slice(1));
  check(p && p.rows.length === 200 && p.more && ids.includes('k150') && Math.min(...nums) >= 50 && Math.max(...nums) <= 250 && nums.every((n, i) => !i || n > nums[i - 1]) && t < 1000,
    `a chain of 300 stops at the 200 nearest, in order, and says so, in ${Math.round(t)} ms: ${JSON.stringify({ n: p && p.rows.length, more: p && p.more, lo: Math.min(...nums), hi: Math.max(...nums) })}`);
  await pg.close();
}

console.log('\n— accessibility: axe at five points, and the list, dialogs and toasts by keyboard (H10) —');
{
  /* axe-core, as @axe-core/playwright runs it, at the points the card
     names. Any serious or critical rule fails the check; lesser ones are
     printed with where they are. */
  const { default: AxeBuilder } = await import('../../../rata-next/node_modules/@axe-core/playwright/dist/index.mjs');
  const axe = async (pg, where) => {
    /* Measure the settled screen: wait for every CSS transition and
       animation still running (a theme switch or a resize starts them) to
       finish. Twice on CI a fade in progress was measured as poor contrast
       (#top-search-btn: its background fades over .15s while its text
       colour changes at once), which nobody sees once the fade is done. */
    await pg.evaluate(() => Promise.race([
      Promise.all(document.getAnimations()
        .filter((a) => a.effect && a.effect.getComputedTiming().endTime !== Infinity)
        .map((a) => a.finished.catch(() => {}))),
      new Promise((r) => setTimeout(r, 2000)),
    ]));
    const r = await new AxeBuilder({ page: pg }).analyze();
    const bad = r.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
    for (const v of r.violations) console.log(`        axe ${v.impact}: ${v.id} at ${v.nodes.slice(0, 4).map((n) => n.target.join(' ')).join(' | ')}${v.nodes.length > 4 ? ` (+${v.nodes.length - 4})` : ''}`);
    check(bad.length === 0, `axe, ${where}: no serious or critical violation (${r.passes.length} rules pass): ${JSON.stringify(bad.map((v) => v.id))}`);
  };
  /* AxeBuilder needs a page made from a context of its own. */
  const ctx = await browser.newContext();
  const pg = await open(true, { context: ctx });
  const now = Date.now();
  const mk = (folder, uid, extra) => Object.assign({ id: 'me@example.com_' + (folder === 'inbox' ? '' : folder + '_') + uid, folder, acct: 'me@example.com', acct_label: 'Example',
    from_name: 'Sender ' + uid, from_addr: 's' + uid + '@example.org', to_name: '', to_addr: 'me@example.com', to_all: ['me@example.com'], cc: [], in_reply_to: '', subject: 'Subject ' + uid, preview: 'Preview of ' + uid, body: 'Body ' + uid,
    ts: now - uid * 60e3, unread: uid % 3 === 0, starred: uid % 5 === 0, uid, uidvalidity: 7, message_id: 'h' + uid + '@example.org', reply_to: '', truncated: false, attachments: [], html: false }, extra || {});
  const REPLY = ['Thursday works, and here are the pictures.', '', '-- ', 'Bo Li', '', 'On Mon, 28 Sep 2026 at 09:00, Ann <ann@example.org> wrote:', '> Lunch on Thursday?', '> And bring the pictures.'].join('\n');
  const atts = [{ index: 1, name: 'holiday.png', mime: 'image/png', size: 2000, disguised: false }, { index: 2, name: 'notes.pdf', mime: 'application/pdf', size: 900, disguised: false }];
  const msgs = [
    mk('inbox', 91, { from_name: 'Ann', from_addr: 'ann@example.org', subject: 'Lunch', message_id: 'a@example.org', body: 'Lunch on Thursday?', ts: now - 3 * 3600e3, unread: false }),
    mk('inbox', 95, { from_name: 'Bo Li', from_addr: 'bo@example.org', subject: 'Re: Lunch', in_reply_to: 'a@example.org', body: REPLY, attachments: atts, unread: true, ts: now - 1000 }),
  ];
  for (let u = 1; u <= 40; u++) msgs.push(mk('inbox', u));
  await pg.evaluate(async ({ msgs, atts, REPLY }) => {
    const c = document.createElement('canvas'); c.width = 64; c.height = 48;
    const x = c.getContext('2d'); x.fillStyle = '#4a7'; x.fillRect(0, 0, 64, 48);
    __mock.readable = { 1: { name: 'holiday.png', mime: 'image/png', data: c.toDataURL('image/png').split(',')[1] } };
    __mock.openAttsBy = { 95: atts };
    __mock.openTextBy = { 95: REPLY };
    __mock.refresh = { messages: msgs, flags: [], problems: [], skipped: [] };
    await serverSync('mail');
    go('inbox'); mailFilter = 'all'; renderMailFilters(); renderMail();
  }, { msgs, atts, REPLY });
  await pg.waitForTimeout(300);
  await axe(pg, 'the inbox with messages');

  await pg.evaluate(() => openMail('me@example.com_95'));
  await pg.waitForFunction(() => document.querySelector('#mail-detail .md-pics img') && !document.querySelector('#md-convo').hidden && document.querySelector('#md-body .md-qbtn'), null, { timeout: 8000 }).catch(() => {});
  const shown = await pg.evaluate(() => ({ pic: !!document.querySelector('#mail-detail .md-pics img'), convo: !document.querySelector('#md-convo').hidden, quote: !!document.querySelector('#md-body .md-qbtn') }));
  check(shown.pic && shown.convo && shown.quote, `the open message has a picture, a conversation and a folded quote: ${JSON.stringify(shown)}`);
  await axe(pg, 'an open message with a conversation, a picture and a folded quote');

  await pg.evaluate(() => { newDraft(); openCompose(); });
  await pg.click('#cmp-to');
  await pg.keyboard.type('an');
  await pg.waitForTimeout(300);
  const sugg = await pg.evaluate(() => !document.querySelector('#cmp-sugg').hidden);
  check(sugg, 'the composer is open with its suggestion list showing');
  await axe(pg, 'the composer with the suggestion list open');
  await pg.keyboard.press('Escape');
  await pg.evaluate(() => closeCompose());

  await pg.evaluate(() => go('set'));
  await pg.waitForTimeout(200);
  await axe(pg, 'Settings');

  await pg.evaluate(() => { go('inbox'); askToOpen('https://example.org/a/very/long/path', 'example.org'); });
  await axe(pg, 'the link question');
  await pg.evaluate(() => closeLink());
  await pg.evaluate(() => { askDisguised({ n: 'invoice.pdf.exe' }, true); });
  await axe(pg, 'the disguised-program question');
  await pg.evaluate(() => closeWarn(false));
  await pg.evaluate(() => openKeys());
  await axe(pg, 'the keyboard sheet');
  await pg.evaluate(() => closeKeys());

  /* The same two screens in the dark, where DESIGN.md gives its own
     contrast figures. */
  await pg.emulateMedia({ colorScheme: 'dark' });
  await pg.evaluate(() => { go('inbox'); selMail = null; renderMail(); renderResting(); });
  /* Let the theme settle before measuring: .top-search fades its background
     over .15s while its text colour changes at once, so axe run inside that
     fade (as it was on a slower CI runner) measures dark-mode text on a
     still-light background, which no one sees once the switch is done. */
  await pg.waitForTimeout(300);
  await axe(pg, 'the inbox, dark');
  await pg.evaluate(() => openMail('me@example.com_95'));
  await pg.waitForTimeout(300);
  await axe(pg, 'an open message, dark');
  await pg.emulateMedia({ colorScheme: 'light' });
  /* Between 901 and 1200 px the rail is icons only: still named. */
  await pg.setViewportSize({ width: 1000, height: 720 });
  const rail = await pg.evaluate(() => [...document.querySelectorAll('#rail .nav-item')].filter((b) => b.getClientRects().length).map((b) => b.querySelector('.nav-label') && getComputedStyle(b.querySelector('.nav-label')).display !== 'none' && b.querySelector('.nav-label').getBoundingClientRect().width <= 1));
  check(rail.length >= 4 && rail.every(Boolean), `icons-only, each rail button still carries its word for a screen reader: ${JSON.stringify(rail)}`);
  await axe(pg, 'the inbox with the rail as icons only');
  await pg.setViewportSize({ width: 1280, height: 720 });

  /* Landmarks, each named. */
  const marks = await pg.evaluate(() => ({
    main: document.querySelectorAll('main').length,
    nav: document.querySelector('#rail').tagName === 'NAV' && document.querySelector('#rail').getAttribute('aria-label'),
    list: document.querySelector('#view-inbox .side-list').getAttribute('role') + ':' + document.querySelector('#view-inbox .side-list').getAttribute('aria-label'),
    pane: document.querySelector('#mail-detail').getAttribute('role') + ':' + document.querySelector('#mail-detail').getAttribute('aria-label'),
    composer: document.querySelector('#compose').getAttribute('role') + ':' + document.querySelector('#compose').getAttribute('aria-labelledby'),
  }));
  check(marks.main === 1 && marks.nav === 'Sections' && marks.list === 'region:Message list' && marks.pane === 'region:Reading pane' && marks.composer === 'dialog:cmp-title',
    `the navigation, the list, the reading pane and the composer are landmarks with names: ${JSON.stringify(marks)}`);

  /* The list is a listbox of options that know their place in the whole
     list, however few are drawn, with one Tab stop. */
  await pg.evaluate(() => { go('inbox'); selMail = null; KEY_AT = null; renderMail(); document.activeElement && document.activeElement.blur(); });
  const lb = () => pg.evaluate(() => {
    const sc = document.querySelector('#mail-scroll'), win = sc.querySelector('.vl-win'), opts = [...win.querySelectorAll('[role=option]')];
    const a = document.activeElement;
    return { role: win.getAttribute('role'), label: win.getAttribute('aria-label'), n: VL.get(sc).pool.length, drawn: opts.length,
      sizes: [...new Set(opts.map((o) => o.getAttribute('aria-setsize')))], pos: opts.map((o) => +o.getAttribute('aria-posinset')),
      stops: opts.filter((o) => o.tabIndex === 0).map((o) => o.dataset.id), focus: a && a.classList.contains('mail-row') ? a.dataset.id : a && (a.id || a.className || a.tagName),
      focusPos: a && a.getAttribute('aria-posinset'), ring: a && a.classList.contains('mail-row') ? getComputedStyle(a).outlineStyle : null,
      selected: opts.filter((o) => o.getAttribute('aria-selected') === 'true').map((o) => o.dataset.id), at: KEY_AT, sel: selMail };
  });
  let l = await lb();
  check(l.role === 'listbox' && l.label === 'Messages' && l.n === 42 && l.drawn < l.n && l.sizes.join() === '42' && l.pos.every((p, i) => i === 0 || p === l.pos[i - 1] + 1) && l.stops.length === 1,
    `the mail list is a listbox; each drawn row an option of 42 at its own place, one of them the Tab stop: ${JSON.stringify({ role: l.role, label: l.label, n: l.n, drawn: l.drawn, sizes: l.sizes, stops: l.stops })}`);
  await pg.focus('#mail-sort');
  for (let k = 0; k < 6 && !(await pg.evaluate(() => document.activeElement.classList.contains('mail-row'))); k++) await pg.keyboard.press('Tab');
  l = await lb();
  check(l.focus === l.stops[0] && l.focusPos === '1' && l.ring === 'solid', `Tab reaches the list at its one stop, the first row, with the focus ring: ${JSON.stringify({ focus: l.focus, pos: l.focusPos, ring: l.ring })}`);
  for (let k = 0; k < 3; k++) await pg.keyboard.press('ArrowDown');
  l = await lb();
  check(l.focusPos === '4' && l.at === l.focus && l.stops.join() === l.focus && l.sel === null, `Down moves the focus row by row, the Tab stop with it, opening nothing: ${JSON.stringify({ pos: l.focusPos, at: l.at, stops: l.stops, sel: l.sel })}`);
  await pg.keyboard.press('j'); await pg.keyboard.press('j'); await pg.keyboard.press('k');
  l = await lb();
  check(l.focusPos === '5' && l.at === l.focus, `j and k move the focus too: ${JSON.stringify({ pos: l.focusPos, at: l.at })}`);
  await pg.keyboard.press('End');
  await pg.waitForTimeout(100);
  l = await lb();
  check(l.focusPos === '42' && l.at === l.focus && l.stops.join() === l.focus, `End goes to the last of the 42, drawing it: ${JSON.stringify({ pos: l.focusPos, at: l.at })}`);
  await pg.keyboard.press('Home');
  await pg.waitForTimeout(100);
  l = await lb();
  const firstId = l.focus;
  check(l.focusPos === '1', `Home goes back to the first: ${l.focusPos}`);
  await pg.keyboard.press('Enter');
  await pg.waitForTimeout(150);
  l = await lb();
  check(l.sel === firstId && l.focus === firstId && l.selected.join() === firstId, `Enter opens the focused message; the row keeps the focus and says it is selected: ${JSON.stringify({ sel: l.sel, focus: l.focus, selected: l.selected })}`);
  /* A redraw (a star from elsewhere) keeps the focus on the row. */
  await pg.evaluate(() => renderMail());
  l = await lb();
  check(l.focus === firstId, `a redraw of the list keeps the focus where it was: ${l.focus}`);
  /* A click on a row still opens it, and the row takes the focus. */
  await pg.click('#mail-scroll [aria-posinset="3"]');
  l = await lb();
  check(l.sel === l.focus && l.focusPos === '3', `a click opens a row and gives it the focus: ${JSON.stringify({ sel: l.sel, focus: l.focus, pos: l.focusPos })}`);

  /* Unread and starred are more than a colour: a weight and a dot, a star,
     and the words for a screen reader. */
  const cue = await pg.evaluate(() => {
    const row = (id) => document.querySelector(`#mail-scroll [data-id="${id}"]`);
    const un = row('me@example.com_3'), star = row('me@example.com_5');
    return { unWords: un && un.querySelector('.vh') && un.querySelector('.vh').textContent, unWeight: un && getComputedStyle(un.querySelector('.m-from')).fontWeight,
      dot: un && getComputedStyle(un, '::before').width, starWords: star && star.querySelector('.vh') && star.querySelector('.vh').textContent,
      starGlyph: star && star.querySelector('.clip.star') && star.querySelector('.clip.star').textContent };
  });
  check(/Unread/.test(cue.unWords) && cue.unWeight === '600' && cue.dot === '6px' && /Starred/.test(cue.starWords) && cue.starGlyph === '★',
    `unread is bold with a dot and says "Unread"; starred has a star and says "Starred": ${JSON.stringify(cue)}`);

  /* Toasts are announced, and take no focus. */
  const t = await pg.evaluate(() => { const before = document.activeElement; toast('Checked'); const el = document.querySelector('#toast');
    return { role: el.getAttribute('role'), live: el.getAttribute('aria-live'), kept: document.activeElement === before, send: document.querySelector('#send-toast').getAttribute('role') }; });
  check(t.role === 'status' && t.live === 'polite' && t.kept && t.send === 'status', `a toast is a polite status that leaves the focus where it was: ${JSON.stringify(t)}`);

  /* Every overlay is a modal dialog with a name. */
  const dl = await pg.evaluate(() => ['#link-ov', '#warn-ov', '#pic-ov', '#keys-ov', '#welcome-ov', '#compose', '#assist'].map((s) => {
    const el = document.querySelector(s), by = el.getAttribute('aria-labelledby');
    return s + ':' + /^(alert)?dialog$/.test(el.getAttribute('role')) + ':' + el.getAttribute('aria-modal') + ':' + !!(by && document.getElementById(by));
  }));
  check(dl.every((d) => d.endsWith(':true:true:true')), `every overlay is a modal dialog labelled by its heading: ${JSON.stringify(dl)}`);

  /* The keyboard stays in the dialog on top, and comes back to where it
     was when the dialog closes. */
  const inside = (sel) => pg.evaluate((sel) => document.querySelector(sel).contains(document.activeElement), sel);
  const focusId = () => pg.evaluate(() => document.activeElement && document.activeElement.id);
  const round = async (sel, n) => { let ok = true; for (let k = 0; k < n; k++) { await pg.keyboard.press(k % 3 === 2 ? 'Shift+Tab' : 'Tab'); ok = ok && await inside(sel); } return ok; };
  await pg.evaluate(() => openMail('me@example.com_95'));
  await pg.focus('#md-forward');
  await pg.evaluate(() => askToOpen('https://example.org/x', 'example.org'));
  let kept = await round('#link-ov', 6);
  await pg.keyboard.press('Escape');
  check(kept && (await focusId()) === 'md-forward', `the link question keeps Tab inside it, and Escape gives the focus back: ${kept}`);
  await pg.focus('#md-forward');
  const warned = pg.evaluate(() => askDisguised({ n: 'invoice.pdf.exe' }, true));
  /* askDisguised's promise settles only when the question closes, so it is
     not awaited here; wait instead until the question is open and holds the
     focus. Without this the first Tab could reach the page before the
     question opened (it did on the 0.1.44 release gate), moving the focus
     off Forward, which the question then gave back to on Escape. */
  await pg.waitForFunction(() => document.querySelector('#warn-ov').contains(document.activeElement));
  kept = await round('#warn-ov', 5);
  await pg.keyboard.press('Escape');
  await warned;
  check(kept && (await focusId()) === 'md-forward', `the disguised-program question too: ${kept}`);
  await pg.focus('#compose-btn');
  await pg.keyboard.press('Enter');
  await pg.waitForTimeout(150);
  kept = await round('#compose-ov', 16);
  await pg.evaluate(() => document.activeElement.blur());
  await pg.keyboard.press('Escape');
  check(kept && (await focusId()) === 'compose-btn', `the composer keeps Tab inside it, and closing it gives the focus back to Compose: ${JSON.stringify({ kept, focus: await focusId() })}`);
  await pg.focus('#assist-fab');
  await pg.keyboard.press('Enter');
  const asIn = await inside('#assist');
  kept = await round('#assist-ov', 4);
  await pg.keyboard.press('Escape');
  check(asIn && kept && (await focusId()) === 'assist-fab', `Assist takes the focus, keeps it, and gives it back: ${JSON.stringify({ asIn, kept })}`);
  await pg.focus('#assist-fab');
  await pg.evaluate(() => showWelcome({ name: 'Ann' }));
  const wIn = await focusId();
  kept = await round('#welcome-ov', 4);
  await pg.keyboard.press('Escape');
  const wOpen = await pg.evaluate(() => document.querySelector('#welcome-ov').classList.contains('open'));
  check(wIn === 'w-enter' && kept && !wOpen && (await focusId()) === 'assist-fab', `the welcome takes the focus, keeps it, closes on Escape and gives it back: ${JSON.stringify({ wIn, kept, wOpen })}`);
  await pg.evaluate(() => go('set'));
  await pg.click('#btn-delete');
  const dIn = await focusId();
  await pg.keyboard.press('Escape');
  const dShown = await pg.evaluate(() => getComputedStyle(document.querySelector('#del-confirm')).display !== 'none');
  check(dIn === 'del-email' && !dShown && (await focusId()) === 'btn-delete', `Delete account's confirmation takes the focus; Escape keeps the account and gives it back: ${JSON.stringify({ dIn, dShown })}`);

  /* Less motion, when the system asks. */
  await pg.emulateMedia({ reducedMotion: 'reduce' });
  const still = await pg.evaluate(() => {
    newDraft(); openCompose();
    const c = getComputedStyle(document.querySelector('#compose')), tt = getComputedStyle(document.querySelector('#toast'));
    const r = { compose: c.animationName, toast: tt.transitionDuration, glide: glide() };
    closeCompose(); return r;
  });
  check(still.compose === 'none' && /^(0s|1e-05s|0\.00001s)$/.test(still.toast.split(',')[0]) && still.glide === 'auto', `reduced motion stops what moves: ${JSON.stringify(still)}`);
  await pg.emulateMedia({ reducedMotion: 'no-preference' });
  await ctx.close();

  const lctx = await browser.newContext();
  const lp = await open(false, { context: lctx });
  await lp.waitForSelector('#rata-licence', { timeout: 5000 }).catch(() => {});
  await axe(lp, 'the licence box');
  const lic = await lp.evaluate(() => { const b = document.querySelector('#rata-licence'); return b && [b.getAttribute('role'), b.getAttribute('aria-modal'), document.getElementById(b.getAttribute('aria-labelledby'))?.textContent].join(':'); });
  let licIn = true;
  for (let k = 0; k < 5; k++) { await lp.keyboard.press('Tab'); licIn = licIn && await lp.evaluate(() => document.querySelector('#rata-licence').contains(document.activeElement)); }
  check(lic === 'dialog:true:Your licence key' && licIn, `the licence box is a modal dialog named by its heading, and Tab stays in it: ${JSON.stringify({ lic, licIn })}`);
  await lctx.close();
}

await browser.close();
console.log(fails ? `\n${fails} FAILED` : '\nALL PASSED');
process.exit(fails ? 1 : 0);
