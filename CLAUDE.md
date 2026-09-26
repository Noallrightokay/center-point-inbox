# Working on RATA

Read `README.md` first — it has the layout, the build and test commands, and the
full list of traps. This file is what an assistant needs that the README does
not say, and the state of play as of the last session.

## The one-line version

RATA is a desktop mail client. Mail is read on the customer's own machine over
IMAP and SMTP; the password lives in the OS keychain and never reaches a server.
`mailrata.org` sells licences and runs the AI relay — it has no credentials, no
connection to anyone's mailbox, and no code for one.

Every decision in this repository follows from that. The owner's two rules
(2026-09-26): RATA must never need big servers, and must never hold anything
a breach could leak. So a mail password never reaches a server, and mail
never does either, with **one** deliberate exception the owner chose: the AI
relay (`rata-next/app/api/ai`), which passes the text of a message a customer
asked to summarize or translate to Anthropic and back, stores and logs none
of it, and costs RATA at most `AI_MONTHLY_CAP_USD` ($2) per customer a month
on Haiku. Anything else that would put mail on a server is wrong, however
convenient.

## Where things are

| Path | What |
|---|---|
| `desktop/rata-mail/` | Mail engine: DNS discovery, IMAP, SMTP, outbound guard, server-side message actions, paging back through history, what a reply needs, decoding message bodies and attachments, sending with attachments, sanitising HTML. Standalone, knows nothing about the app. 145 tests. |
| `desktop/rata-app/` | Tauri shell: keychain, store, licence verification, the bridge to the interface, saving attachments. 46 tests. |
| `rata-next/` | The website: marketing, Stripe, licence issue and renewal, and the AI relay (`/api/ai`). Next.js on Hostinger. |
| `rata-next/public/app.html` | The interface. **One copy.** The desktop app builds its own from this at build time. |

There is no `src/`. Nine .NET microservices on Kubernetes were abandoned and
deleted in `064674b`. Any reference to `centerpoint-inbox.com`, an API gateway
or a translation worker is a ghost — report it.

## Before changing anything

- `desktop/rata-app/` → run `./sync-ui.sh` **before** `cargo build`, every time.
  Tauri compiles the interface into the binary; without this you are testing the
  previous build and nothing warns you.
- Do not remove `"dangerousDisableAssetCspModification": ["style-src"]` from
  `tauri.conf.json`. Without it Tauri nonces `style-src`, a nonce makes
  `'unsafe-inline'` ignored, and every inline `style=` attribute is silently
  dropped — in the packaged build only, never in `tauri dev`. That shipped in
  v0.1.2: `auth.html` hides panels with `style="display:none"`, so the first
  screen showed a password-reset form over the sign-up form. Only `style-src`
  is exempted; scripts keep Tauri's nonce and hash protection.
- Check UI changes in a **packaged** build (`cargo build --release --features
  tauri/custom-protocol`), not `tauri dev`: the two differ in exactly the ways
  that ship bugs.
- `bridge.js` replaces the browser's `fetch` to route the interface's server
  calls into Rust. Read it before touching either side.

## Releases

**To release, bump the version and merge.** `release.yml` runs on every merge
to `main` that touches the app; if `v<version>` from `tauri.conf.json` has not
been released, it builds the four installers into a draft pre-release (0.x is
beta) and publishes it once installers are attached — publishing is what
creates the tag, with the workflow's own token. A merge without a version bump
builds nothing. Nobody creates a tag by hand: that step failed three times in
four through the GitHub UI, and an assistant session cannot do it at all.

The version lives in three places, bumped together: `tauri.conf.json`
(`version` *and* the window `title`, which is how testers report their build)
and `src-tauri/Cargo.toml`.

Traps, all of which have bitten:

- A manual `workflow_dispatch` produces artifacts but **no release**. This hid
  a missing `GITHUB_TOKEN` for two runs.
- A tag build uses the workflow **as it existed at that tag**; fixing the
  workflow does not fix an existing tag.
- Pushing a `v*` tag by hand still works. It must start with `v`, and in the
  GitHub UI the tag and title are separate fields — the tag takes no spaces.
- After a release, verify the shipped binary rather than assuming: extract it and
  check the licence public key, `org.mailrata.desktop`, and that no
  `fonts.googleapis`/`fonts.gstatic` string is present.

