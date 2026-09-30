# RATA

Your mail, on your machine.

RATA links the mailboxes you already have (Gmail, Fastmail, a work IMAP server)
and reads them on your own computer. The mail password is kept in the operating
system's keychain and never leaves the device. There is no server holding
anybody's mail, because there is no server between you and your mailbox.

That last sentence is the product. It is also why this repository looks the way
it does: the mail code is a Rust library that runs on the customer's machine,
and the website sells licences. The one exception is deliberate: when you ask
for an AI summary, translation or briefing, that text goes through RATA's AI
relay to Anthropic and back, and nothing of it is stored or logged.

---

## How mail actually flows

```
   ┌──────────────────────┐                    ┌─────────────────┐
   │  RATA on your PC     │  IMAP 993 ────────▶│  imap.gmail.com │
   │                      │  SMTP 465/587 ────▶│  (or whoever)   │
   │  password in the     │                    └─────────────────┘
   │  OS keychain         │
   └──────────┬───────────┘
              │
              │  licence: a signed token, verified offline, renewed
              │  in its last week. AI: only the text you ask it to
              │  summarise or translate, passed on and never kept.
              ▼
      ┌────────────────┐
      │  mailrata.org  │
      └────────────────┘
```

The website cannot read your mail even if it wanted to. It has no credentials,
no connection to your mailbox, and no code for one. It sees only the text you
press Summarize, Translate or Run briefing on.

---

## Repository layout

| Path | What it is |
|---|---|
| `desktop/rata-mail/` | The mail engine. A standalone Rust library: DNS discovery, IMAP, SMTP, signing in with a password or an OAuth token, and the outbound guard. Knows nothing about the app. |
| `desktop/rata-app/` | The desktop application. Tauri shell, keychain, licence verification, Sign in with Microsoft, and the glue to the interface. |
| `desktop/rata-app/harness/` | The interface driven in a browser against a fake backend (`ui-harness.mjs`), the website's screenshots (`shots.mjs`), the update feed's rules (`feed.cjs`, tested by `feed.test.cjs`), the release check (`verify-release.sh`; `--title <version>` prints the window title it expects, "beta" for 0.x only) and the installed-app smoke test the release workflow runs (`smoke-installed.sh`, `.ps1`). |
| `rata-next/` | The website: marketing pages, the help page (`public/help.html`, at `/help`), the privacy policy and terms (`public/privacy.html`, `public/terms.html`), Stripe checkout, licence issuing and renewal, the AI relay (`/api/ai`), account deletion and `/api/health`. Next.js on Hostinger's Node.js hosting. The help page quotes the app's error sentences, and a test checks each quote is still in the code, so rewording an error means updating the page. |
| `rata-next/public/app.html` | The interface. One copy, shared: the desktop app builds its own from this file. |
| `DESIGN.md` | How RATA looks: colour, type, shape and copy rules for the app and the website. Read it before changing either. |
| `infrastructure/backup/` | Nightly encrypted Postgres backup for the VPS, the restore drill, and a self-test. |
| `infrastructure/licence/` | `mint.sh`, which mints a tester's licence on the VPS and checks the signing key against the one the released app trusts. |
| `infrastructure/monitoring/` | Uptime and alerting: what to watch (`/api/health`, the backup's heartbeat, Stripe's failed deliveries) and how. |
| `docs/` | `MVP-PLAN.md` (the plan to MVP), `SECURITY-REVIEW-2026-09.md`, `SMOKE.md` (the live-provider checklist) and `MVP-EVIDENCE.md` (its results), `WINDOWS-SIGNING.md`, `hostinger-mcp.md` (managing the VPS and DNS from the repo). |

There is no `src/`. An earlier version of this product was nine .NET
microservices on Kubernetes; it was abandoned and the code removed in favour of
the two halves above. If you find a reference to `centerpoint-inbox.com`,
an API gateway, or a translation worker, it is a ghost: report it.

