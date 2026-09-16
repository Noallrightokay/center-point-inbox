import { LIVE_STATUSES } from './stripe.js';

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
   allowed. */
export function blocksDeletion(sub) {
  if (!sub) return null;
  const status = String(sub.status || '').toLowerCase();
  if (!LIVE_STATUSES.includes(status)) return null;
  return `Your${sub.plan ? ' ' + sub.plan : ''} subscription is still ${status}. `
    + 'Cancel it in the billing portal first — otherwise Stripe keeps charging for an account that no longer exists. '
    + 'Then come back and delete.';
}

/* What deletion takes and what it cannot reach. Written once, so the warning
   shown in the app is generated from the same place as the behaviour. */
export const DELETION_REMOVES = [
  'the encrypted password for every mailbox you connected',
  'your preferences and folder names',
  'your subscription record',
  'your login',
];

export const DELETION_KEEPS = [
  'your mail, which stays at your provider',
  "Stripe's billing records, which Stripe must keep",
  'anything already on this device, until you clear it here',
];
