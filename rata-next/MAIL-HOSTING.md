# RATA-hosted mail — what it takes

Pro and Enterprise members can be given addresses RATA hosts itself, rather
than only connecting mailboxes they already have elsewhere.

**The app needs almost nothing for this.** A RATA mailbox is an IMAP/SMTP
mailbox, and RATA links any IMAP/SMTP mailbox already — `discoverHosts()` would
resolve `mailrata.org` from the table the same way it resolves Fastmail. What
does not exist is the mail server. This is an infrastructure project on the VPS
with a small amount of app work at the end, not the other way round.

The entitlement half **is** built: `PLANS` in `lib/plan.js` carries `ratamail`
and `domains`, the Stripe webhook records purchased domains, and
`domainRefusal()` offers the add-on rather than an upgrade. None of that waits
on the server.

## What is sold

| | Addresses at mailrata.org | Mail on a domain you own |
|---|---|---|
| Base ($12.99) | — | — |
| Pro ($23.99) | yes | $1.50 / month per domain |
| Enterprise ($72) | yes | included, no limit |

## What has to exist

**1. A mail server.** [Stalwart](https://stalw.art) is the recommendation: one
binary carrying IMAP, SMTP submission, DKIM signing and spam filtering, against
Postfix + Dovecot + OpenDKIM + Rspamd wired together by hand. Fewer moving
parts is worth a great deal on a box that also runs Supabase.

**2. DNS.** As of this writing `mailrata.org` has **no MX record and no
SPF/DKIM/DMARC**, so it receives nothing today. Needed:

- `MX` → the VPS
- `SPF` (TXT) naming whatever is permitted to send
- `DKIM` (TXT) with the server's public key
- `DMARC` (TXT), starting at `p=none` while the other two are proved
- `PTR` on the VPS IP, set through Hostinger — receiving servers check that the
  IP's reverse name matches the name it greets them with, and a mismatch alone
  is enough to be filtered

**3. An outbound path that actually lands.** This is the part that decides
whether the feature is worth having.

Receiving directly on the VPS is straightforward. Sending is not: a fresh IP has
no sending reputation, consumer hosting ranges are widely filtered regardless of
reputation, and outbound port 25 is frequently blocked by the provider. The
consequence is not an error the customer can see — their mail is accepted, and
then silently filed as spam by the recipient. They will blame RATA, and they
will be right.

**Decided: receive directly on the VPS, relay outbound through a provider.** A
few dollars a month, reversible once RATA's own IP has a history. Sending
directly is the cheaper decision and the one most likely to produce a product
that appears to work and does not.

### The trap in choosing one

Most of the well-known providers here — Postmark, SendGrid, Mailgun, Resend —
sell **transactional email**: mail an application generates, like a receipt or
a password reset. Several of them prohibit, in their terms, using the service as
a general relay for mail that end users compose in their own mailboxes. That is
exactly what RATA would be doing.

This matters because the failure is not a rejected signup. The account works,
the business runs on it, and it is closed later for a terms violation — taking
every customer's outbound mail with it, at whatever moment the provider notices.

So the question to ask any provider before paying them is not "can you handle
the volume" but **"may I relay mail that my users wrote, from their own
mailboxes, under my domain?"** Get the answer in writing.

**Amazon SES** is the recommendation, because it is sold as an SMTP relay rather
than as a transactional API, it is what mail hosts commonly sit behind, and the
cost is negligible ($0.10 per thousand). Two consequences to plan for:

- **Production access.** New accounts are sandboxed and can only send to
  verified addresses. Leaving the sandbox is a request with a written
  description of what is being sent. Do this before promising anyone a launch
  date.
- **Bounces and complaints are yours to handle.** SES will suspend an account
  whose bounce or complaint rate climbs. That means consuming the notifications
  and cutting off a mailbox that generates them, not ignoring them.

### And what it costs the custom-domain add-on

A relay will only send as a domain that has been verified in it, with its DKIM
records published. So every customer domain sold at $1.50 a month needs
verifying in the relay as well as pointing its MX here. That is per-customer
setup work, and it is the real reason the add-on is not free — worth remembering
if the price is ever revisited.

## Two things to settle before it is public, not after

**Sending caps per account.** Selling a mailbox for $23.99 means somebody will buy
one in order to send from it. One spam run gets `mailrata.org` blocklisted, and
a blocklisted domain does not damage the spammer — it damages every honest
customer's mail at once, including any RATA sends itself. A per-account daily
ceiling and a way to cut an account off are cheaper written now than during the
incident.

**Disk.** Real mailboxes are real storage, on a VPS already running Supabase and
Caddy. Worth a number before selling it rather than after.

## The DNS, exactly

For `mailrata.org`, once the server is up and the relay is chosen:

| Type | Name | Value |
|---|---|---|
| MX | `@` | `mail.mailrata.org` (priority 10) |
| A | `mail` | the VPS IP |
| TXT | `@` | `v=spf1 include:<the relay's SPF domain> -all` |
| TXT | `<selector>._domainkey` | the DKIM public key the relay issues |
| TXT | `_dmarc` | `v=DMARC1; p=none; rua=mailto:dmarc@mailrata.org` |
| PTR | the VPS IP | `mail.mailrata.org` — set in hPanel, not in the zone |

`-all` rather than `~all` once the relay is confirmed as the only sender: a soft
fail invites spoofing of your own domain. Start DMARC at `p=none`, read the
reports for a fortnight, then move to `p=quarantine`.

The PTR is the one most often missed, and a mismatch between it and the name the
server greets with is on its own enough to be filtered.

## Verifying it before selling it

1. Send to a `mail-tester.com` address — it scores SPF, DKIM, DMARC, PTR and
   blocklist status in one page. Below 10/10, fix it before continuing.
2. Send to a Gmail address and a Microsoft one, and read the headers: `spf=pass`,
   `dkim=pass`, `dmarc=pass`.
3. Check the VPS IP against the major blocklists even though the relay sends —
   because receiving is still direct, and a listed IP affects what will talk
   to you.
4. Send **from** a RATA address **to** a RATA address, and from outside in. It is
   possible to have outbound perfect and inbound silently rejecting.

## What the app gains at the end

An endpoint to create an address and a screen to manage them. The linking half is
already built and sitting inert: `RATA_MAIL_HOST` (and optionally
`RATA_SMTP_HOST`, and `RATA_MAIL_DOMAIN` for a staging deployment) turns
RATA-hosted addresses on. Until that variable is set, an `@mailrata.org` address
is treated as any other unknown domain — which is deliberate, because pointing
users at a server that is not answering and calling it a mail problem is worse
than not offering it at all.

The credential for a RATA-hosted mailbox goes through `lib/secrets.js` like every
other one. RATA running the server on the far side changes nothing about how the
password is stored — if anything it matters more, since the same database would
otherwise hold both halves.