Released: **v0.1.1**, four of five files (the Linux `.AppImage` upload failed
with `Error saving asset`, probably transient; the bundle built fine and is on
the run as an artifact). v0.1.2 shipped with a broken first screen (see the
CSP note above) and must not be given to anyone. **v0.1.3 is the beta build
testers started on**; v0.1.4 adds server-side read/star/delete/archive;
v0.1.5 adds Load older mail and shipped all five files; v0.1.6 adds Reply and
removes every control that led nowhere; v0.1.7 decodes message bodies;
v0.1.8 opens whole messages and saves attachments; v0.1.9 sends
attachments; v0.1.10 shrinks the window to fit small screens (it opened
1280×860, taller than a 1366×768 laptop, with Send below the edge — see
`fit_to_screen` in `main.rs`, which has to use the configured size because
the window reports 0×0 during setup); v0.1.11 adds Forward, which carries
the original's attachments; v0.1.12 shows HTML mail formatted; v0.1.13
keeps mail in IndexedDB, one record per message, with no storage cap;
v0.1.14 opens links in the browser, asking first for links in formatted mail;
v0.1.15 keeps a closed draft whole; v0.1.16 draws only the rows of the
list that are on screen; v0.1.17 adds AI summaries, translation and task
flags through the relay (which needs the website deployed — see below);
v0.1.18 rebuilds the Format Bridge (real .docx, structure kept, every sheet)
and makes its downloads work in the app at all; v0.1.19 reads PDFs; v0.1.20
converts an attachment straight from the message; v0.1.21 keeps message
text on disk until it is needed instead of in memory. An
upload that fails
still leaves the installer on the run as an artifact, and the release is still
published with whatever did attach.

## Permissions in this environment

An assistant session here can push branches, open PRs and merge them — which
is all a release needs now. It **cannot** push or delete tags, create or delete
releases directly, or dispatch workflows — all return 403. Do not discover this
mid-task and hand it back; say so immediately.

## Licensing

A signed Ed25519 token, verified offline against a public key compiled in at
build time via `RATA_LICENCE_PUBLIC_KEY`. Tokens last 30 days
(`LICENCE_DAYS` in `rata-next/lib/licence.js`) and the app renews itself in the
final week. The private key lives only on the VPS at `/etc/rata/licence.key`
and is minted with `/etc/rata/mint.sh <email> <plan> <days>`.

A build without the public key rejects every licence; a build with the wrong one
rejects every legitimate licence with a signature error.

## Status

**In beta.** Testers follow `BETA.md` and report through the *Beta bug* issue
template. A report's exact error text is the primary evidence — read it before
theorising, and prefer a failing test that reproduces it to a guess.

The ten findings of the 2026-09-23 code review are fixed on the way to v0.1.2:
auth classification now only believes permanent refusals during sign-in (SMTP
5xx, IMAP `NO` without a "come back later"), a locked keychain is `keychain`
rather than `auth`, Remove really removes, discovery continues past a refused
host, partial fetches and lapsed licences no longer report success.

Works: licence verification (incl. offline and renewal), adding a mailbox by
address and app password, server discovery via SRV → MX → conventional names,
fetching newest messages from INBOX across several mailboxes, sending, a
**Reply** button that threads (`replyTo` in `app.html` → `inReplyTo` →
`Message.message_id`, which `thread_id` refuses if it could smuggle a header)
and goes to the sender's Reply-To, and a guard stopping a hostile server
redirecting the app at the local network.

**Email only.** v0.1.6 removed Slack, Discord, texts, Google/Microsoft sign-in,
translation, the AI brief, the Extras/Connections/launcher screens and the
operator config rows — all were switches with nothing behind them, and
translation and the AI brief sent mail text to Google or Anthropic. `tidyV6`
strips their leftovers from an older saved workspace. Adding a mailbox has one
home: Settings → Linked accounts (`openAddMailbox`).

