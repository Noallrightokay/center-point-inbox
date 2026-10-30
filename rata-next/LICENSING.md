# Licensing the desktop app

RATA runs on the customer's machine and talks to their mail servers directly.
It cannot ask a server whether they have paid — not without being useless on a
train, and not without making RATA's uptime a condition of reading your own
mail.

So a licence is **signed, not looked up**. The server holds an Ed25519 private
key and issues a small token saying who paid and for what; the app ships with
the matching public key and verifies that token with no network at all.

```
v1.<payload>.<signature>        payload is base64url JSON, readable on purpose
{ "v":1, "sub":"buyer@example.com", "plan":"pro", "iat":…, "exp":… }
```

Nothing secret is in there. "This person bought Pro" is not a secret, and a
customer being able to read what they were issued is a feature the first time
something goes wrong.

---

## 1. Generate the key pair

**RATA already has its pair.** The private half is on the VPS at
`/etc/rata/licence.key`, and the public half is compiled into every installer
since v0.1.3. For the website, use that key (LAUNCH.md §2); a new one would
make every licence the site issues fail in every copy of the app already
installed. The command below is for a deliberate rotation only, which also
means shipping a new app build with the new public half.

Once, on your own machine, from the repository root. Never in a browser,
never through chat. `lib/licence.js` is an ES module, so it is imported, not
`require()`d (that fails with `ERR_REQUIRE_ESM` before Node 22.12):

```bash
node --input-type=module -e "import { generateKeys } from './rata-next/lib/licence.js'; const k = generateKeys();
console.log('--- PRIVATE (server only) ---\n' + k.privateKey);
console.log('--- PUBLIC (ships in the app) ---\n' + k.publicKey)"
```

| Half | Where it goes | If it leaks |
|---|---|---|
| **Private** | `LICENCE_PRIVATE_KEY` on the server, nowhere else | Anyone can mint licences. Rotate it, ship a new app build, and every old licence stops verifying. |
| **Public** | Embedded in the desktop app at build time | Nothing. It only checks signatures; it cannot make them. |

Back the private key up somewhere durable and separate. Losing it does not
break existing licences — they keep verifying against the public key until they
expire — but you cannot issue or renew any until you replace it, which means
shipping a new app build with a new public key.

## 2. Set it on the server

`LICENCE_PRIVATE_KEY` alongside the other server-only variables in
`LAUNCH.md`. Hosting panels mangle multi-line values, so the loader accepts the
PEM either with real newlines or with them written as `\n`.

## 3. Put the public half in the app build

The desktop app checks licences against a key compiled into it:

```bash
cd desktop/rata-app && ./sync-ui.sh && cd src-tauri
RATA_LICENCE_PUBLIC_KEY="$(cat licence.pub)" cargo build --release --features tauri/custom-protocol
```

Releases get it from the GitHub secret `RATA_LICENCE_PUBLIC_KEY`, and
`release.yml` stops before building if that secret is empty.

A build without it refuses every licence. That is the safe direction to fail in
— the alternative, treating a missing key as "no checking needed", turns a build
mistake into a free product — but it is not a thing to discover after shipping,
so check one installer before releasing.

## 4. The flow

1. Customer buys through the Stripe payment link. The webhook writes
   `subscriptions` as it does today — nothing changes there.
2. They sign in on the website. `GET /api/licence` signs a licence for the
   subscription on that account and the page shows them the key.
3. They paste it into the app once. From then on it is checked locally, with no
   network at all.
4. Whenever the app is online and the licence is inside its last week, it posts
   the licence it holds to `POST /api/licence/renew` and gets a fresh 30 days.
   Nobody is asked to do anything.

The first licence is issued against the **signed-in account**, never an address
in the request body — the subscription is keyed on email, so accepting a
caller-supplied one would hand a licence to anyone who could guess a customer's
address.

### Why renewal has no sign-in

Because the old licence is the credential. It is signed with a key only the
server holds, so presenting one proves it was issued here, and the address
inside it is the address the subscription is keyed on. An expired licence is
accepted deliberately, because renewing an expired licence is the entire job,
but only up to `RENEW_GRACE_DAYS` (90, `lib/licence.js`) after it expired.
An older one is refused with `reason: 'too-old'` before the database is
asked, and the app shows the server's sentence: sign in at mailrata.org to
get a new one. So a token that leaked long ago cannot be revived.

