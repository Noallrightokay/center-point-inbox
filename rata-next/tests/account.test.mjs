/* The licence page.

   This is where somebody who has just paid goes to find the key that makes the
   desktop app run, so the failure modes matter more than the happy path: the
   worst outcome is a customer who has paid, sees nothing, and concludes the
   product is broken. Every state here is therefore checked for saying
   something, and for never implying they have not paid when they have. */
import { chromium } from 'playwright';
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
        check(total >= 45 && total <= 90, `and lasts ${total}s: long enough for the subscription event to follow the checkout, short enough to not look hung`);
      }
      /* It must not wait on answers the webhook will never change. */
      check(/JUST_PAID && d\.reason === 'no-subscription'/.test(page) && /d\.reason === 'pending'/.test(page),
        'and only for the answers a webhook is expected to change: none yet after paying, or a purchase being set up');
      check(!/has not reached us/.test(page), 'and never says the payment has not reached us');
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
      check(/auth\.html\?mode=signup&next=account/.test(page), 'for Create an account too, which used to drop it (BUG-S)');
    } finally { await s.stop(); }
  }

  /* BUG-S, driven in Chromium: from paying to the key, with no dead end.
     The page is the real one, signed in through supabase-js's own stored
     session; /api/licence is answered by the test, in turn, as the webhook
     would leave it. Time is Playwright's clock, so the 60 s wait takes none. */
  console.log('\n— from paying to the key, in a browser (BUG-S) —');
  {
    const BASE = 'https://buy.stripe.com/test_base', PRO = 'https://buy.stripe.com/test_pro';
    const s = await startServer({ env: (url) => ({
      SUPABASE_URL: url, SUPABASE_ANON_KEY: fakeSupabaseKey('anon'),
      STRIPE_BASE: BASE, STRIPE_PRO: PRO,
    }) });
    const browser = await chromium.launch();
    const USER = { id: '6c1f3a0e-0000-4000-8000-00000000abcd', email: 'buyer@example.com', aud: 'authenticated', role: 'authenticated' };
    const signedIn = async (ctx) => ctx.addInitScript((user) => {
      const far = Math.floor(Date.now() / 1000) + 30 * 86400;
      localStorage.setItem('sb-127-auth-token', JSON.stringify({
        access_token: 'a-session', refresh_token: 'r', token_type: 'bearer', expires_in: 30 * 86400, expires_at: far, user,
      }));
    }, USER);
    /* Answers /api/licence from `answers` in turn, the last one for good. */
    const licence = async (page, answers) => {
      let n = 0;
      await page.route('**/api/licence', (route) => {
        const a = answers[Math.min(n++, answers.length - 1)];
        route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(a) });
      });
      return () => n;
    };
    const PENDING = { licensed: false, reason: 'pending', message: 'Your checkout has reached us and your licence is being set up.' };
    const NONE = { licensed: false, reason: 'no-subscription', message: 'No active subscription on this account. Choose a plan to use RATA on this device.' };
    const LIVE = { licensed: true, licence: 'v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJleHAiOjQxMDI0NDQ4MDB9.c2ln', plan: 'pro', planLabel: 'RATA Pro' };
    const buyLinks = (page) => page.evaluate(() => [...document.querySelectorAll('#body a')].map(a => a.href).filter(h => /buy\.stripe\.com/.test(h)));
    const text = (page) => page.evaluate(() => document.getElementById('body').innerText);
    /* Steps the page's own wait (WAIT_STEPS x WAIT_GAP) through on the fake clock. */
    const waitOut = async (page, done) => {
      for (let i = 0; i < 40 && !(await done()); i++) {
        await page.clock.fastForward(2000);
        await page.waitForTimeout(40);
      }
    };
    try {
      /* Signed out, straight from Stripe. */
      {
        const ctx = await browser.newContext();
        const page = await ctx.newPage();
        await page.goto(s.url + '/account?checkout=success');
        await page.waitForSelector('text=Sign in to see your licence');
        const hrefs = await page.evaluate(() => [...document.querySelectorAll('#body a')].map(a => a.getAttribute('href')));
        check(hrefs.length === 2 && hrefs.every(h => /next=account/.test(h) && /checkout=success/.test(h)),
          `signed out after paying: Log in and Create an account both come back here, still just paid: ${hrefs.join(' ')}`);
        check(hrefs.some(h => /mode=signup/.test(h)), 'Create an account is the sign-up form');
        check(/address you paid with/.test(await text(page)), 'and the page says to use the address they paid with');

        await page.goto(s.url + '/account');
        await page.waitForSelector('text=Sign in to see your licence');
        const plain = await page.evaluate(() => [...document.querySelectorAll('#body a')].map(a => a.getAttribute('href')));
        check(plain.every(h => /next=account/.test(h) && !/checkout=/.test(h)),
          `signed out, not from Stripe: both come back here, with no just-paid flag: ${plain.join(' ')}`);

        await page.goto(s.url + '/' + hrefs.find(h => /mode=signup/.test(h)));
        check(await page.isVisible('#paid-note') && /address you paid with/.test(await page.textContent('#paid-note')),
          'the sign-up form, reached that way, says to sign up with the address they paid with');
        check(await page.evaluate(() => nextPage()) === 'account.html?checkout=success',
          'and sends them back to the licence page still just paid, so it waits for the webhook');
        await page.goto(s.url + '/auth.html?mode=signup&next=account');
        check(!(await page.isVisible('#paid-note')) && await page.evaluate(() => nextPage()) === 'account.html',
          'without the flag: no note, and back to the licence page as before');
        await page.goto(s.url + '/auth.html?mode=signup&next=app&checkout=success');
        check(await page.evaluate(() => nextPage()) === 'app.html', 'the flag rides only to the licence page');
        const sub = await page.textContent('#s-sub');
        check(!/connect your email straight after/i.test(sub) && /licence key/.test(sub) && /desktop app/.test(sub),
          `sign-up says what comes next, truly: "${sub}"`);
        await ctx.close();
      }

      /* Signed in, straight from Stripe, the checkout recorded but the plan
         not yet named: "setting up", never a buy button, then the key. */
      {
        const ctx = await browser.newContext();
        await signedIn(ctx);
        const page = await ctx.newPage();
        await page.clock.install();
        const asked = await licence(page, [PENDING, PENDING, LIVE]);
        await page.goto(s.url + '/account?checkout=success');
        await page.waitForSelector('text=Setting up your licence');
        check((await buyLinks(page)).length === 0, 'pending: setting up, with no buy buttons');
        await waitOut(page, async () => /Your licence key/.test(await text(page)));
        check(/Your licence key/.test(await text(page)) && asked() === 3, `and the key appears as soon as the webhook finishes (${asked()} asks)`);
        await ctx.close();
      }

      /* Pending for the whole wait: still no buy button, a sentence saying
         the key appears on reload, and never "has not reached us". */
      for (const [label, url, answers] of [
        ['pending', '/account', [PENDING]],
        ['just paid, no row yet', '/account?checkout=success', [NONE]],
      ]) {
        const ctx = await browser.newContext();
        await signedIn(ctx);
        const page = await ctx.newPage();
        await page.clock.install();
        const asked = await licence(page, answers);
        await page.goto(s.url + url);
        await page.waitForSelector('text=Setting up your licence');
        await waitOut(page, async () => !!(await page.$('#reload')));
        const t = await text(page);
        check(!!(await page.$('#reload')) && asked() >= 30, `${label}: the page waits about a minute (${asked()} asks) before settling`);
        check(/reload this page/i.test(t) && (await buyLinks(page)).length === 0,
          `${label}: then says the key appears on reload, with no buy buttons: "${t.split('\n').slice(0, 3).join(' / ')}"`);
        check(!/has not reached us/i.test(t), `${label}: and never says the payment has not reached us`);
        if (label !== 'pending') check(/address you paid with/.test(t), `${label}: and points at the address they paid with`);
        await ctx.close();
      }

      /* No plan, not from Stripe: the buy buttons, carrying the account. */
      {
        const ctx = await browser.newContext();
        await signedIn(ctx);
        const page = await ctx.newPage();
        await licence(page, [NONE]);
        await page.goto(s.url + '/account');
        await page.waitForSelector('text=Choose a plan to get your key');
        const links = (await buyLinks(page)).map(h => new URL(h));
        check(links.length === 2 && links.every(u => u.searchParams.get('prefilled_email') === USER.email
          && u.searchParams.get('client_reference_id') === USER.id),
          `the buy buttons carry prefilled_email and client_reference_id: ${links.map(String).join(' ')}`);
        check(/anyone with this key can run RATA as you while your subscription is live/.test(await page.content()),
          'the key note says what a leaked key can do');
        check(!/change your plan or ask us to reissue it/.test(await page.content()),
          'and no longer claims a plan change or reissue stops it');
        await ctx.close();
      }

      /* The home page's buy links, for a signed-in person and for anyone. */
      {
        const ctx = await browser.newContext();
        const page = await ctx.newPage();
        await page.goto(s.url + '/');
        check(await page.getAttribute('#buy-base', 'href') === BASE && await page.getAttribute('#buy-pro', 'href') === PRO,
          'signed out, the home page buy links are the plain Payment Links');
        await page.evaluate((u) => localStorage.setItem('centra_session', JSON.stringify({ uid: u.id, email: u.email, mode: 'supabase' })), USER);
        await page.goto(s.url + '/');
        for (const id of ['buy-base', 'buy-pro']) {
          const u = new URL(await page.getAttribute('#' + id, 'href'));
          check(u.searchParams.get('prefilled_email') === USER.email && u.searchParams.get('client_reference_id') === USER.id,
            `signed in, #${id} carries both: ${u}`);
        }
        /* One rule: index.html's and account.html's buyLink are the same
           text, and behave the same on the awkward cases. */
        const home = await (await fetch(s.url + '/')).text();
        const acct = await (await fetch(s.url + '/account')).text();
        const fn = (html) => (html.match(/function buyLink\(u, email, uid\) \{[\s\S]*?\n\}/) || [''])[0];
        check(fn(home).length > 100 && fn(home) === fn(acct), 'index.html and account.html share one buyLink');
        const cases = await page.evaluate(() => [
          buyLink('https://buy.stripe.com/x?a=1', 'a+b@example.com', 'u_1'),
          buyLink('https://mailrata.org/#pricing', 'a@example.com', 'u_1'),
          buyLink('/#pricing', 'a@example.com', 'u_1'),
          buyLink('https://buy.stripe.com/x', 'a@example.com', 'has a space'),
          buyLink('', 'a@example.com', 'u_1'),
        ]);
        check(cases[0] === 'https://buy.stripe.com/x?a=1&prefilled_email=a%2Bb%40example.com&client_reference_id=u_1',
          `keeps the link's own query and encodes the address: ${cases[0]}`);
        check(cases[1] === 'https://mailrata.org/#pricing' && cases[2] === '/#pricing',
          'leaves our own pricing section alone (no address in our URLs)');
        check(cases[3] === 'https://buy.stripe.com/x?prefilled_email=a%40example.com', 'drops an id Stripe would refuse');
        check(cases[4] === '', 'and leaves no link as no link');
        await ctx.close();
      }
    } finally { await browser.close(); await s.stop(); }
  }
}
