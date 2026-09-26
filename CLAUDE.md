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
| `desktop/rata-mail/` | Mail engine: DNS discovery, IMAP, SMTP, outbound guard, server-side message actions, paging back through history, what a reply needs. Standalone, knows nothing about the app. 98 tests. |
| `desktop/rata-app/` | Tauri shell: keychain, store, licence verification, the bridge to the interface. 39 tests. |
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
removes every control that led nowhere. An upload that fails
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
**Bodies are not decoded — the gap a tester will hit first.** Fetch takes
`BODY.PEEK[TEXT]<0.2048>` and `words::plain` only strips tags and collapses
whitespace: no MIME parsing, no quoted-printable or base64. Multipart mail
(nearly all of it) shows boundaries and encoded text in the reading pane, cut
at 2 KB. Fix with a real MIME parser in `rata-mail`, fetching the whole
message (or BODYSTRUCTURE + the text part); HTML and attachments build on it.
Also not built: INBOX only; no HTML or attachments; no auto-update; no code
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
