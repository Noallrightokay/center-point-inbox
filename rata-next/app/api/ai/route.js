import { NextResponse } from 'next/server';
import { admin } from '../../../lib/server';
import { check } from '../../../lib/licence';
import { planDef } from '../../../lib/plan';
import { corsHeaders, preflight } from '../../../lib/cors';
import { API_URL, LIMITS, MODEL, MONTHLY_CAP_MICRO, costMicro, monthOf, parseTasks, prompt, tooFast, validate, worstCaseMicro } from '../../../lib/ai';

export const dynamic = 'force-dynamic';

/* Summaries, translation and task flags for the desktop app. See lib/ai.js
   for the rules; in short: the licence is the credential, the plan decides
   what is allowed, every request holds its worst-case cost against the
   customer's monthly allowance before it is sent and is settled to the real
   cost after, and no text is ever written anywhere. */
/* PostgREST's "no such function" (PGRST202) and Postgres's own (42883): a
   deploy whose database has not had section 5 of database.sql run since the
   reservation functions were added. */
const missingFunction = (e) => e && (e.code === 'PGRST202' || e.code === '42883');

export function OPTIONS(req) {
  return preflight(req);
}

export async function POST(req) {
  const cors = corsHeaders(req);
  const reply = (body, status = 200) => NextResponse.json(body, { status, headers: cors });

  const raw = await req.text();
  if (raw.length > LIMITS.body) return reply({ error: 'That is more text than RATA sends at once.' }, 413);
  let body;
  try { body = JSON.parse(raw); } catch { return reply({ error: 'Bad request' }, 400); }

  /* The licence is signed by this server, so presenting a current one proves
     who is asking and what they bought. An expired one is refused here — the
     app renews it on its own when it is online. */
  const seen = check(String(body.licence || ''), process.env.LICENCE_PUBLIC_KEY);
  if (!seen.ok) {
    return reply({
      error: seen.reason === 'expired' ? 'Your licence needs renewing — RATA does this itself when it is online.' : 'This copy of RATA is not licensed.',
      reason: seen.reason,
    }, seen.reason === 'no-public-key' ? 503 : 401);
  }
  const who = String(seen.licence.sub).toLowerCase();

  const asked = validate(body, planDef(seen.licence.plan));
  if (asked.error) return reply({ error: asked.error, reason: asked.reason }, asked.status);

  const key = process.env.ANTHROPIC_API_KEY;
  const sb = admin();
  if (!key || !sb) return reply({ error: 'AI is not switched on for RATA yet.', reason: 'not-configured' }, 503);

  if (tooFast(who)) return reply({ error: 'Too many AI requests at once — try again in a minute.', reason: 'rate' }, 429);

  /* The month's allowance. The most this request could cost is held against
     it before anything is sent, in one statement that refuses when the hold
     would pass the cap — so requests arriving together cannot all fit under
     it on the same reading. No answer from the database means no request to
     the model: the cap fails closed. */
  const p = prompt(asked.task, asked.input);
  const month = monthOf();
  const cap = MONTHLY_CAP_MICRO();
  const hold = worstCaseMicro(p);
  const call = async (fn, args) => {
    try { return await sb.rpc(fn, args); } catch (e) { return { data: null, error: { message: (e && e.name) || 'unreachable' } }; }
  };
  const { data: held, error: holdErr } = await call('ai_reserve', { p_email: who, p_month: month, p_micro: hold, p_cap: cap });
  if (holdErr) {
    if (missingFunction(holdErr)) {
      console.warn('ai: the database has no ai_reserve function; run section 5 of rata-next/database.sql');
      return reply({ error: 'AI is not switched on for RATA yet.', reason: 'not-configured' }, 503);
    }
    console.warn('ai: could not reserve spending', holdErr.code || holdErr.message);
    return reply({ error: 'AI is unavailable right now.' }, 503);
  }
  if (held === null || held === undefined) {
    return reply({ error: 'This month’s AI allowance is used up. It starts again on the 1st.', reason: 'allowance', used: 1 }, 429);
  }

  /* Moves the hold to what was actually spent: back down by the difference,
     or all of it when the model never answered. If this fails the larger
     hold stays: the month is over-counted by at most this request's worst
     case, and the cap is never passed. */
  const settle = async (delta) => {
    if (!delta) return Number(held);
    const { data, error } = await call('ai_settle', { p_email: who, p_month: month, p_delta: delta });
    if (error) { console.warn('ai: could not settle spending', error.code || error.message); return null; }
    return data;
  };

  let r, d;
  try {
    r = await fetch(API_URL(), {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'x-api-key': key, 'anthropic-version': '2023-06-01' },
      body: JSON.stringify({ model: MODEL(), max_tokens: p.max_tokens, system: p.system, messages: [{ role: 'user', content: p.user }] }),
      signal: AbortSignal.timeout(45_000),
    });
    d = await r.json();
  } catch (e) {
    /* The reason, never the text. */
    console.warn('ai: upstream unreachable', e && e.name);
    await settle(-hold);
    return reply({ error: 'The AI service could not be reached. Try again shortly.' }, 502);
  }
  if (!r.ok) {
    console.warn('ai: upstream refused', r.status, d && d.error && d.error.type);
    await settle(-hold);
    return reply({ error: 'The AI service could not answer. Try again shortly.' }, 502);
  }

  const total = await settle(costMicro(d.usage) - hold);
  const used = Math.min(1, Number(total ?? held) / cap);

  const text = (Array.isArray(d.content) ? d.content : []).filter(c => c && c.type === 'text').map(c => c.text).join('').trim();
  if (asked.task === 'tasks') {
    return reply({ tasks: parseTasks(text, asked.input.messages.map(m => m.id)), used });
  }
  return reply({ text, cut: !!asked.input.cut, used });
}
