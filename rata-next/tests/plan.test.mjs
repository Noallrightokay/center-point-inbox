/* What each plan includes, and what RATA may offer to sell.

   The one rule worth a suite of its own: the domain add-on is priced in
   lib/plan.js but must not be sold (STRIPE_PRICE_DOMAIN stays unset; there is
   no RATA-hosted mail). So no sentence the plan rules produce may quote its
   price, or offer to add one, unless its Stripe price is configured. */
import { makeChecker } from './helpers.mjs';
import {
  PLANS, SELLABLE, DOMAIN_ADDON, NO_DOMAIN_HOSTING,
  money, domainRefusal, domainAddonOnSale,
} from '../lib/plan.js';

export default async function run(state) {
  const check = makeChecker(state);
  const offers = s => /\$|add (one|another)/i.test(s || '');

  console.log('\n— prices and what is on sale are as they were —');
  {
    check(money(12.99) === '$12.99' && money(72) === '$72' && money(1.5) === '$1.50',
      `money: ${money(12.99)}, ${money(72)}, ${money(1.5)}`);
    check(JSON.stringify(SELLABLE) === '["base","pro"]', `on sale: ${SELLABLE.join(', ')} (Enterprise is not)`);
    check(PLANS.pro.domains === 0 && DOMAIN_ADDON.price === 1.5, 'Pro includes no domain; the add-on keeps its price');
  }

  console.log('\n— STRIPE_PRICE_DOMAIN unset: the refusal offers nothing —');
  {
    for (const env of [{}, { STRIPE_PRICE_DOMAIN: '' }, { STRIPE_PRICE_DOMAIN: '   ' }]) {
      check(!domainAddonOnSale(env), `not on sale with ${JSON.stringify(env)}`);
    }
    const cases = [[null, 0, 0], ['base', 0, 0], ['pro', 0, 0], ['pro', 1, 1], ['pro', 3, 2]];
    for (const [plan, used, bought] of cases) {
      const why = domainRefusal(plan, used, bought, {});
      check(why === NO_DOMAIN_HOSTING && !offers(why),
        `${plan || 'no plan'}, ${used} in use, ${bought} bought: "${why}"`);
    }
    check(/does not host mail on your own domain/.test(NO_DOMAIN_HOSTING), 'and says plainly that RATA does not host it');

    /* Called the way a route would, with no env: it reads process.env. */
    const was = process.env.STRIPE_PRICE_DOMAIN;
    delete process.env.STRIPE_PRICE_DOMAIN;
    try {
      check(!offers(domainRefusal('pro', 0)), 'with no env argument, an unset process.env offers nothing either');
    } finally { if (was !== undefined) process.env.STRIPE_PRICE_DOMAIN = was; }
  }

  console.log('\n— configured, the add-on is offered with its price —');
  {
    const env = { STRIPE_PRICE_DOMAIN: 'price_dom_000' };
    check(domainAddonOnSale(env), 'on sale once its price is set');
    check(/Add one for \$1\.50 a month/.test(domainRefusal('pro', 0, 0, env)), `Pro: "${domainRefusal('pro', 0, 0, env)}"`);
    check(/Add another for \$1\.50/.test(domainRefusal('pro', 1, 1, env)), 'Pro with one bought is offered another');
    check(/can add one for \$1\.50/.test(domainRefusal(null, 0, 0, env)), 'no plan is pointed at Pro and the add-on');
    check(!/Enterprise/.test(domainRefusal('pro', 0, 0, env)), 'and never at Enterprise, which is not on sale');
  }

  console.log('\n— a domain that is allowed is never refused —');
  {
    check(domainRefusal('pro', 0, 1, {}) === null && domainRefusal('pro', 0, 1, { STRIPE_PRICE_DOMAIN: 'p' }) === null,
      'Pro with one bought and none in use: allowed either way');
    check(domainRefusal('enterprise', 50, 0, {}) === null, 'Enterprise: unlimited');
  }
}