Stored, and acted on for real: the interface keeps every fetched message, so
mail persists between launches and is searchable. Read, unread, star, delete and archive change RATA's
copy at once and then the real mailbox (`serverAct` → `/api/mail/act` →
`change_messages` → `rata_mail::imap::act`), one connection per mailbox for a
whole selection. `act` never permanently deletes (Trash is a move to the
server's declared `\Trash`; none means refused) and never acts on a UID it
cannot vouch for (UIDVALIDITY must match; only UIDs still present are touched).
Deletions are also remembered in `S.gone` so sync does not restore them, and a
sync takes the server's read/starred for messages it already holds. What is
missing is scale. A refresh fetches the newest ~15; older mail comes 50 at a
time through **Load older mail** (`loadOlder` → `/api/mail/older` →
`older_mail` → `rata_mail::imap::fetch_older`), which pages by position below
the oldest UID RATA holds, and a newly linked mailbox gets one page straight
away.
**Where mail is kept (v0.1.13).** Each message is its own record in
IndexedDB (`rata-mail-<uid>`, store `messages`, key = id, value = the
message as JSON); everything else in `S` stays one small `localStorage`
entry, marked `store:'idb'` and written without `messages`. `S.messages`
was still all in memory then (0.1.21 moved the text out — below). `save()` finds
what changed by comparing each message with how it was last stored
(`MS_HELD`: the body by identity — nearly all the bytes, almost never
changed — and the rest as JSON), and writes only that, plus deletions, in
one transaction started in the same turn. At 10 000 messages a star costs
~27 ms (it was ~480 ms comparing whole JSON). `load()` reads both, merges
(the `localStorage` copy wins), drops anything in `S.gone` — a delete the
window closed on still holds — and moves mail an older build kept in the
entry into IndexedDB, rewriting the entry only after IndexedDB confirms.
If IndexedDB will not open (5 s timeout) mail stays in the entry the old
way and `loadOlder` keeps its `STORE_SOFT_CAP`; the two halves merge the
next time it opens. Deleting the account deletes this database too.
Verified in the packaged WebKit build: 1 500 messages (12 MB) written,
the app killed and restarted, all back with bodies and a star intact.
**Text on disk, not in memory (v0.1.21).** The database is version 2 with
two stores: `meta` (each message without its text — what start-up reads
and `S.messages` holds) and `messages` (the text, as JSON with a `body`
field). The text store keeps the old store's name on purpose: the upgrade
from version 1 leaves every whole-message record where it is and only
copies the rest of each into `meta` (`textOf` reads `body` from either
shape). Rewriting the text instead took 30 s at 10 000 messages in
Chromium here, because writes are what is slow; this way it is ~2 s once.
A message that arrives with `m.body` (sync, older mail, re-read, sent,
import) keeps it until the next `msFlush`, which moves it to `BODY_PUT`
(waiting), then `BODY_FLY` (being written — so the next save does not
write it again, which once made a relaunch wait out a 48 MB write), then
disk. **Never read `m.body`**: use `bodyNow(m)` (text if to hand, else
undefined) or `bodyOf(m)` / `bodiesOf(list)` (read from disk in one
transaction; a small LRU `BODY_CACHE` keeps what was read lately and is
only filled, never overwritten, by a read). Search (`bodiesMatching`)
walks the text store with a cursor and parses a record only when its raw
JSON matches; batching with `getAll` was slower. Export reads every text
back into the file. Without IndexedDB the old way stands: text stays on
each message in the `localStorage` entry. At 10 000 messages (48 MB of
text) in Chromium: messages held 48 MB → 2.9 MB, JS heap 103 → 13 MB,
start-up ~0.9 → ~0.75 s, search 65 → ~480 ms (it says "Searching…").
Verified in the packaged WebKit build: a 0.1.20-format mailbox of 3 000
messages upgraded in place, star kept, no text in memory, opening, search
(~250 ms), reply quoting from disk, and the same after a restart.
**The list draws a screenful (v0.1.16).** `vlist(sc,pool,tail)` puts a
spacer as tall as every row in a list and draws only the rows in view plus
`VL_MARGIN` either side, moved with `translateY`; `vlDraw` redraws on
scroll (one per frame), resize, and a `ResizeObserver` width change. Every
row is one height — subject, preview and badges each one line
(`.m-meta` no longer wraps; a missing subject shows "(no subject)") — set
as `--vl-h` from a measured row, so row i is at i × stride. A list that is
hidden when drawn (no height to measure) draws its first `VL_BLIND` rows
until it is shown. Clicks, ticks and drags are handled on the list
(`wireRows`), not per row, since rows come and go; ticks live in `SEL`, so
they survive scrolling. `vlist` restores the spacer's height before the
scroll position, or a redraw (a star) jumps to the top. At 10 000
messages drawing the list takes ~12 ms (it was ~800) and a star click
~30 ms in all. Side-by-side columns use the same list; a column that is
rebuilt drops its listeners when it finds itself detached.
**Bodies are decoded (v0.1.7).** Fetch takes `BODY.PEEK[]<0.65536>` — headers
and the start of the message, since the text cannot be decoded without the
headers that declare its encoding — and `body::read` parses it with
`mail-parser` (with `full_encoding` for CJK charsets): the text/plain part if
there is one, else `html::to_text`, our own HTML reader, which keeps block
structure and every link's address as `text <url>` and drops styles, scripts
and hidden preheaders. `html.rs` runs on a stranger's input, so its tag scan
is bounded (`MAX_TAG`) and linear; keep it that way. A part cut by the 64 KiB
limit, or text past `BODY_CHARS` (16 000), sets `truncated` and the reading
pane says so. Mail stored
by an older build (no `bodyV: 2`) is re-read by UID (`reread_mail` →
`rata_mail::imap::fetch_uids`), 50 per mailbox after each sync and at once
when opened.
**Opening in full (v0.1.8).** A message marked `truncated`, or with
attachments seen in its first 64 KiB, is fetched whole when opened
(`openWhole` → `open_message` → `rata_mail::imap::fetch_whole`, which asks
`RFC822.SIZE` first and refuses over `WHOLE_MAX`, 60 MB). The full text lives
in `OPENED` for the session only; the attachment list is stored, which is what
puts 📎 in the list. Inline images with a Content-ID are not listed. Saving
(`save_attachment`) re-fetches the message and writes one part into the
Downloads folder: the page supplies only message and index, and Rust picks the
folder, cleans the name (`safe_file_name`: no path parts, Windows device
names, or bidi controls that disguise `.exe` as `.pdf`) and creates the file
exclusively (`write_new`, `name (2).pdf`). RATA never opens a saved file.
**Sending attachments (v0.1.9).** The composer reads picked files only when
Send is pressed and hands them over as base64 (`send_mail`'s `attachments`);
`core::send` refuses over `ATTACH_MAX` (18 MB) before dialling, and
`compose::render` builds `multipart/mixed` with every part base64, so the
`=_rata_` boundary can never occur inside one. File names are cleaned for
headers (`compose::file_name`: no path, no control characters, at most 150
bytes so every header line stays under 998) and sent both as encoded-words in
`filename=` — what Gmail and Outlook send and read — and as RFC 2231
`filename*`. SMTP DATA gets a minute plus a second per 64 KiB.
**Forwarding (v0.1.11).** `forwardMessage` fetches the whole message first if
the stored copy may be partial, and the composer holds `CMP_FWD`: the
original's mailbox, UID, UIDVALIDITY and attachment indexes — never bytes.
`send_mail` takes one `draft` (clippy's 7-argument limit; `core::Draft`), and
`core::send` fetches those attachments from the mailbox (`forwarded_files`)
before the size check. The drag-between-panes forward uses the same path; it
used to announce files it never sent.
**One draft, kept whole (v0.1.15).** The composer's draft is To, Subject,
body, From, `CMP_FILES`, `CMP_FWD`, `REPLYING` and `DRAFT` (what it is, for
the title). Closing keeps all of it; Compose reopens it; Reply, Forward, a
person's Write and a `mailto:` link call `newDraft()` first; a send clears
it. Until 0.1.15 closing kept the words and dropped the rest, so a closed
forward came back saying "Forwarded message" with no files, and a reply lost
its thread. Because a reopened draft can carry a thread nobody sees, its
title says what it is ("Reply to Ann", "Forward") and **Discard** starts a
blank message.
**HTML mail (v0.1.12) — three locks, keep all three.** (1) `html::safe`
sanitises with ammonia: scripts, handlers, forms, frames, `<meta>`, `<base>`,
relative URLs and any `href` that is not http, https or mailto go; layout, `<style>`, an allowlist of
CSS properties and `cid:` pictures (inlined as `data:`, images only, capped)
stay. (2) The interface shows it in `<iframe sandbox="allow-popups">` —
never add `allow-scripts` or `allow-same-origin` (together they undo the
sandbox, and this page can call the app), nor `allow-top-navigation`,
`allow-forms` or `allow-popups-to-escape-sandbox`. `allow-popups` is there
only because a click on a link is a request for a new window (`<base
target="_blank">`), which is how Rust hears of it; no window ever opens (see
*Links* below). (3) The frame's own CSP
(`frameDoc`) fetches nothing but `https:` pictures after **Load images**; the
app CSP's `img-src … https:` exists only so that can work, and its
`frame-src 'none'` also blocks a frame navigating itself. Verified in the
packaged WebKit build with a hostile message that bypassed the sanitiser: it
rendered, and neither its script nor its `onerror` ran. Testing traps found
on the way: Chromium fires a `request` event for pictures its CSP then
refuses — count what reaches the network (a route handler or a real server),
not request events; and the desktop harness (`beta-fixes.mjs` in the session
scratchpad) now serves the page under the app's real CSP from
`tauri.conf.json`. Opening a message fetches it whole when it may have HTML
(`m.html`, or unknown for mail stored before 0.1.12).
**Links (v0.1.14).** The app's webview holds the bridge to everything, so no
outside page may ever load in it; links go to the customer's browser, and
only http and https do (`links.rs`: `classify` refuses `file:`,
`javascript:`, `data:`, custom schemes, and `user@host` addresses that
disguise the real host). The main window is built in `main.rs` rather than
from the config (`"create": false`) so it can carry two handlers.
`on_new_window` receives every click on a link in a formatted message (the
frame's only permission), always answers `Deny`, and tells the page via
`eval` of a JSON literal: `window.__rataLink({kind:'web',url,host})` shows
"Open this link in your browser?" naming the host — the words of a link can
say anything — and only **Open** calls `open_link`; `kind:'mail'` opens
RATA's composer (`writeTo`, with an empty body). `on_navigation` keeps the
window on `tauri://localhost` / `http(s)://tauri.localhost` / `about:`,
sends mailrata.org links to the browser and refuses the rest — the licence
box's "Sign in at mailrata.org" used to replace the app with the website,
with no way back. In the Text view, addresses are linked (`linkify`) and
open straight away, since the text *is* the address; bridge.js hands every
http(s) link on the app's own page to `open_link`, and `open_link` checks
again in Rust whatever the page asked. Verified in the packaged WebKit build
(a click in the frame reached `on_new_window`, the dialog named the host,
Open passed exactly that URL to `xdg-open`, `javascript:` did nothing,
`location.href` to example.com was refused). Testing trap: under Xvfb here
the webview draws at 3/4 width but takes clicks at full width — multiply a
screenshot's x by 4/3 before clicking with xdotool; y is unchanged.
**AI (v0.1.17), through RATA's relay.** Summarize, Translate (reading pane)
and Run briefing (Assist) call `askAI` → bridge `/api/ai` → `POST
https://mailrata.org/api/ai` with the licence token as the credential
(`rata-next/app/api/ai/route.js`, rules in `lib/ai.js`): plan-gated
(`translate` for Base, `ai` for Pro — `lib/plan.js`), Haiku 4.5, text capped
(12 000 chars; the briefing sends the start, 1 500 chars, of the 25 most
recent messages from the last 14 days), every prompt fences the email as
untrusted, the task list is parsed strictly (ids must be ones sent). Each
request is charged from Anthropic's reported token use into `ai_usage`
(`database.sql` §5) and refused at the month's cap; nothing is logged but a
status. The app asks once before the first AI use (`aiOk`, `AI_NOTICE`),
never sends anything on its own (opening Assist runs the local briefing), and
falls back to the local summary/briefing with the reason when AI cannot be
used. Flags are stored as `m.flag` and shown in the list. Translations live
in `TRANSLATED` for the session. **Two things found on the way:** the live
mailrata.org is an old deploy (`/api/licence/renew` is 404 there), and even
the current code never answered the app's cross-origin preflight, so licence
renewal from the app has never worked — `lib/cors.js` now allows exactly the
app's origins on `/api/ai` and `/api/licence/renew`. Neither AI nor renewal
works for testers until the site is redeployed with the variables in
`rata-next/LAUNCH.md` §4 and `database.sql` §5 has been run. Testing trap:
this container's Chromium reports `navigator.language` as `en-US@posix`,
which `Intl.DisplayNames` rejects — `userLang()` takes only the leading
letters.
**Format Bridge (v0.1.18).** Every input is read into one model —
headings 1–3, paragraphs, list items, tables, with bold/italic runs — or,
for spreadsheets, every sheet's rows (`BR.blocks` / `BR.sheets`), and every
output is written from it: `.docx` by `blocksToDocx` (a stored zip written
by `zipStore`, with Heading1–3, a list style and a bordered table style —
the old "Word" output was HTML named `.doc`), Markdown, PDF (jsPDF), `.xlsx`
with all sheets, CSV (several sheets → one CSV each in a `.zip`). `.docx` is
read through mammoth's HTML, `.html` directly — both parsed with
**DOMParser**, never `innerHTML`: a stranger's `<img onerror>` put into this
page's DOM, even detached, runs with the app's powers (the old HTML reader
did exactly that). PDF's standard fonts are Windows-1252 only, so
`pdfBlocker` refuses text outside it, naming the script, rather than
producing gibberish. Pictures are not carried over, and the note says so.
Checked by reading the outputs back with LibreOffice (install
`libreoffice-writer-nogui libreoffice-calc-nogui`; the container has only
the core) and in CI with mammoth and SheetJS. **Downloads in the app:** the
webview silently drops a page's downloads, so "Convert & download" and
Export did nothing in the desktop build; `brDownload` now sends the bytes
to `save_file` (core.rs: `safe_file_name`, `write_new`, 100 MB cap). And on
Linux Tauri's `download_dir()` exists only when the desktop has
`user-dirs.dirs` — without it every save, attachments included, failed —
so `commands::downloads` falls back to `~/Downloads`.
**PDF input (v0.1.19).** PDF.js 6.3.289, legacy build (older engines than
the modern one; the Linux webview lags), vendored as `.js` so every server
sends a script type, loaded with `import()` (`pdfLib`) and parsing in its
module worker. `pdfToBlocks` rebuilds structure from positions: runs on one
baseline make a line, and a line keeps its cells (runs more than 0.3 em
apart); two or more consecutive rows whose cell left edges line up
(within max(4pt, 0.35 em)) make a table; the body size is the size most
text is set in and the line step the median gap, so a gap over 1.3 steps
starts a paragraph; larger type is a heading (levels by size); a leading
bullet — including the Symbol/Wingdings private-use ones Word and
LibreOffice write — or number makes a list item, whose indented next line
continues it; a hyphen at a line's end before a lowercase letter joins the
word; a page break continues the paragraph when the last line had no room
for the next page's first word; lone page numbers are dropped. Scanned
(text-less), password-locked and non-PDF files each get their own message.
Limits, named in the note: layout and pictures; Chrome's PDFs draw bullets
as shapes and merge a row's cells into one run, so their lists read as
paragraphs and their tables as lines. Checked against LibreOffice and
Chrome PDFs of known documents (structure matched exactly for
LibreOffice), in CI with a LibreOffice-made fixture and jsPDF-made
hyphen, scan, locked and junk cases, and in the packaged WebKit build
(read in ~0.3 s, saved, read back by LibreOffice).
**Attachments into the Bridge (v0.1.20).** An attachment whose extension the
Bridge reads (`bridgeable`, from `BR_KINDS`) gets **⇄ Convert** beside it;
`convertAttachment` → bridge `/api/mail/attachment/read` →
`read_attachment` → `core::read_attachment`, which fetches it like a save
(`attachment_of`, shared with `save_attachment`) but hands the bytes back as
base64 instead of writing them, capped at `READ_MAX` (25 MB, kind
`too-large`). The page asks by message and index only; the bytes go to
`brIngest` as a `File`, through the same readers as a dropped file, and
nothing is saved until the customer converts.
Also not built: INBOX only; no auto-update; no code signing. Settings shows the website's plan ("No plan yet") rather than the
licence's.

**Never tested: no real mailbox has ever been opened by this code.** The suites
cover every path up to the socket and stop. When a real connection fails, the
three files to read are `resolve.rs` (discovery), `imap.rs` (handshake and
auth classification) and `guard.rs` (host refusal). Errors are typed, so the
message names the hosts tried and what each said.

## Working style that earned its place here

- Verify before asserting. Several wrong claims last session came from reasoning
  about artifacts instead of downloading and checking them.
- When CI fails, read the log before theorising. Both release failures were one
  line in the log and neither matched the first guess.
- Prefer one validated push to three speculative ones.