**A licence from the App Store.** Since subscriptions are sold in the iPhone
app, `sub` may instead be `apple:<originalTransactionId>`, Apple's number for
the purchase, never an address. Such a licence renews from the
`apple_subscriptions` row Apple's signed purchases and notifications keep,
not from `subscriptions`, and its refusals never send anyone to sign in at
mailrata.org, where a purchase on the phone has no account: one too old to
renew itself, or one that cannot be read, is told to press Restore
purchases (`APPLE_SAID`), which sends the phone's purchases to
`/api/licence/apple` for a new one. See `LAUNCH.md` §11 and `lib/apple.js`.

Such a licence is also shorter (below, *Why 30 days*), and the AI relay
reads its row before each request (*Refunds, cancellations and
revocation*).

The phone sends `/api/licence/apple` the transactions StoreKit holds and,
beside them, each subscription's signed renewal info (`renewalInfos`, the
jwsRepresentation of StoreKit 2's `Product.SubscriptionInfo.RenewalInfo`),
which says when a billing grace period ends. A renewal info is used only
with a transaction of the same request that has the same original
transaction id and was verified in the same environment; the grace period
it states is recorded under the same guard as a notification's, stamped
with when Apple signed it. So a phone that sends it keeps a customer in
Apple's billing grace period licensed by itself, even when no notification
reaches mailrata.org. The StoreKit code (`claude/iap-native`) sends only
`transactions` so far: until a build sends `renewalInfos` too, a grace
period reaches the row only through Apple's notifications (`LAUNCH.md`
§11, steps 4 and 5).

**Before the Mac App Store build ships:** the App Store app's bundle id,
`org.mailrata.mail`, is shared by the iPhone and Mac apps (Universal
Purchase), so this path already takes a purchase made on a Mac. The words do
not yet: the privacy policy, the terms, the help page, `LAUNCH.md` §11 and
`APPLE_SAID` say "the iPhone app" throughout, and a test or two pins those
words (`tests/legal.test.mjs`, `tests/help.test.mjs`). Widen them, and the
tests with them, as a step of that release, not before.

The worst somebody with a stolen token can do is obtain a copy of a token they
already have — the endpoint returns a licence for the address inside the licence
presented, and nothing else. Asking a customer to type an email and password
every month to keep reading mail already on their computer would undo the reason
for signing the licence in the first place.

## Why 30 days, and not the subscription's length

A licence that expired with the billing period would lock someone out the
moment their card was retried. Thirty days is the *offline grace period*: how
long RATA keeps working with no contact at all. Long enough for a holiday, a
dead battery and a fortnight of bad wifi; short enough that a cancellation
stops mattering within a billing cycle or two.

`LICENCE_DAYS` in `lib/licence.js` is the one place to change it.

**An App Store licence is shorter.** A licence for an address lasts 30
days from the day it is issued, so one renewed on the last day of a period
runs nearly a whole period past it; and a purchase in Apple's test
environment costs nothing, so a 30-day licence from one would be a month
of RATA for nothing, as often as anyone liked. So a licence for
`apple:<id>` lasts until the end of the subscription's current period as
Apple last reported it when the licence is issued (or of its billing grace
period), plus a margin, and never more than `LICENCE_DAYS`: the margin is
`APPLE_MARGIN_DAYS` (3) for the App Store and `APPLE_SANDBOX_MARGIN_HOURS`
(1) in the test environment (`lib/apple.js`, `appleLicenceUntil`). For a
monthly subscription bought today that is still 30 days; it is shorter
when it is issued later in a period, and much shorter in the test
environment. A subscription that is cancelled, or stops renewing, stops
unlocking RATA within 3 days of its end.

It is not shorter after a refund. Apple reports a refund as a revocation,
not as an earlier end of the period, so a licence issued before it keeps
unlocking mail until its own end: after a refund on the first day of a
monthly period, about 30 days. What a refund stops at once is AI (below,
*Refunds, cancellations and revocation*), and the next renewal, but only
once Apple's REFUND or REVOKE notification for the period the row records
has reached mailrata.org. Nothing else records a refund: StoreKit stops
listing a refunded purchase, and `/api/licence/apple` drops one the phone
sends with a `revocationDate` (`appleIgnored`, `revoked`). Without the
notifications (`LAUNCH.md` §11, step 5) the row stays live to the end of
the refunded period: the licence renews until then, one renewed at that
end lasts the margin past it (3 days; an hour in the test environment),
and the AI relay answers to the same end.

