/* ---------------------------------------------------------------------------
   Limiting how often a sign-in can be attempted.

   /api/link/mail takes an email address and a password and tries them against
   a real mail server, then says which way it went. That is exactly the shape
   of a credential-testing oracle: a signed-up account could feed it a list of
   leaked address/password pairs and read off, from RATA's own replies, which
   ones still work — with the attempts arriving at Gmail and iCloud from RATA's
   address rather than the attacker's.

   It is also how a legitimate user gets locked out. Providers count failed
   sign-ins per account, so a mistyped app password retried in a loop does real
   damage to the person doing the retrying.

   Both problems are the same problem: failed attempts must get slower.
   Successes are not counted at all, so somebody linking six mailboxes in a row
   never meets this.

   Two keys, because they answer different questions:
     per account   — how much guessing one RATA user may do
     per address   — how much guessing one mailbox may receive, no matter how
                     many RATA accounts the guessing is spread across

   This counts in memory, in one process. RATA runs as a single Node app, so
   that is the whole picture today; behind several instances the effective
   limit multiplies by the instance count, which still bounds the attack but
   less tightly. Moving the counter to Postgres is the change to make then —
   the call sites would not move.
   --------------------------------------------------------------------------- */

const windows = new Map();

/* Swept on write rather than on a timer: a timer would hold the process awake,
   and this map is only ever touched by the requests that populate it. */
function sweep(now) {
  if (windows.size < 500) return;
  for (const [k, w] of windows) if (w.until <= now) windows.delete(k);
}

/* Is this key allowed another attempt? Read-only — nothing is counted here, so
   a check that passes and then succeeds leaves no trace. */
export function allowed(key, { limit, windowMs }, now = Date.now()) {
  const w = windows.get(key);
  if (!w || w.until <= now) return { ok: true, remaining: limit };
  if (w.count < limit) return { ok: true, remaining: limit - w.count };
  return { ok: false, remaining: 0, retryAfterMs: w.until - now };
}

/* Count one failure. Returns the state after counting. */
export function failed(key, { limit, windowMs }, now = Date.now()) {
  sweep(now);
  let w = windows.get(key);
  if (!w || w.until <= now) {
    /* A fixed window, deliberately: the window starts at the first failure and
       does not slide forward as more arrive, so a user who mistypes once is
       not held for the full period after every subsequent try. */
    w = { count: 0, until: now + windowMs };
    windows.set(key, w);
  }
  w.count++;
  return allowed(key, { limit, windowMs }, now);
}

/* A success clears the record: getting it right is proof the attempts were
   somebody working out their own password, not working through a list. */
export function succeeded(key) {
  windows.delete(key);
}

/* For tests, and for a deploy that wants to start clean. */
export function reset() {
  windows.clear();
}

/* The limits themselves, named so the routes read as policy rather than
   arithmetic. Five wrong app passwords in a quarter of an hour is far past
   what typing one out of a provider's screen takes. */
export const LINK_ATTEMPTS = { limit: 5, windowMs: 15 * 60 * 1000 };
export const ADDRESS_ATTEMPTS = { limit: 3, windowMs: 15 * 60 * 1000 };

/* "Try again in four minutes" — a number somebody can act on, rather than a
   timestamp or a bare refusal. */
export function waitPhrase(ms) {
  const mins = Math.ceil((ms || 0) / 60000);
  if (mins <= 1) return 'in a minute';
  return `in ${mins} minutes`;
}