---

## Building and testing

### The mail engine

```bash
cd desktop/rata-mail
cargo test --all-targets   # 287 tests, no network required
cargo clippy --all-targets -- -D warnings
```

Toolchain is pinned to **1.94.1** in CI; anything from that release on works.

Those tests never open a socket beyond a scripted server of their own.
Protocol handling is tested against recorded server dialogue, which means the
suite is fast and honest about what it proves, and about what it does not.
See *What has never been tested* below.

A second suite, `tests/loopback.rs` (15 tests), runs the engine against a
real Dovecot (IMAP) and GreenMail (SMTP, and IMAPS for a server that
declares no special-use folders) on 127.0.0.1. CI runs it on every PR as
*Mail layer against real servers*. Locally (needs `dovecot-imapd`, Java and
`openssl`):

```bash
sudo desktop/rata-mail/tests/loopback/servers.sh start /tmp/rata-lb
cd desktop/rata-mail
SSL_CERT_FILE=/tmp/rata-lb/ca.crt cargo test --features loopback-tests --test loopback
sudo tests/loopback/servers.sh stop /tmp/rata-lb
```

The `loopback-tests` feature lets the outbound guard connect to 127.0.0.1,
so it refuses to compile without debug assertions, and `build.rs` refuses
it in any release-profile build even with debug assertions forced on.
Never enable it in `rata-app`.

### The desktop app

```bash
cd desktop/rata-app
./sync-ui.sh        # MUST run first: see the trap below
cd src-tauri
cargo test          # 127 tests
```

