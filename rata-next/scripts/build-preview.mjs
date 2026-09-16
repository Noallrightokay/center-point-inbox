/* Build a standalone, self-contained copy of the RATA app for review.

   The point is to shorten the loop between "change the UI" and "look at it".
   A deploy takes a build, an upload and a rebuild on the host; this takes a
   second and produces one HTML file that runs anywhere, so the product can be
   argued about before anything ships.

   It is the real app, not a mockup — public/app.html with its own markup, CSS
   and behaviour intact. Four things change, and only these:

     1. The conversion engines are inlined. In production they are separate
        files the service worker precaches; here there is no origin to fetch
        them from, and brLib() checks for the global before loading, so
        inlining makes the Format Bridge work with no change to the app.
     2. A demo workspace is seeded before the app boots — two mailboxes with
        mail in each, some files, a couple of people — because an empty shell
        shows nothing about how the product feels in use.
     3. Signing out has nowhere to go without auth.html, so it resets the demo
        instead of navigating into a blank page.
     4. A harness bar is added: switch plan tier, reload the demo, and a plain
        statement that this is a preview. It floats above the app and changes
        no layout, so what you are judging is the product and not a variant.

   Regenerate after any UI change:  npm run preview
*/

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const pub = join(here, '..', 'public');
const read = f => readFileSync(join(pub, f), 'utf8');

let html = read('app.html');

/* ---- 1. inline the conversion engines ---------------------------------- */
/* Supabase is deliberately not inlined: the preview has no backend, CLOUD is
   false, and the SDK is never reached. */
const ENGINES = [
  'mammoth-1.8.0.browser.min.js',
  'xlsx-0.20.3.full.min.js',
  'jspdf-2.5.1.umd.min.js',
];
/* Only one sequence ends a <script> from the inside, and escaping the "<" is
   invisible to JavaScript. Resist escaping "<!--" and "-->" as well: "-->" is
   ordinary JavaScript (i-->0 is a decrement followed by a comparison) and
   rewriting it breaks minified source.

   U+FFFD is the other case. SheetJS carries 51,671 of them as the placeholder
   for undefined code points in its codepage tables — real library content, not
   damage — but a lone replacement character in a file is normally the
   fingerprint of a lost byte, so tools reject it on sight. They sit inside
   string literals, where the \\uFFFD escape parses to exactly the same
   character. */
const scriptSafe = js => js
  .replace(/<\/script/gi, '<\\/script')
  .replace(/\uFFFD/g, '\\\\uFFFD');

const engines = ENGINES
  .map(f => `<script>/* ${f} */\n${scriptSafe(read('vendor/' + f))}\n</script>`)
  .join('\n');

/* ---- the logo has no origin to load from -------------------------------- */
/* app.html shows its mark with <img src="icons/mark-256.png">. In one file
   that is a broken image in the corner of every screen, which reads as a bug
   in the product rather than an artefact of the preview. Inlined as a data
   URI it looks exactly like production. */
const mark = readFileSync(join(pub, 'icons', 'mark-256.png')).toString('base64');
html = html.replaceAll('src="icons/mark-256.png"', `src="data:image/png;base64,${mark}"`);

/* ---- 2. the demo workspace --------------------------------------------- */
const UID = 'preview-user';
const now = Date.now();
const hrs = n => now - n * 3600000;

/* Six mailboxes, because that is the case the side-by-side view exists for:
   a Business account with more inboxes than fit on screen at once, choosing
   which two or three to put beside each other. */
const WORK = 'lk-work', HOME = 'lk-home';
const BILLING = 'lk-billing', LEGAL = 'lk-legal', OLD = 'lk-old', SUPPORT = 'lk-support';

