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
| `desktop/rata-mail/` | Mail engine: DNS discovery, IMAP, SMTP, outbound guard. Standalone, knows nothing about the app. 73 tests. |
| `desktop/rata-app/` | Tauri shell: keychain, store, licence verification, the bridge to the interface. 36 tests. |
| `rata-next/` | The website: marketing, Stripe, licence issue and renewal. Next.js on a Hostinger VPS. |
| `rata-next/public/app.html` | The interface. **One copy.** The desktop app builds its own from this at build time. |

There is no `src/`. Nine .NET microservices on Kubernetes were abandoned and
deleted in `064674b`. Any reference to `centerpoint-inbox.com`, an API gateway
or a translation worker is a ghost — report it.

## Before changing anything

- `desktop/rata-app/` → run `./sync-ui.sh` **before** `cargo build`, every time.
  Tauri compiles the interface into the binary; without this you are testing the
  previous build and nothing warns you.
- Styling in the desktop app goes in `bridge.css`. Inline `style=` attributes are
  silently dropped in the packaged build (Tauri nonces `style-src`, which makes
  `'unsafe-inline'` ignored) but work fine in `tauri dev` — so this only shows up
  after packaging.
- `bridge.js` replaces the browser's `fetch` to route the interface's server
  calls into Rust. Read it before touching either side.

## Releases

Tag → build → installers attach to the release page. The traps, all of which
have bitten:

- The tag **must** start with `v` (`v0.1.1`). The workflow ignores anything else.
- Tag and title are different fields. The tag takes no spaces; the title does.
- A manual `workflow_dispatch` produces artifacts but **no release** — it leaves
  `tagName` empty and skips that step. This hid the missing `GITHUB_TOKEN` for
  two runs.
- A tag build uses the workflow **as it existed at that tag**. Fixing the
  workflow does not fix an existing tag; cut a new one.
- After a release, verify the shipped binary rather than assuming: extract it and
  check the licence public key, `org.mailrata.desktop`, and that no
  `fonts.googleapis`/`fonts.gstatic` string is present.

Released: **v0.1.1**, four of five files (the Linux `.AppImage` upload failed
with `Error saving asset`, probably transient; the bundle built fine and is on
the run as an artifact). **v0.1.2 is the beta build** — the app version lives in
both `tauri.conf.json` (`version` *and* the window `title`, which is how testers
report it) and `src-tauri/Cargo.toml`; bump all three together.

## Permissions in this environment

An assistant session here can push branches, open PRs and merge them. It
**cannot** push or delete tags, create or delete releases, or dispatch
workflows — all return 403. Do not discover this mid-task and hand it back;
say so immediately and ask whether to get a token instead.

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
fetching newest messages from INBOX across several mailboxes, sending with
correct reply threading, and a guard stopping a hostile server redirecting the
app at the local network.

Stored, but not synced: the interface keeps every fetched message in one
`localStorage` blob (`save()` in `app.html`), so mail persists between launches,
is searchable, and has local read/unread, star and delete (deletions are
remembered in `S.gone` so sync does not restore them). What is missing is the
server side and scale: none of those actions reach the real mailbox; each
refresh fetches only the newest ~15, with no backfill of older mail; and the
single blob hits `localStorage`'s few-MB cap after heavy use — **the largest
gap**, fixed by a per-message store (IndexedDB) plus server-side actions.
Also not built: INBOX only, no archive; plain text only, no HTML or
attachments; the interface still shows Slack, AI and file
browsing controls that now refuse; no auto-update; no code signing.

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
