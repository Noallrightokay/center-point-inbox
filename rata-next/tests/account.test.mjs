/* The licence page.

   This is where somebody who has just paid goes to find the key that makes the
   desktop app run, so the failure modes matter more than the happy path: the
   worst outcome is a customer who has paid, sees nothing, and concludes the
   product is broken. Every state here is therefore checked for saying
   something, and for never implying they have not paid when they have. */
import { startServer, makeChecker, fakeSupabaseKey } from './helpers.mjs';

export default async function run(state) {
  const check = makeChecker(state);

  console.log('\n— the page is on a clean URL, like every other page —');
  {
    const s = await startServer();
    try {
      const r = await fetch(s.url + '/account');
      const html = await r.text();
      check(r.status === 200, `/account -> ${r.status}`);
      check(/Your licence/.test(html), 'and is the licence page');
      /* It must not leak into the app shell or the signup form. */
      check(!/Create your account/.test(html), 'not the signup page by accident');
    } finally { await s.stop(); }
  }

  console.log('\n— with no database configured there are no licences, and it says so —');
  {
    const s = await startServer();
    try {
      const page = await (await fetch(s.url + '/account')).text();
      /* The text is rendered by script, so what is checked here is that the
         branch exists and names the honest reason rather than a sign-in form
         that could not work. */
      check(/no accounts configured/.test(page), 'explains that the deployment has no accounts');
      check(/RATA still runs on this device/.test(page), 'and that the app works anyway');
    } finally { await s.stop(); }
  }

  console.log('\n— a customer who has paid is never told they have not —');
  {
    const s = await startServer();
    try {
      const page = await (await fetch(s.url + '/account')).text();
      check(/This is at our end, not yours/.test(page),
        'a server-side failure says whose fault it is');
      check(/your subscription is unaffected/i.test(page),
        'and that their subscription is fine');
      /* The only place "no active subscription" may appear is the branch that
         actually means it. */
      check(/no-subscription/.test(page), 'the unsubscribed branch is keyed on the reason, not on any error');

      /* The server's own words are for support, never for the customer.
         "LICENCE_PRIVATE_KEY is not set" is true, useless to whoever is
         reading it, and reads as an accusation to somebody who has just paid. */
      check(/never in the sentence they read/.test(page),
        'a server fault shows our words separately from theirs');
      check(/this is the part that helps us find it/i.test(page),
        'and labels the technical detail as something to quote to support');
    } finally { await s.stop(); }
  }

  /* The bug this suite was written around. The buttons read stripeMonthly and
     stripeAnnual — names from a two-plan price list that no longer exists — so
     /api/config supplied nothing, both fell through to their signup href, and
     nobody could reach checkout. A button that goes somewhere plausible looks
     like a working button, which is why this failed silently. */
  console.log('\n— the buy buttons reach Stripe —');
  {
    const s = await startServer({ env: {
      SUPABASE_URL: 'https://demo.supabase.co',
      SUPABASE_ANON_KEY: fakeSupabaseKey('anon'),
      STRIPE_BASE: 'https://buy.stripe.com/test_base',
      STRIPE_PRO: 'https://buy.stripe.com/test_pro',
    }});
    try {
      const cfgBody = await (await fetch(s.url + '/config.js')).text();
      const scope = {};
      new Function('window', cfgBody)(scope);
      const cfg = scope.RATA_CONFIG;

      const home = await (await fetch(s.url + '/')).text();
      /* The ids the script writes to must be the ids on the page. */
      for (const id of ['buy-base', 'buy-pro']) {
        check(home.includes(`id="${id}"`), `${id} exists on the page`);
        check(new RegExp(`getElementById\\('${id}'\\)`).test(home), `and the script sets ${id}'s href`);
      }
      /* And the keys it reads must be the keys the server emits. */
      const reads = [...home.matchAll(/RC\.(stripe[A-Za-z]+)/g)].map(m => m[1]);
      check(reads.length > 0, `the page reads ${reads.join(', ')}`);
      for (const key of reads) {
        check(key in cfg, `/api/config emits ${key} — otherwise the button silently goes nowhere`);
        check(cfg[key] !== '', `and it has a value: ${cfg[key]}`);
      }
      /* Aimed at what is read rather than what is written: the comment
         recording this bug names the old keys, and a test that cannot tell a
         comment from code is a test that punishes explaining yourself. */
      check(!/RC\.stripeMonthly|RC\.stripeAnnual/.test(home),
        'nothing still reads the names from the old price list');
      check(!/getElementById\('buy-(monthly|annual)'\)/.test(home),
        'and no script writes to buttons that no longer exist');
    } finally { await s.stop(); }
  }

  /* The worst sentence this page could show is "no active plan", and the most
     likely moment for it is the one time we know it is wrong: the redirect
     back from Stripe, which routinely beats the webhook by a second or two. */
  console.log('\n— straight back from checkout, "no plan yet" is not "no plan" —');
  {
    const s = await startServer();
    try {
      const page = await (await fetch(s.url + '/account')).text();
      check(/checkout=success/.test(page), 'the page knows when somebody has just paid');
      check(/Setting up your licence/.test(page), 'and waits, rather than telling them they have not paid');
      check(/your subscription is already active/.test(page), 'saying the money is safe while it waits');
      /* Bounded: a wait with no end is a page that never says anything. */
      const steps = page.match(/WAIT_STEPS\s*=\s*(\d+)/);
      const gap = page.match(/WAIT_GAP\s*=\s*(\d+)/);
      check(!!steps && !!gap, 'the wait is bounded');
      if (steps && gap) {
        const total = Number(steps[1]) * Number(gap[1]) / 1000;
        check(total >= 10 && total <= 60, `and lasts ${total}s — long enough for a webhook, short enough to not look hung`);
      }
      /* It must not wait on answers the webhook will never change. */
      check(/JUST_PAID && d\.reason === 'no-subscription'/.test(page),
        'and only for the one answer a webhook is expected to change');
    } finally { await s.stop(); }
  }

  console.log('\n— signing in can be sent back to the licence page, but only there —');
  {
    const s = await startServer();
    try {
      const auth = await (await fetch(s.url + '/auth')).text();
      check(/NEXT=\{account:'account\.html',app:'app\.html'\}/.test(auth.replace(/\s+/g, '')) ||
            /account:'account\.html'/.test(auth), 'the destinations are a fixed list');
      /* The important half: the value is looked up, never followed. An open
         redirect on the page that handles passwords is the worst place for one. */
      check(!/location\.replace\(\s*new URLSearchParams/.test(auth), 'the query string is never used as a destination');
      check(/NEXT\[new URLSearchParams/.test(auth), 'it is matched against the list instead');

      const page = await (await fetch(s.url + '/account')).text();
      check(/auth\.html\?mode=login&next=account/.test(page), 'and the licence page asks for exactly that');
    } finally { await s.stop(); }
  }
}