On Linux you need the system webview first:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf
```

Building installers, and where things land on a customer's machine, are covered
in **[`desktop/rata-app/README.md`](desktop/rata-app/README.md)**. Read that
before packaging anything.

### The interface, driven

`desktop/rata-app/harness/ui-harness.mjs` drives the real interface in
Chromium against a fake backend, under the app's own content security
policy (240 checks). CI runs it as *Desktop interface, driven*, together
with the update feed's rules (`node --test desktop/rata-app/harness/feed.test.cjs`). From the
repository root:

```bash
(cd desktop/rata-app && ./sync-ui.sh)
(cd rata-next && npm ci && npx playwright install chromium)   # once
(cd desktop/rata-app/ui && python3 -m http.server 3181 --bind 127.0.0.1 &)
node desktop/rata-app/harness/ui-harness.mjs http://127.0.0.1:3181
```

It exits non-zero on any FAIL.

### The website

```bash
cd rata-next
npm ci
npm run build       # npm test reads the build
npm test 2>&1 | cat # 785 checks, no network, no database required
npm run dev
```

After a deploy, `scripts/live-check.sh [https://mailrata.org]` checks the
live site from outside, with no credentials: the current routes answer, the
app's preflight is allowed and a stranger's refused, `/api/config` publishes
no service-role key, the security headers are sent, and the landing page is
the current one (`LAUNCH.md` §6). It also fails while the live privacy
policy or terms still show an `[OWNER: …]` placeholder (`LAUNCH.md`, the
step before §6). Pipe `npm test` through `cat` to count its lines: the
servers the tests start share a file's offset, so a straight redirect
garbles the log.

Fonts are vendored under `public/fonts` (Geist, Geist Mono, and Fredoka for the
wordmark only) by `node scripts/vendor-fonts.mjs`. Never link a font CDN: the
shipped app is checked for `fonts.googleapis`/`fonts.gstatic`. The landing
page's screenshots in `public/shots` are of the real interface; retake them
after a visible change rather than editing them.

---

## Traps

Every one of these cost someone a day. They are here so it is not you.

**`sync-ui.sh` must run before `cargo build`, every time.**
Tauri compiles the web assets *into* the binary at build time. Editing
`app.html` and rebuilding does nothing until you re-run `sync-ui.sh`, and the
app will keep showing the previous interface with no warning. If a change
refuses to appear, this is why.

**There is only one copy of the interface, and it lives in `rata-next/public`.**
`sync-ui.sh` copies it into `desktop/rata-app/ui/` and applies the desktop
differences loudly, in one script you can read. Do not create a second
`app.html`. The generated `ui/` directory is never committed.

**Inline `style="..."` attributes used to be silently dropped in the shipped
app.** Tauri adds a nonce to `style-src`, and per CSP a nonce makes
`'unsafe-inline'` ignored, even though the config asks for it. It never
reproduced in `tauri dev`, only in the packaged build, and it shipped in v0.1.2
as a first screen with two forms on it. `tauri.conf.json` now sets
`dangerousDisableAssetCspModification: ["style-src"]` so the declared policy is
the one that applies; scripts keep Tauri's protection. Leave it there, and check
interface changes in a packaged build.

**HTML mail is shown behind three locks. Keep all three.** It is sanitised in
Rust (`html::safe`, ammonia: no scripts, handlers, forms, frames, `<base>`,
`<meta>`, or link addresses other than http, https and mailto); shown in an
iframe with `sandbox="allow-popups"` and nothing more (no scripts, no origin
of its own; `allow-popups` is there only so a click on a link reaches Rust,
which opens no window and asks before handing the link to the browser); and
that frame's own content security policy fetches nothing but, once the
customer clicks Load images, `https:` pictures. Behind those, the app's CSP has `frame-src 'none'`, which also stops
a frame navigating anywhere, and `img-src … https:`, which only the frame's
policy narrows. Do not add `allow-scripts` or `allow-same-origin` to that
sandbox, ever: together they undo it, and the page it sits in can call the app.
Nor `allow-top-navigation`, `allow-forms` or `allow-popups-to-escape-sandbox`.

**`bridge.js` replaces the browser's `fetch`.**
The interface was written to call a server. Rather than rewrite it, the bridge
intercepts `fetch` and routes those calls to Rust instead. It is the single
most surprising file in the repository and the first one to read. A call it
does not implement is refused with a message rather than failing silently.

**The interface is email only.** Slack, Discord, texts, Google and Microsoft
sign-in, translation and the AI brief were switches with nothing behind them,
and translation and the AI brief sent the text of your mail to Google or
Anthropic. They were removed in v0.1.6, together with the screens that held
them; a workspace saved by an older build is tidied on load (`tidyV6`). Do not
bring any of them back as a control that leads nowhere. Translation and AI
summaries came back in v0.1.17 through RATA's own relay, and Sign in with
Microsoft (for a Microsoft mailbox, v0.1.39) is real OAuth with the tokens
in the keychain.

**The licence public key is compiled in at build time.**
The build reads `RATA_LICENCE_PUBLIC_KEY`. A binary built without it cannot
verify any licence, and a binary built with the wrong one rejects every
legitimate licence with a signature error. The private half lives only on the
VPS at `/etc/rata/licence.key` and is not in this repository.

**An unset GitHub secret is an empty string, not absent.**
`std::env::var` returns `Ok("")`, which Tauri reads as "yes, sign this build",
and macOS codesigning then fails confusingly. The release workflow has an
explicit opt-in step for the Apple variables because of this.

**`tauri.conf.json` builds `nsis`, not `msi`.**
The Windows installer is `RATA_<version>_x64-setup.exe`. Anything promising a
`.msi` is wrong.

---

## How licensing works

A licence is a signed token, not a subscription check:

```
v1.<base64url payload>.<base64url Ed25519 signature>
    {"v":1,"sub":"you@example.com","plan":"pro","iat":…,"exp":…}
```

The app verifies the signature against the compiled-in public key and reads the
expiry. **No network involved**: RATA works on a plane. The token is signed,
not encrypted, and is meant to be readable.

Tokens are issued for **30 days** (`LICENCE_DAYS` in `rata-next/lib/licence.js`)
and the app renews itself by presenting the old token to `/api/licence/renew`
once it is inside its final week: at start, every six hours while it is
open, when the network comes back, and whenever a refresh or the AI relay
finds the licence lapsed (v0.1.43). A renewal is kept only if it verifies,
and a pasted key that does not verify never replaces one that works or can
still renew. Thirty days covers a holiday or a dead
laptop without anyone noticing. The trade is that a cancelled subscription
stops working at the next renewal rather than instantly. That is accepted deliberately,
because the alternative is phoning home.

Releases are built by the **Release installers** workflow, which produces a
`.deb` and `.AppImage` for Linux, a `.dmg` for each Mac architecture, and an
NSIS `.exe` for Windows. They are **unsigned** so far, so Windows shows a
SmartScreen warning and macOS asks once: open it, then System Settings →
Privacy & Security → Open Anyway (macOS 15 removed right-click → Open). Signing is wired
for both (`docs/WINDOWS-SIGNING.md` for Windows) and waits for the owner's
certificates. Before an installer is kept, the workflow installs and
launches it on its own runner and checks the window title.

---

## Status: what works and what does not

Be realistic about this before promising anything to a customer.

**Works, and is tested:**

- Licence verification, including offline and self-renewal
- Adding a mailbox by address and app password, and telling a wrong
  password (`auth`) from a refused or expired OAuth token (`oauth`) and from
  a server that cannot be reached (`net`)
- Server discovery: RATA asks the domain's DNS (SRV, then MX, then conventional
  names), which is what makes `you@yourcompany.com` work when it is really Google
