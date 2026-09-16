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

Once, on your own machine. Never in a browser, never through chat.

```bash
node -e "const{generateKeys}=require('./rata-next/lib/licence.js');const k=generateKeys();
console.log('--- PRIVATE (server only) ---\n'+k.privateKey);
console.log('--- PUBLIC (ships in the app) ---\n'+k.publicKey)"
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

## 3. The flow

1. Customer buys through the Stripe payment link. The webhook writes
   `subscriptions` as it does today — nothing changes there.
2. They sign in on the website and download the app.
3. The app calls `GET /api/licence` with their session. The server looks up the
   subscription for that address and signs a licence for it.
4. The app stores the token and checks it locally from then on.
5. Whenever it is next online it asks again, and gets a fresh 30 days.

The licence is issued against the **signed-in account**, never an address in
the request body — the subscription is keyed on email, so accepting a
caller-supplied one would hand a licence to anyone who could guess a customer's
address.

## Why 30 days, and not the subscription's length

A licence that expired with the billing period would lock someone out the
moment their card was retried. Thirty days is the *offline grace period*: how
long RATA keeps working with no contact at all. Long enough for a holiday, a
dead battery and a fortnight of bad wifi; short enough that a cancellation
stops mattering within a billing cycle or two.

`LICENCE_DAYS` in `lib/licence.js` is the one place to change it.

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

## What this does *not* protect against

A determined customer can patch the binary. Every offline licence check can be
removed by someone willing to edit the app, and no amount of cryptography
changes that — the code runs on their computer.

What this stops is the easy thing: sharing a token, editing `"plan":"base"` to
`"pro"`, or setting the clock back. Those all fail. Someone prepared to patch
a binary was never going to pay, and defending against them costs real money
and punishes the people who did.

## Testing it

`npm test` covers the parts that matter: a licence verifies offline against the
public key alone; one signed by a different key is refused; a Base licence
edited to say Pro is refused; and an expired licence is reported as **expired**
rather than forged, because telling a paying customer their licence is fake is
a support call you do not recover from.
