/* Deleting an account.

   The parts that can be checked without a database are the ones that decide
   whether deletion happens at all: the confirmation, and the refusal to delete
   while Stripe is still charging. The deletes themselves are four statements
   against tables that cascade anyway; what is worth testing is that nothing
   gets past the gate. */
import { startServer, makeChecker } from './helpers.mjs';
import { blocksDeletion, DELETION_REMOVES, DELETION_KEEPS } from '../lib/account.js';

export default async function run(state) {
  const check = makeChecker(state);

  console.log('\n— a live subscription stops the deletion —');
  {
    check(blocksDeletion(null) === null, 'no subscription: nothing in the way');
    check(blocksDeletion({ status: 'canceled', plan: 'pro' }) === null, 'a cancelled one: nothing in the way');
    check(blocksDeletion({ status: 'incomplete_expired' }) === null, 'an expired attempt: nothing in the way');

    for (const status of ['active', 'trialing', 'past_due']) {
      const why = blocksDeletion({ status, plan: 'pro' });
      check(!!why && /billing portal/i.test(why),
        `${status}: refused, and points at the portal`);
    }

    /* The reason this refuses rather than deleting and warning: RATA sells
       through Payment Links and holds no Stripe secret key, so it cannot
       cancel on somebody's behalf. Deleting anyway would leave them paying for
       an account that no longer exists. */
    const why = blocksDeletion({ status: 'active', plan: 'pro' });
    check(/keeps charging/i.test(why), `and says what would otherwise happen: "${why}"`);
  }

  console.log('\n— the warning is generated, not written twice —');
  {
    check(DELETION_REMOVES.some(r => /login/i.test(r)) && DELETION_REMOVES.some(r => /subscription/i.test(r)),
      `removes: ${DELETION_REMOVES.length} things, including the subscription and the login`);
    /* And no longer claims to remove a mailbox password, because there is not
       one here to remove — that is the whole point of the move to the device,
       and a deletion notice that overstates itself is worse than none. */
    check(!DELETION_REMOVES.some(r => /mailbox|password|credential/i.test(r)),
      'and claims nothing about mailbox passwords, which RATA no longer holds');
    check(DELETION_KEEPS.some(k => /keychain/i.test(k)),
      'while saying where they actually are');
    check(DELETION_KEEPS.some(k => /your provider/i.test(k)),
      'and is honest that the mail is not RATA\'s to delete');
    check(DELETION_KEEPS.some(k => /Stripe/.test(k)),
      "nor Stripe's billing records");
  }

  console.log('\n— the endpoint itself —');
  {
    const s = await startServer();
    try {
      const anon = await fetch(s.url + '/api/account', { method: 'DELETE' });
      const body = await anon.json();
      check(!!body.error, `no session, no deletion: ${JSON.stringify(body.error)}`);

      const look = await (await fetch(s.url + '/api/account')).json();
      check(!!look.error, 'and the summary of what would go is behind the same check');

      /* Every other verb is absent by design: there is no way to delete an
         account with a link somebody could be tricked into clicking. */
      const get = await fetch(s.url + '/api/account', { method: 'POST' });
      check(get.status === 405, `POST to the account endpoint: ${get.status}`);
    } finally { await s.stop(); }
  }
}