- New mail within seconds on servers that offer IMAP IDLE (nearly all do),
  and by itself on every server (at start, every five minutes and when you come
  back to the window) from the inbox, Sent, Archive, Spam and Drafts of several
  mailboxes at once, downloading only what is new
- Replying from the message itself, threaded with `In-Reply-To` and sent to
  the sender's Reply-To address when they gave one
- Drafts saved to the mailbox's Drafts folder (v0.1.41) when the composer
  closes and every two minutes while you write, replacing only RATA's own
  earlier copy, so a draft can be finished on a phone
- Delete, Archive and Move put a message back, and say why, when the server
  refuses (v0.1.43); Delete uses the declared Trash, or a folder named
  exactly Trash, Deleted Items, Deleted Messages, Bin or INBOX.Trash
- Unsubscribe from a mailing list's own header (v0.1.43): a web address
  opens in the browser after a prompt naming the host, a mail address opens
  the composer; RATA makes no request to the list itself
- Address suggestions in To, Cc and Bcc from mail you already hold and
  People, never from spam (v0.1.43), sent as `"Name" <address>`
- Settings → Copy diagnostics (v0.1.43): a plain-text block for a bug report
  with no address, password or message text in it, plus Help and Report a bug
- A guard that stops a hostile mail server redirecting the app at your own LAN

**Updates:** from v0.1.23 the app offers each new version itself and
installs it only if it is signed with the project's update key, once that key
is configured (`rata-next/LAUNCH.md` §9). Until then, and in any build without
it, new versions are a download from the releases page. From v0.1.43 each
kind of installation (AppImage, .deb, Windows, each Mac) reads its own feed
file first, so a platform whose installer was held back from a release
keeps being offered the last version it had.

**Not built yet:**

- **Very large mailboxes.** Each message is its own record in IndexedDB
  (v0.1.13), the list draws only the rows on screen (v0.1.16), and since
  v0.1.21 a message's text stays on disk until it is opened, replied to,
  searched or summarised, so memory holds only what the list shows (about
  3 MB for 10,000 messages, against 48 MB of text). What still grows with
  the mailbox: start-up reads every message's details (about 0.75 s at
  10,000), and a search reads through all the text on disk (about half a
  second at 10,000).