const demo = {
  v: 5,
  settings: { name: 'Sam Reyes', api: '', profile: 'HIPAA', plan: 'pro', planSince: now - 86400000 * 40, org: 'Reyes & Co.' },
  linked: [
    { id: WORK, type: 'mail', label: 'sam@reyesandco.com', status: 'live', addedAt: now - 86400000 * 12, lastSync: hrs(1), host: 'imap.reyesandco.com', provLabel: 'reyesandco.com' },
    { id: HOME, type: 'mail', label: 'sam.reyes@gmail.com', status: 'live', addedAt: now - 86400000 * 12, lastSync: hrs(2), host: 'imap.gmail.com', provLabel: 'Gmail' },
    { id: BILLING, type: 'mail', label: 'billing@reyesandco.com', status: 'live', addedAt: now - 86400000 * 9, lastSync: hrs(3), host: 'imap.reyesandco.com', provLabel: 'reyesandco.com' },
    { id: LEGAL, type: 'mail', label: 'contracts@reyesandco.com', status: 'live', addedAt: now - 86400000 * 9, lastSync: hrs(4), host: 'imap.reyesandco.com', provLabel: 'reyesandco.com' },
    { id: SUPPORT, type: 'mail', label: 'hello@reyesandco.com', status: 'live', addedAt: now - 86400000 * 7, lastSync: hrs(2), host: 'imap.reyesandco.com', provLabel: 'reyesandco.com' },
    { id: OLD, type: 'mail', label: 's.reyes@outlook.com', status: 'live', addedAt: now - 86400000 * 30, lastSync: hrs(20), host: 'outlook.office365.com', provLabel: 'Outlook' },
  ],
  rules: [],
  plugins: { split: false, jobs: false, slack: false, sms: false, discord: false, autopilot: false },
  jobs: [],
  contacts: [
    { id: 'c1', name: 'Dana Whitfield', nick: '', addr: 'dana@brightpathdental.com', phone: '', slack: '', discord: '', context: 'work', job: '' },
    { id: 'c2', name: 'Marcus Reyes', nick: 'Dad', addr: 'm.reyes@shaw.ca', phone: '', slack: '', discord: '', context: 'personal', job: '' },
  ],
  messages: [
    { id: 'm1', ch: 'email', prov: 'imap', acct: WORK, real: true, cid: 'c1', fromName: 'Dana Whitfield', fromAddr: 'dana@brightpathdental.com',
      subj: 'Signed lease — countersigned copy attached', prev: 'Everything looks right on our end. Countersigned and attached; let me know if you need it in another format.',
      body: 'Hi Sam,\n\nEverything looks right on our end. Countersigned and attached — let me know if you need it in another format.\n\nDana', ts: hrs(2), unread: true, starred: true, atts: [{ n: 'lease-countersigned.pdf' }] },
    { id: 'm2', ch: 'email', prov: 'imap', acct: WORK, real: true, cid: null, fromName: 'Northgate Property', fromAddr: 'billing@northgateproperty.com',
      subj: 'Invoice 4471 — due 30 September', prev: 'Your quarterly invoice is ready. Amount due $2,480.00.',
      body: 'Your quarterly invoice is ready.\n\nAmount due: $2,480.00\nDue date: 30 September', ts: hrs(5), unread: true, starred: false, atts: [{ n: 'invoice-4471.pdf' }] },
    { id: 'm3', ch: 'email', prov: 'imap', acct: WORK, real: true, cid: null, fromName: 'Priya Anand', fromAddr: 'priya@anandbooks.ca',
      subj: 'Q3 numbers — can you send the spreadsheet?', prev: 'Ready to start on Q3 whenever you are. Could you send the sheet in Excel rather than Numbers this time?',
      body: 'Ready to start on Q3 whenever you are.\n\nCould you send the sheet in Excel rather than Numbers this time? Saves me a conversion step.\n\nPriya', ts: hrs(26), unread: false, starred: false, atts: [] },
    { id: 'm4', ch: 'email', prov: 'imap', acct: HOME, real: true, cid: 'c2', fromName: 'Marcus Reyes', fromAddr: 'm.reyes@shaw.ca',
      subj: 'Photos from the weekend', prev: 'Got some good ones of the kids at the lake. Sending the rest tomorrow.',
      body: 'Got some good ones of the kids at the lake.\n\nSending the rest tomorrow once I work out how to shrink them.\n\nDad', ts: hrs(9), unread: true, starred: false, atts: [] },
    { id: 'm5', ch: 'email', prov: 'imap', acct: HOME, real: true, cid: null, fromName: 'Westside Clinic', fromAddr: 'no-reply@westsideclinic.ca',
      subj: 'Appointment reminder — Tuesday 9:40am', prev: 'This is a reminder of your appointment on Tuesday at 9:40am.',
      body: 'This is a reminder of your appointment on Tuesday at 9:40am.\n\nReply CANCEL to cancel.', ts: hrs(30), unread: false, starred: false, atts: [] },
    { id: 'm6', ch: 'email', prov: 'imap', acct: HOME, real: true, cid: null, fromName: 'Shaw', fromAddr: 'billing@shaw.ca',
      subj: 'Your September statement', prev: 'Your statement is ready to view. Balance $114.20.',
      body: 'Your statement is ready to view.\n\nBalance: $114.20', ts: hrs(48), unread: false, starred: false, atts: [] },

    { id: 'm7', ch: 'email', prov: 'imap', acct: BILLING, real: true, cid: null, fromName: 'Stripe', fromAddr: 'receipts@stripe.com',
      subj: 'Payout of $6,142.00 is on its way', prev: 'Your payout will arrive in 1–2 business days.',
      body: 'Your payout of $6,142.00 will arrive in 1–2 business days.', ts: hrs(4), unread: true, starred: false, atts: [] },
    { id: 'm8', ch: 'email', prov: 'imap', acct: BILLING, real: true, cid: null, fromName: 'Priya Anand', fromAddr: 'priya@anandbooks.ca',
      subj: 'August reconciliation is done', prev: 'Two receipts missing — I have listed them.',
      body: 'August is reconciled. Two receipts missing, listed in the attachment.', ts: hrs(22), unread: false, starred: false, atts: [{ n: 'missing-receipts.xlsx' }] },
    { id: 'm9', ch: 'email', prov: 'imap', acct: LEGAL, real: true, cid: 'c1', fromName: 'Dana Whitfield', fromAddr: 'dana@brightpathdental.com',
      subj: 'Renewal terms — redline attached', prev: 'Our counsel marked up clause 7. Nothing dramatic.',
      body: 'Our counsel marked up clause 7. Nothing dramatic — take a look.', ts: hrs(7), unread: true, starred: false, atts: [{ n: 'renewal-redline.docx' }] },
    { id: 'm10', ch: 'email', prov: 'imap', acct: LEGAL, real: true, cid: null, fromName: 'Northgate Property', fromAddr: 'legal@northgateproperty.com',
      subj: 'Lease addendum for signature', prev: 'Please return signed by the 28th.',
      body: 'Please return the addendum signed by the 28th.', ts: hrs(29), unread: false, starred: true, atts: [{ n: 'addendum.pdf' }] },
    { id: 'm11', ch: 'email', prov: 'imap', acct: SUPPORT, real: true, cid: null, fromName: 'Toni Alvarez', fromAddr: 'toni@wavecrest.co',
      subj: 'Can you take on a small job in October?', prev: 'Two rooms, nothing structural. Budget is modest.',
      body: 'Two rooms, nothing structural. Budget is modest but the timeline is easy.', ts: hrs(6), unread: true, starred: false, atts: [] },
    { id: 'm12', ch: 'email', prov: 'imap', acct: SUPPORT, real: true, cid: null, fromName: 'Ken Obi', fromAddr: 'ken@obidesign.studio',
      subj: 'Re: quote request', prev: 'Thanks — the numbers work. When can you start?',
      body: 'Thanks, the numbers work. When can you start?', ts: hrs(33), unread: false, starred: false, atts: [] },
    { id: 'm13', ch: 'email', prov: 'imap', acct: OLD, real: true, cid: null, fromName: 'LinkedIn', fromAddr: 'no-reply@linkedin.com',
      subj: 'You appeared in 14 searches this week', prev: 'See who is looking at your profile.',
      body: 'See who is looking at your profile.', ts: hrs(52), unread: false, starred: false, atts: [] },
  ],
  documents: [
    { id: 'd1', name: 'lease-countersigned.pdf', fmt: 'PDF', prov: 'rata', origin: 'rata', size: '184 KB', ts: hrs(2), folder: 'Brightpath Dental', content: '' },
    { id: 'd2', name: 'q3-summary.csv', fmt: 'CSV', prov: 'rata', origin: 'rata', size: '2 KB', ts: hrs(26), role: 'source',
      content: 'Region,Q1,Q2\nWest,18400,21250\nEast,15900,16050\nNorth,9400,11020' },
    { id: 'd3', name: 'q3-summary.xlsx', fmt: 'XLSX', prov: 'rata', origin: 'rata', size: '16 KB', ts: hrs(26), role: 'converted', from: 'd2', fromName: 'q3-summary.csv', folder: 'Anand Bookkeeping' },
    { id: 'd4', name: 'invoice-4471.pdf', fmt: 'PDF', prov: 'rata', origin: 'rata', size: '96 KB', ts: hrs(5), content: '' },
  ],
  folders: ['Brightpath Dental', 'Anand Bookkeeping'],
  connections: { gmail: false, outlook: false, imap: true, slackc: false, smsc: false, discordc: false, drive: false, onedrive: false, dropbox: false },
  counters: { scans: 12, warned: 2, blocked: 1, conversions: 4, autopilot: 0 },
  audit: [],
};

