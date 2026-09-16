import { NextResponse } from 'next/server';
import { timingSafeEqual } from 'node:crypto';
import { admin } from '../../../lib/server';
import { issue } from '../../../lib/licence';

export const dynamic = 'force-dynamic';

/* ---------------------------------------------------------------------------
   Something worth pointing a monitor at.

   An uptime check on `/` answers 200 for as long as the web server is alive,
   which is the least interesting thing that can be true. It says nothing about
   the two failures that actually take RATA down without taking the site down:
   Supabase unreachable, so nobody can sign in or be billed; and
   LICENCE_PRIVATE_KEY missing, so nobody can get a licence and every copy of
   the app stops working within thirty days — the product's whole purpose,
   broken, behind a page that loads perfectly.

   The second one is the reason this exists. It used to be TOKEN_ENC_KEY, for
   exactly the same reason: it is not a crash, it is a variable, and a variable
   can go missing on any redeploy without a single error anywhere. The mailbox
   credentials it protected now live on the customer's own machine, so the
   variable that can silently end the product is the signing key instead.

   Two audiences, so two levels of detail. Anyone may ask whether RATA is up —
   that is what a monitor needs and it gives nothing away. Which piece is
   broken is only told to a caller holding HEALTH_TOKEN, because "your
   encryption key is unset" is a useful thing for a stranger to learn.
   --------------------------------------------------------------------------- */

/* A health check that hangs is worse than one that fails: the monitor waits,
   the page never comes, and the alert arrives as a timeout that says nothing
   about what is wrong. */
const DEADLINE_MS = 4000;

function withDeadline(p, ms, onLate) {
  let t;
  return Promise.race([
    p.finally(() => clearTimeout(t)),
    new Promise(res => { t = setTimeout(() => res(onLate()), ms); }),
  ]);
}

function authorised(req) {
  const want = process.env.HEALTH_TOKEN;
  if (!want) return false;
  const url = new URL(req.url);
  const auth = req.headers.get('authorization') || '';
  const given = auth.startsWith('Bearer ') ? auth.slice(7) : (url.searchParams.get('token') || '');
  const a = Buffer.from(given, 'utf8'), b = Buffer.from(want, 'utf8');
  return a.length === b.length && timingSafeEqual(a, b);
}

async function checkDatabase() {
  const sb = admin();
  if (!sb) return { ok: false, detail: 'SUPABASE_URL or SUPABASE_SERVICE_ROLE_KEY is not set' };
  return withDeadline(
    (async () => {
      /* The cheapest real round trip: one row that may not exist. Anything
         that comes back without an error proves the database answered. */
      const { error } = await sb.from('subscriptions').select('email').limit(1);
      return error ? { ok: false, detail: error.message } : { ok: true };
    })(),
    DEADLINE_MS,
    () => ({ ok: false, detail: `no answer within ${DEADLINE_MS}ms` }),
  );
}

export async function GET(req) {
  const started = Date.now();

  const database = await checkDatabase();
  /* Issuing a licence is tried rather than assumed: a key that is present but
     malformed reads as broken here, rather than as fine until the first
     customer who has paid cannot get a licence. Nothing is stored — the token
     is signed and thrown away. */
  let licensing;
  try {
    issue({ email: 'health@rata.invalid', plan: 'base' });
    licensing = { ok: true };
  } catch (e) {
    licensing = {
      ok: false,
      detail: /LICENCE_PRIVATE_KEY/.test(e.message)
        ? 'LICENCE_PRIVATE_KEY is not set — no licence can be issued, so the app stops working for everyone within thirty days'
        : e.message,
    };
  }

  const billing = process.env.STRIPE_WEBHOOK_SECRET
    ? { ok: true }
    : { ok: false, detail: 'STRIPE_WEBHOOK_SECRET is not set — payments will not be recorded' };

  const checks = { database, licensing, billing };

  /* `up` and `ready` are different questions and a monitor wants both. The app
     is up when the database answers; it is ready when everything the product
     needs to actually work is configured. Only `up` decides the status code,
     so a deployment that is deliberately unconfigured does not page anyone at
     three in the morning — it simply reports ready:false. */
  const up = database.ok;
  const ready = Object.values(checks).every(c => c.ok);

  const body = { ok: up, ready, service: 'rata', time: new Date().toISOString() };

  if (authorised(req)) {
    body.checks = checks;
    body.tookMs = Date.now() - started;
  } else {
    /* Names only, never reasons. Enough for an operator to see at a glance
       which one to go and look at with the token. */
    body.failing = Object.entries(checks).filter(([, c]) => !c.ok).map(([k]) => k);
    if (!body.failing.length) delete body.failing;
  }

  return NextResponse.json(body, {
    status: up ? 200 : 503,
    headers: { 'Cache-Control': 'no-store, must-revalidate' },
  });
}
