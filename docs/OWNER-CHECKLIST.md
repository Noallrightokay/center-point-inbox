# The owner's checklist

Everything below needs the owner: an account, a payment, a secret, a server
login or a decision. No assistant session can do these steps, and none
should ever be asked for a key, token or password in chat. Each item says
what it unlocks, roughly how long it takes, where the full steps are, and
how to tell it worked. They are in the order that unblocks the most.

Where a step ends in "a GitHub **Variable**" or "a GitHub **Secret**", it is
GitHub → the repository → Settings → Secrets and variables → Actions. The
next release built after it is set picks it up; tell the assistant and it
will build one.

## 1. Licence keys for your testers (minutes, each)

**Unlocks:** RATA opening mail at all. Without a key it opens but reads
nothing.

On the VPS: `infrastructure/licence/mint.sh <their email> pro 30`, and send
them the key it prints. Run `infrastructure/licence/mint.sh check` once
first: it must say the key is the one the released installers trust.

**Worked when:** they paste it and Settings → Current plan says "Licensed for
RATA Pro until …".

## 2. The update key (about 5 minutes)

**Unlocks:** RATA updating itself. Until then every tester installs each
version by hand.

Steps: `rata-next/LAUNCH.md` §9 (make a key pair on your own computer, add
three **Secrets**: `TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, `RATA_UPDATER_PUBKEY`; back the key up
offline).

**Worked when:** the next release's run log says "Signing this build's
installers for updates" and a release named *Update feed (not an
installer)* appears. That release is the last one anyone installs by hand.

## 3. The website (an hour or two, once)

**Unlocks:** AI summaries and translation, licence renewal from the app,
buying on mailrata.org, the current help page and privacy policy. The live
site is an old build.

1. `rata-next/LAUNCH.md` §0 to §4: the database sections, the licence
   signing key (the VPS's existing one, never a new one), Stripe and the
   environment variables.
2. A Hostinger API token (`docs/hostinger-mcp.md` §1), put in this
   environment's settings as `HOSTINGER_API_TOKEN`, so the assistant can
   deploy. Then tell it, in so many words, to deploy.
3. Fill in the `[OWNER: …]` parts of the privacy policy and terms
   (`LAUNCH.md`, before §6).

**Worked when:** `rata-next/scripts/live-check.sh` passes, and LAUNCH.md §6's
checks pass.

## 4. Microsoft (an hour, plus Microsoft's verification)

**Unlocks:** Outlook, Hotmail and Microsoft 365 mailboxes (Sign in with
Microsoft), and OneDrive in Connected accounts (Pro).

Steps: `rata-next/LAUNCH.md` §10 for the app registration, then
`docs/CONNECTIONS-SETUP.md` → OneDrive for the extra Graph permission
(`Files.ReadWrite`). One **Variable**: `RATA_MS_CLIENT_ID`. Publisher
verification is strongly recommended (work accounts warn without it).

**Worked when:** in the next release, adding an `@outlook.com` address
offers Sign in with Microsoft, and Settings → Connected accounts lists
OneDrive.

## 5. Slack (30 minutes)

**Unlocks:** Share to Slack (Pro): a message or file to a channel or person.

Steps: `docs/CONNECTIONS-SETUP.md` → Slack. Register all three redirect
addresses exactly (`http://localhost:28417`, `:28418`, `:28419`), turn on
PKCE, add the seven user scopes. One **Variable**: `RATA_SLACK_CLIENT_ID`.
Never copy the client secret anywhere.

**Worked when:** Settings → Connected accounts lists Slack, and Connect opens
Slack's page in the browser.

## 6. Google Drive (an hour, plus Google's brand verification, days)

**Unlocks:** Google Drive in Connected accounts (Pro), through Google's own
file chooser.

Steps: `docs/CONNECTIONS-SETUP.md` → Google Drive (a Cloud project, the Drive
and Picker APIs, the consent screen asking for `drive.file` only, a Desktop
client). Two **Variables**: `RATA_GOOGLE_CLIENT_ID` and
`RATA_GOOGLE_CLIENT_SECRET`. Never add the `drive`, `drive.readonly` or Gmail
scopes: they need a paid yearly assessment.

**Worked when:** Connect on the Google Drive row opens Google's file chooser.

## 7. Code signing (a purchase, then about an hour each)

**Unlocks:** no "Windows protected your PC" and no macOS "Open Anyway".

- Windows: `docs/WINDOWS-SIGNING.md` (Azure Artifact Signing or a `.pfx`,
  never both).
- macOS: an Apple Developer membership ($99 a year) and six **Secrets**,
  `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`
  (the signing rows of `docs/MVP-PLAN.md`; the wiring is in
  `.github/workflows/release.yml`).

**Worked when:** the release run's signing steps pass; on a Mac,
`spctl -a -vv /Applications/RATA.app` says accepted, notarized.

## 8. Try it with real mailboxes (an afternoon)

**Unlocks:** the MVP's evidence. No customer mailbox has been opened by RATA
yet; it is tested against real mail servers set up for testing, not against
Gmail or Outlook.

Steps: `docs/SMOKE.md`, results in `docs/MVP-EVIDENCE.md`. While there: open
a file from Files → Create file in Word, Excel and PowerPoint (and Pages,
Numbers, Keynote on a Mac) and say if any complains, and try Convert on a
PDF on a Mac.

## 9. Decisions only you can make

- **Sent copies.** Some providers do not file mail sent over SMTP into Sent
  (Gmail and Outlook do). Should RATA put a copy there itself?
- **Email confirmation before paid launch** (`LAUNCH.md` §7; the security
  review's open Medium, D8).
- **Renewals inside the 90-day window** (security review row 15): a copy
  that keeps renewing is not stopped. A business decision.
- **More extensions in the disguised-program lists** (security review P3-4).