const seed = `<script>
/* Preview harness — seeds a demo workspace before the app reads storage.
   None of this ships: production boots from a real session and an empty
   workspace. */
(function(){
  var UID=${JSON.stringify(UID)};
  var W=${scriptSafe(JSON.stringify(demo))};
  window.__RATA_PREVIEW__={uid:UID,workspace:W};
  function load(){
    localStorage.setItem('centra_session',JSON.stringify({uid:UID,email:'sam@reyesandco.com',mode:'local'}));
    localStorage.setItem('centra_ws_'+UID,JSON.stringify(W));
  }
  window.__rataResetDemo=function(){load();location.reload()};
  if(!localStorage.getItem('centra_ws_'+UID))load();
  else localStorage.setItem('centra_session',JSON.stringify({uid:UID,email:'sam@reyesandco.com',mode:'local'}));
})();
</script>`;

/* ---- 3. signing out has nowhere to go ---------------------------------- */
html = html.replaceAll("location.replace('auth.html')", "window.__rataResetDemo()");

/* ---- the service worker has no origin to cache -------------------------- */
html = html.replace(
  /<script>\s*if\('serviceWorker' in navigator\)[\s\S]*?<\/script>/,
  '<script>/* service worker omitted: the preview is a single file with no origin to cache */</script>'
);