In the test environment, a subscription bought in TestFlight renews once a
day, up to six times a week (App Store Connect Help, *Subscription renewal
rate in TestFlight*); only one bought with a Sandbox Apple Account signed in
on a device has periods minutes long. The one-hour margin is for both.

At the end of a period, nothing shows. The licence then has less than a
week left, so the app renews it at launch, every six hours and when the
connection comes back (bridge.js, `settleLicence`), and on the iPhone also
on each transaction StoreKit hands over, a renewal included
(`claude/iap-native`). Once Apple has renewed the subscription, through its
notification or the phone's own transaction, the next renewal brings a
licence for the new period. A customer offline for more than the margin
across the boundary finds the licence expired. The app then renews it by
itself as soon as it is online: at launch, when the connection comes back,
or when a refresh finds it unlicensed. Until then the licence box says so.
AI keeps answering across the boundary too, to the same margin (below). A
customer whose subscription ended, or was refunded, keeps working until
the licence's own end, and is then told "There is no active RATA
subscription on this Apple account."

## Refunds, cancellations and revocation

There is no revocation list, and adding one would undo the reason for signing:
an app that phones home to ask permission is an app that stops working when the
server does.

What happens instead: the subscription goes to `canceled`, the next renewal
request is refused, and the current licence runs out within thirty days. A
refunded customer keeps working for up to a month. **That is the deliberate
trade** — a month of unpaid use, in exchange for an app that never stops
working because of something at our end.

If that ever becomes too generous, shorten `LICENCE_DAYS` rather than adding a
callback. Fifteen days halves the exposure and still covers a normal holiday.

An App Store subscription refunded or revoked keeps the licence it has to
that licence's end (above, *Why 30 days*): mail keeps working, and the
licence is not renewed. The AI relay, whose answers cost money, does ask:
for an `apple:` licence it reads the subscription's row before anything is
reserved (`app/api/ai`, `appleAiLive`), and refuses at once a row that is
refunded or revoked (which only Apple's notification records, above), and
a licence with no row. A row that has only
expired is still answered until the end a licence issued from it would
have, its period's end (or its billing grace period's) and the margin, so
a paying customer is not refused between a period's end and its renewal
reaching the row (Apple's `DID_RENEW` delivered late, or the phone not
opened yet), and AI never outlasts what the licence itself may. A refusal
is answered as an expired licence is (`reason: 'expired'`): the app renews
once and asks again, the renewal of a refunded row is refused too, and the
customer reads the App Store's sentence beside the summary or translation
made on the device. The relay also validates the request against the lower
of the licence's plan and the row's. When the row's is lower (Pro changed
to Base) and the row is live, it answers `expired`, so the app renews to
Base and asks again. When the row is only inside its margin, renewal is
refused and the app keeps the Pro licence, so `expired` would refuse every
request until the next period reached the row; the relay then answers what
Base allows and refuses the rest (403 `plan`). A row that cannot be read
refuses (503), as the AI cap does.

## What this does *not* protect against

A determined customer can patch the binary. Every offline licence check can be
removed by someone willing to edit the app, and no amount of cryptography
changes that — the code runs on their computer.

What this stops is the easy thing: editing `"plan":"base"` to `"pro"`, or
forging a token for another address. Those fail. Sharing a token does not:
a copy works until it expires and renews like the original while the
subscription is live (security review row 15, left as a business decision).
Nor does setting the computer's clock back: the app checks the expiry
against the system clock (`standing` in `core.rs`) and keeps no record of the latest
time it has seen. Someone prepared to patch
a binary was never going to pay, and defending against them costs real money
and punishes the people who did.

## Testing it

`npm test` covers the parts that matter: a licence verifies offline against the
public key alone; one signed by a different key is refused; a Base licence
edited to say Pro is refused; and an expired licence is reported as **expired**
rather than forged, because telling a paying customer their licence is fake is
a support call you do not recover from.
