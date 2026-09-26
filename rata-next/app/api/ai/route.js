import { NextResponse } from 'next/server';
import { admin } from '../../../lib/server';
import { check } from '../../../lib/licence';
import { planDef } from '../../../lib/plan';
import { corsHeaders, preflight } from '../../../lib/cors';
import { API_URL, LIMITS, MODEL, MONTHLY_CAP_MICRO, costMicro, monthOf, parseTasks, prompt, tooFast, validate } from '../../../lib/ai';

export const dynamic = 'force-dynamic';

/* Summaries, translation and task flags for the desktop app. See lib/ai.js
   for the rules; in short: the licence is the credential, the plan decides
   what is allowed, every request is charged against the customer's monthly
   allowance, and no text is ever written anywhere. */
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

  /* The month's spending so far. Refused before anything is sent when it is
     used up; a request already running when the cap is reached is still
     charged, which is the most it can go over by. */
  const month = monthOf();
  const cap = MONTHLY_CAP_MICRO();
  const { data: spent, error: readErr } = await sb.rpc('ai_spent', { p_email: who, p_month: month });
  if (readErr) { console.warn('ai: could not read spending', readErr.message); return reply({ error: 'AI is unavailable right now.' }, 503); }
  if (Number(spent) >= cap) {
    return reply({ error: 'This month’s AI allowance is used up. It starts again on the 1st.', reason: 'allowance', used: 1 }, 429);
  }

  const p = prompt(asked.task, asked.input);
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
    return reply({ error: 'The AI service could not be reached. Try again shortly.' }, 502);
  }
  if (!r.ok) {
    console.warn('ai: upstream refused', r.status, d && d.error && d.error.type);
    return reply({ error: 'The AI service could not answer. Try again shortly.' }, 502);
  }

  const cost = costMicro(d.usage);
  const { data: total, error: chargeErr } = await sb.rpc('ai_charge', { p_email: who, p_month: month, p_micro: cost });
  if (chargeErr) console.warn('ai: could not record spending', chargeErr.message);
  const used = Math.min(1, Number(total ?? (Number(spent) + cost)) / cap);

  const text = (Array.isArray(d.content) ? d.content : []).filter(c => c && c.type === 'text').map(c => c.text).join('').trim();
  if (asked.task === 'tasks') {
    return reply({ tasks: parseTasks(text, asked.input.messages.map(m => m.id)), used });
  }
  return reply({ text, cut: !!asked.input.cut, used });
}