/* ---- 4. the harness bar ------------------------------------------------- */
const harness = `
<style>
  /* Deliberately not styled like RATA: this is scaffolding around the product,
     and it should never be mistaken for part of it. It floats, so the app's
     own full-height layout is untouched. */
  #preview-bar{
    position:fixed;left:50%;transform:translateX(-50%);bottom:14px;z-index:9999;
    display:flex;align-items:center;gap:10px;flex-wrap:wrap;justify-content:center;
    padding:8px 12px;border-radius:10px;
    background:#16181d;color:#c9cdd6;border:1px solid #2b2f38;
    box-shadow:0 8px 28px rgba(0,0,0,.34);
    font:500 12px/1.2 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;
    letter-spacing:.01em;max-width:calc(100vw - 24px);
  }
  #preview-bar .pv-tag{color:#8b93a1;text-transform:uppercase;letter-spacing:.09em;font-size:10px;white-space:nowrap}
  #preview-bar .pv-sep{width:1px;height:18px;background:#2b2f38}
  #preview-bar button{
    font:inherit;color:#c9cdd6;background:#20242b;border:1px solid #333845;
    border-radius:6px;padding:5px 9px;cursor:pointer;
  }
  #preview-bar button:hover{background:#272c35;color:#eef1f6}
  #preview-bar button[aria-pressed="true"]{background:#3b5bdb;border-color:#3b5bdb;color:#fff}
  #preview-bar button:focus-visible{outline:2px solid #6b8afd;outline-offset:2px}
  #preview-bar .pv-hide{margin-left:2px;color:#8b93a1}
  @media (max-width:520px){ #preview-bar .pv-tag{display:none} }
  @media print{ #preview-bar{display:none} }
</style>
<div id="preview-bar" role="group" aria-label="Preview controls">
  <span class="pv-tag">Preview · not the live site</span>
  <span class="pv-sep"></span>
  <span class="pv-tag">Plan</span>
  <button type="button" id="pv-base" data-plan="base">Base $12.99</button>
  <button type="button" id="pv-pro" data-plan="pro">Pro $23.99</button>
  <button type="button" id="pv-enterprise" data-plan="enterprise">Enterprise $72</button>
  <span class="pv-sep"></span>
  <button type="button" id="pv-reset">Reload demo</button>
  <button type="button" id="pv-hide" class="pv-hide" title="Hide this bar">✕</button>
</div>
<script>
(function(){
  /* The app declares its state as a top-level "let S", which lives in the
     global lexical scope and is NOT a property of window: window.S is always
     undefined here, while a bare S resolves fine. Reading it the wrong way
     made every button on this bar silently do nothing. */
  function state(){ return typeof S!=='undefined' ? S : null; }
  var bar=document.getElementById('preview-bar');
  function mark(){
    var st=state();
    var p=(st&&st.settings&&st.settings.plan)||'free';
    bar.querySelectorAll('[data-plan]').forEach(function(b){
      b.setAttribute('aria-pressed', String(b.dataset.plan===p));
    });
  }
  bar.querySelectorAll('[data-plan]').forEach(function(b){
    b.addEventListener('click',function(){
      var st=state();
      if(!st)return;
      st.settings.plan=b.dataset.plan;
      if(typeof save==='function')save();
      /* Repaint whatever the tier changes: the file library appears on
         Business, and the billing panel states the plan. */
      /* Function declarations do land on window, unlike let. */
      ['renderFolders','renderDocs','renderPlan','renderSettings','renderAcctMenu','renderSplitLock','renderMail','renderSplit'].forEach(function(fn){
        try{ if(typeof window[fn]==='function')window[fn](); }catch(e){}
      });
      mark();
      if(typeof toast==='function')toast('Previewing the '+b.dataset.plan+' plan');
    });
  });
  document.getElementById('pv-reset').addEventListener('click',function(){window.__rataResetDemo()});
  document.getElementById('pv-hide').addEventListener('click',function(){bar.remove()});
  setTimeout(mark,400);
})();
</script>`;

