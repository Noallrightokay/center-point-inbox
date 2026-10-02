import { LIVE_STATUSES, PENDING } from './stripe.js';
import { LICENCE_DAYS } from './licence.js';

/* ---------------------------------------------------------------------------
   Closing an account.

   The rule that decides whether deletion may proceed lives here rather than in
   the route, so it can be reasoned about and tested without a Next runtime or
   a database — it is the part with a real decision in it. The route does the
   four deletes.
   --------------------------------------------------------------------------- */

/* Returns null when deletion may go ahead, or the reason it may not.

   RATA sells through Stripe Payment Links and holds no Stripe secret key, so
   it cannot cancel a subscription on somebody's behalf. Deleting the account
   anyway would leave them paying for something that no longer exists, so this
   refuses and sends them to the portal — one click, and then the deletion is
   allowed.

   Two more refusals, from the security review's part 3:

   A subscription not confirmed yet (PENDING: a bank transfer still
   clearing, or a checkout whose subscription event has not landed). Deleted
   now, its confirmation would write the row back as live (checkout events
   are keyed by address) and Stripe would bill an address with no login.

   A subscription that ended less than LICENCE_DAYS ago (P3-6). A licence is
   issued only while the subscription is live and works for LICENCE_DAYS, and
   it is the AI relay's only credential. Deleting the account deletes the
   month's AI usage, so deleting straight after cancelling let that licence
   spend the month's allowance a second time. Until the last licence the
   subscription could have issued has run out, the account stays, with the
   date it can go. The row's later time (when Stripe said it ended, or when
   that was written) bounds when the last licence was issued; a status that
   was never live (incomplete_expired) issued none. Chosen over keeping this
   month's usage row after deletion, which would keep an address RATA was
   asked to forget, with nothing to remove it later. `now` is for tests. */
export function blocksDeletion(sub, now = Date.now()) {
  if (!sub) return null;
  const status = String(sub.status || '').toLowerCase();
  if (LIVE_STATUSES.includes(status)) {
    return `Your${sub.plan ? ' ' + sub.plan : ''} subscription is still ${status}. `
      + 'Cancel it in the billing portal first, or Stripe keeps charging for an account that no longer exists. '
      + 'Then come back and delete.';
  }
  if (status === PENDING) {
    return 'Your subscription is waiting for Stripe to confirm it, as it does while a bank transfer clears. '
      + 'Deleted now, it would start once confirmed for an account that no longer exists, and Stripe keeps charging for it. '
      + 'Once it is confirmed, cancel it in the billing portal and then delete; if the payment fails, you can delete then.';
  }
  if (status === 'incomplete_expired') return null;
  const ended = Math.max(...[sub.event_at, sub.updated_at].map(t => (t ? Date.parse(t) : NaN)).filter(Number.isFinite));
  if (!Number.isFinite(ended)) return null;
  const until = ended + LICENCE_DAYS * 86400000;
  if (now >= until) return null;
  const day = new Date(until).toISOString().slice(0, 10);
  return `Your subscription has ended, but a licence RATA issued while it was live can keep working until ${day} (UTC). `
    + `The account can be deleted from then. Deleting it sooner would reset what that licence has used of this month's AI allowance.`;
}

/* What deletion takes and what it cannot reach. Written once, so the warning
   shown in the app is generated from the same place as the behaviour. */
export const DELETION_REMOVES = [
  'your preferences and folder names',
  'your subscription record',
  'your AI usage record (only what your AI requests cost each month)',
  'your login',
];

export const DELETION_KEEPS = [
  'your mail, which stays at your provider',
  'your mailbox passwords, which are in your own computer\u2019s keychain and never reached RATA',
  "Stripe's billing records, which Stripe must keep",
  'anything already on this device, until you clear it here',
];