- **RATA does not add to Sent itself.** A draft begun on a phone or in
  webmail is under **Drafts** and opens with **Continue**; once sent from
  RATA, its copy goes to the Trash. There is Cc and Reply all
  (v0.1.32) and Bcc (v0.1.33). Gmail's archive is shown from v0.1.38: Gmail
  keeps archived mail in "All Mail" with everything else, so RATA asks Gmail
  which of it is archived. The inbox, Sent, Archive, Spam
  and Drafts are read on every refresh; your own folders (Gmail
  labels included) are listed under **Folders** and read when you open one,
  and **Move to** files a message into one. Mail sent from RATA appears in
  Sent only if the provider files it there (Gmail and Outlook do); RATA does
  not add it itself.
- **Mail is fetched in part first.** Each message's first 64 KB is fetched and
  decoded (MIME, quoted-printable, base64, any charset), and an HTML-only message is
  turned into text with its links' addresses kept (`body.rs`, `html.rs`).
  Opening a long message, or one with attachments, fetches all of it; an
  attachment is saved to Downloads under a cleaned-up name and never opened by
  RATA. The engine's `names.rs` cleans the name and spots a program named to
  look like a document (`invoice.pdf.exe`, also with an invisible character
  or a look-alike dot hiding the real extension), which is labelled, and
  RATA asks before saving it; on Windows and macOS every saved file is marked as
  downloaded from the internet (`mark.rs`), so the system checks it (neither
  mark has been seen on a real Windows or Mac yet). Files can be attached when sending, up to 18 MB in all, and a
  forwarded message carries its attachments. HTML mail is shown formatted
  in a locked-down frame (see *Traps*), with remote images off until asked.
  Links open in the browser: web addresses only, and from formatted mail
  only after a prompt naming where the link really goes (`links.rs`).
- **The Format Bridge reads .pdf, .docx, .xlsx, .csv, .md, .txt and .html**
  and writes real .docx, .xlsx (every sheet), .csv, Markdown and PDF, keeping
  headings, bold/italic, lists and tables (from a PDF: text, headings, lists
  and simple tables, rebuilt from where the words sit), and takes an email
  attachment straight from the message with **⇄ Convert**. Not yet: scanned
  PDFs (no OCR), .doc, .pptx or Apple files as input; pictures; PDFs of text
  outside Western European letters (refused, with the reason).
- **AI needs the website deployed.** Summaries, translation and task flags
  go through RATA's relay at `mailrata.org/api/ai` (Haiku, capped at $2 per
  customer a month, nothing stored), which is not live until the site is
  redeployed with `ANTHROPIC_API_KEY` set and `database.sql` §5 run. Only
  the first 12,000 characters of a long message are sent, and the app says
  so (v0.1.43).
- **Microsoft mailboxes** (Outlook.com, Hotmail, Live, Microsoft 365) sign
  in with **Sign in with Microsoft** from v0.1.39, in a build made with the
  owner's Microsoft client id; a build without it says they cannot be added
  yet. Not yet seen against a real Microsoft sign-in.
- Code signing: wired for Windows and macOS, waiting for certificates.

### What has never been tested

**No customer's mailbox has been opened yet.** The engine runs against a real
Dovecot and GreenMail on every PR (`tests/loopback.rs`), but not against
Gmail, Outlook or any hosted provider. The classification that decides
"wrong password" (`auth`) versus "refused token" (`oauth`) versus "server
unreachable" (`net`), and the discovery chain against real DNS, have never
run against a live provider.

That is what `docs/SMOKE.md` is for: a checklist, one column per provider,
that the owner runs on real machines and records in `docs/MVP-EVIDENCE.md`.
If you are picking this up, that is the first thing to do, and the three files
worth reading first when it misbehaves are `resolve.rs` (finding the server),
`imap.rs` (handshake and authentication), and `guard.rs` (host refusal).
Errors are typed rather than stringly, so the message you get back names the
hosts it tried and what each one said.