/* Where to splice needs care, because app.html builds a Word document inside a
   JavaScript string and that string contains its own <head>, </head>, <body>,
   </body> and </html>. A plain .replace() takes the first match and puts two
   megabytes inside a string literal; lastIndexOf takes the last and, for
   </head>, that is also the string's copy rather than the document's.

   So anchor on the two tags where the choice is unambiguous: the document's
   opening <head> is the first in the file, and its closing </html> is the
   last. Both are checked rather than assumed. */
function insertAfterFirst(doc, tag, addition) {
  const at = doc.indexOf(tag);
  if (at < 0) throw new Error(`app.html has no ${tag}`);
  return doc.slice(0, at + tag.length) + '\n' + addition + doc.slice(at + tag.length);
}
function insertBeforeLast(doc, tag, addition) {
  const at = doc.lastIndexOf(tag);
  if (at < 0) throw new Error(`app.html has no ${tag}`);
  return doc.slice(0, at) + addition + '\n' + doc.slice(at);
}

html = insertAfterFirst(html, '<head>', seed);
html = insertBeforeLast(html, '</html>', engines + '\n' + harness);

/* The tab name should say what this is. */
html = html.replace(/<title>[\s\S]*?<\/title>/, '<title>RATA Preview</title>');

const out = join(here, '..', 'preview', 'rata-preview.html');
writeFileSync(out, html);
console.log(`wrote ${out}  (${(html.length / 1048576).toFixed(2)} MB)`);
