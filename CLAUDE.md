# Working on RATA

Read `README.md` first — it has the layout, the build and test commands, and the
full list of traps. This file is what an assistant needs that the README does
not say, and the state of play as of the last session.

## The one-line version

RATA is a desktop mail client. Mail is read on the customer's own machine over
IMAP and SMTP; the password lives in the OS keychain and never reaches a server.
`mailrata.org` sells licences and does nothing else — it has no credentials, no
connection to anyone's mailbox, and no code for one.

Every decision in this repository follows from that. If a change would put mail
or a mail password on a server, it is wrong, however convenient.

## Where things are

| Path | What |
|---|---|
| `desktop/rata-mail/` | Mail engine: DNS discovery, IMAP, SMTP, outbound guard, server-side message actions, paging back through history, what a reply needs, decoding message bodies and attachments, sending with attachments. Standalone, knows nothing about the app. 138 tests. |
| `desktop/rata-app/` | Tauri shell: keychain, store, licence verification, the bridge to the interface, saving attachments. 46 tests. |
| `rata-next/` | The website: marketing, Stripe, licence issue and renewal. Next.js on a Hostinger VPS. |
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
the original's attachments. An
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

Stored, and acted on for real: the interface keeps every fetched message in
one `localStorage` blob (`save()` in `app.html`), so mail persists between
launches and is searchable. Read, unread, star, delete and archive change RATA's
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
away. But the single `localStorage` blob has a few-MB cap, so `loadOlder`
refuses past `STORE_SOFT_CAP` — **the largest gap**, fixed by a per-message
store (IndexedDB), after which that cap can go.
**Bodies are decoded (v0.1.7).** Fetch takes `BODY.PEEK[]<0.65536>` — headers
and the start of the message, since the text cannot be decoded without the
headers that declare its encoding — and `body::read` parses it with
`mail-parser` (with `full_encoding` for CJK charsets): the text/plain part if
there is one, else `html::to_text`, our own HTML reader, which keeps block
structure and every link's address as `text <url>` and drops styles, scripts
and hidden preheaders. `html.rs` runs on a stranger's input, so its tag scan
is bounded (`MAX_TAG`) and linear; keep it that way. A part cut by the 64 KiB
limit, or text past `BODY_CHARS` (16 000), sets `truncated` and the reading
pane says so. Links are shown, never made clickable: navigating the app's
webview to an outside page would hand that page the IPC bridge. Mail stored
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
Also not built: INBOX only; no HTML rendering; no auto-update; no code
signing. Settings shows the website's plan ("No plan yet") rather than the
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
