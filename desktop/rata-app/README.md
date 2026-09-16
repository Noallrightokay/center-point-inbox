# RATA for the desktop

The app that replaced the server.

RATA used to read everybody's mail from a VPS, which meant holding everybody's
mail password. That is a thing worth attacking, and it is a thing worth
attacking *once* — one break, every customer. This app holds nobody's: it runs
on the machine that owns the mailbox, the passwords are in that machine's own
credential store, and the only thing left on a server is the landing page, the
payment link and a signature.

```
desktop/
  rata-mail/          the mail layer — DNS, IMAP, SMTP, the outbound guard
  rata-app/
    src-tauri/        the shell: vault, store, core, commands
    ui-src/           bridge.js — the seam between the interface and Rust
    ui/               assembled at build time, never committed
```

## Where things live on a customer's machine

| What | Where | Why there |
|---|---|---|
| Mail passwords | The OS credential store — Keychain, Credential Manager, Secret Service | Encrypted at rest, tied to the login session, and already trusted with these same passwords by every mail client they have used |
| Mailbox list | `mailboxes.json` in the app's data directory | Metadata only. There is a test asserting no password ever reaches it |
| Messages | The webview's own storage, as on the web | Unchanged from the website |

**There is no fallback to a file for passwords.** If the credential store cannot
be reached, linking fails and says so. The tempting alternative — an encrypted
file with the key beside it — sounds like resilience and is a plaintext password
with extra steps.

## Building

```bash
# Linux only
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf

./sync-ui.sh                        # assemble ui/ from rata-next/public
cd src-tauri && cargo build --release
cargo tauri build                   # installers: .deb/.AppImage, .dmg, .msi
```

Installers must be built on the platform they target — a Linux machine cannot
produce a signed .dmg or .msi. That is three build machines, or three CI runners.

## One interface, not two

`ui/` is generated and gitignored. `sync-ui.sh` copies `rata-next/public` and
applies exactly three differences, each of which fails loudly if its anchor
moves:

1. `bridge.js` is injected after `config.js`.
2. `config.js` is replaced with a device one — no Supabase, so `app.html` falls
   into the on-device account mode it already has.
3. The Google Fonts links are removed. A local-first mail app that contacts
   Google on every launch tells Google when its customer opens their mail. **The
   interface falls back to the system font until the three families are
   vendored** — a visible difference, and a known one.

The interface reaches the outside world through one function, `apiFetch`, and
`bridge.js` answers it. That is the entire desktop-specific frontend.

## The attack surface from the page

Six commands, and nothing else. No filesystem plugin, no shell plugin, no
arbitrary HTTP. A script that somehow reached a rendered message can ask to
refresh the mail; it cannot ask to read `~/.ssh`.

```
link_mailbox  list_mailboxes  unlink_mailbox  retry_mailbox  refresh_mail  send_mail
```

Verified rather than assumed: `ui-src/probe.html` calls each one and also asks
for a command that does not exist. Copy it over `ui/index.html`, run the app,
and read the answers — the last one should say `Command read_file not found`.

## What is not done yet

- **The licence is not checked.** `rata-next/lib/licence.js` issues Ed25519
  tokens and the verifier is written, but nothing in this app reads one, so
  nothing is gated. `/api/links` reports that plan limits are unchecked rather
  than claiming "no limit", which would be a claim rather than a fact.
- **The fonts are not vendored** — see above.
- **OAuth providers** (Outlook, Slack, Google sign-in) need a server to receive
  the redirect. The bridge says so plainly. Every IMAP mailbox, Gmail and
  Outlook included, links here with an app password.
- **No installer has been produced.** The app builds and runs on Linux; nothing
  has been packaged or signed for any platform.
- **No mailbox has been opened for real.** The container this was written in
  blocks 993, 465 and 587, so every network path is tested up to the socket and
  no further.
