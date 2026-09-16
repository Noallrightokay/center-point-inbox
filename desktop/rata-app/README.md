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
cd src-tauri

# The licence key must be compiled in. A build without it refuses every
# licence — which is the safe direction to fail in, and not one you want to
# discover after shipping.
export RATA_LICENCE_PUBLIC_KEY="$(cat /path/to/licence.pub)"
cargo build --release
cargo tauri build                   # installers: .deb/.AppImage, .dmg, .msi
```

`licence.pub` is the public half of the pair in `LICENSING.md`. It is not a
secret — it can only check signatures, not make them — so it belongs in the
build, in CI, and anywhere else convenient. The private half never leaves the
server.

Installers must be built on the platform they target — a Linux machine cannot
produce a signed .dmg or .msi. That is three build machines, or three CI runners.

## CI

`.github/workflows/desktop-ci.yml`, in two jobs. **Mail layer** needs nothing
installed and answers in about a minute; **Desktop shell** installs the system
webview and runs `sync-ui.sh` first, which makes that script's three anchor
checks against `rata-next/public` part of CI rather than something that fails on
a release machine. Both run `cargo fmt --check`, `cargo clippy -- -D warnings`
and `cargo test`.

The toolchain is pinned rather than `stable`. With `-D warnings`, an unpinned
toolchain means a new clippy lint reddens an unrelated pull request on the day
it ships, which teaches everyone to ignore the colour.

Some of the mail tests resolve real names — `anthropic.com` has no mail server
of its own and its MX is the case the whole discovery module exists for, so a
test that stubbed DNS would be testing the stub. They assert on the shape of the
answer rather than on a specific record.

## One interface, not two

`ui/` is generated and gitignored. `sync-ui.sh` copies `rata-next/public` and
applies exactly three differences, each of which fails loudly if its anchor
moves:

1. `bridge.js` is injected after `config.js`.
2. `config.js` is replaced with a device one — no Supabase, so `app.html` falls
   into the on-device account mode it already has.
3. The fonts are checked, not stripped. They used to be stripped, because a
   local-first mail app that contacts Google on every launch tells Google when
   its customer opens their mail — they are vendored now, in
   `rata-next/public/fonts`, so there is nothing to remove. A page that
   reintroduced a Google link would put that request back into an app with no
   network, where the content security policy would block it silently, so the
   script fails on one instead.

Run `sync-ui.sh` **before** `cargo build`, not after. Tauri compiles the web
assets into the binary, so building first produces an app carrying whatever was
in `ui/` last time — which looks exactly like a change that did not take.

The interface reaches the outside world through one function, `apiFetch`, and
`bridge.js` answers it. That is the entire desktop-specific frontend.

## The attack surface from the page

Six commands, and nothing else. No filesystem plugin, no shell plugin, no
arbitrary HTTP. A script that somehow reached a rendered message can ask to
refresh the mail; it cannot ask to read `~/.ssh`.

```
licence_status  set_licence  link_mailbox  list_mailboxes
unlink_mailbox  retry_mailbox  refresh_mail  send_mail
```

Verified rather than assumed: `ui-src/probe.html` calls each one and also asks
for a command that does not exist. Copy it over `ui/index.html`, run the app,
and read the answers — the last one should say `Command read_file not found`.

## The licence

Signed, not looked up. The app reads mail straight from the customer's mail
server, so making it ask us for permission would mean their mail stops working
when we do — and would be useless on a train. Instead the server issues a small
Ed25519-signed token and the app checks it locally, with no network.

`src/licence.rs` is the other half of `rata-next/lib/licence.js`, and there is a
test here that verifies a token produced by that file. Two implementations of
one signature check that disagree is precisely the bug that locks paying
customers out, and neither codebase's own tests would find it.

**What a licence gates.** Linking a mailbox, refreshing, and sending. There is
no free tier — someone without a subscription is not on a cheaper plan, they are
unsubscribed — so all three stop. What does *not* stop is reading what is
already on the machine: the account list, the stored passwords and the
downloaded mail all stay exactly where they are, because they are theirs.

**Thirty days offline**, then it renews itself. The old token is the credential
for renewal — it is signed with a key only the server holds, so presenting one
proves where it came from, and an expired one is accepted because renewing an
expired licence is the whole job. The request is made from `bridge.js` rather
than from Rust, so the app needs no HTTP client and the single address it may
contact is one line of the content security policy.

So a cancelled subscription keeps working for up to a month. That is the
deliberate trade, and `LICENCE_DAYS` is the one place to change it.

## What is not done yet

- **OAuth providers** (Outlook, Slack, Google sign-in) need a server to receive
  the redirect. The bridge says so plainly. Every IMAP mailbox, Gmail and
  Outlook included, links here with an app password.
- **No installer has been produced.** The app builds and runs on Linux; nothing
  has been packaged or signed for any platform.
- **No mailbox has been opened for real.** The container this was written in
  blocks 993, 465 and 587, so every network path is tested up to the socket and
  no further. Renewal has not been exercised against a live server either: the
  endpoint has tests, but no app has renewed against a deployed `mailrata.org`.
- **The interface has no licence screen of its own.** `bridge.js` puts up a box
  asking for the key when there is no usable one. That is desktop-only on
  purpose — the website has no key to type, and giving `app.html` a field that
  appears in one build of two is how one interface becomes two — but it is a
  plain box rather than a designed screen.
