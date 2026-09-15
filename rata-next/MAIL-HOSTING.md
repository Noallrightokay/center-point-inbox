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
| Base ($8) | — | — |
| Pro ($16) | yes | $1.50 / month per domain |
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

So: **receive directly, relay outbound through a service with its own IP
reputation.** A few dollars a month, reversible once RATA's own IP has a history.
Sending directly from the VPS is the cheaper decision and the one most likely to
produce a product that appears to work and does not.

**Undecided.** Which relay, or whether to attempt direct sending first, has not
been settled — it wants a check of whether Hostinger permits outbound 25 from
this VPS at all, and whether the IP is already on the major blocklists.

## Two things to settle before it is public, not after

**Sending caps per account.** Selling a mailbox for $16 means somebody will buy
one in order to send from it. One spam run gets `mailrata.org` blocklisted, and
a blocklisted domain does not damage the spammer — it damages every honest
customer's mail at once, including any RATA sends itself. A per-account daily
ceiling and a way to cut an account off are cheaper written now than during the
incident.

**Disk.** Real mailboxes are real storage, on a VPS already running Supabase and
Caddy. Worth a number before selling it rather than after.

## What the app gains at the end

An endpoint to create an address, a screen to manage them, and `mailrata.org` in
the `MAIL_HOSTS` table so a RATA address links like any other. The credential
for a RATA-hosted mailbox goes through `lib/secrets.js` like every other one —
the fact that RATA also runs the server on the far side changes nothing about
how the password is stored.
