/* Shared test scaffolding: assertions, and starting/stopping a real
   `next start` so suites exercise the production build rather than dev. */
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { fileURLToPath } from 'node:url';
import { createServer as createHttpServer } from 'node:http';
import { randomUUID } from 'node:crypto';
import { setTimeout as sleep } from 'node:timers/promises';

export function makeChecker(state) {
  return (cond, msg) => {
    console.log(`${cond ? '  PASS ' : '  FAIL '} ${msg}`);
    if (!cond) state.fails++;
    return cond;
  };
}

/* Let the OS pick the port. Hardcoded ports collide with a server a previous
   run leaked, and the failure looks like a broken app rather than a busy port. */
function freePort() {
  return new Promise((res, rej) => {
    const srv = createServer();
    srv.on('error', rej);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => res(port));
    });
  });
}

/* Each case that sets environment variables needs its own process, so suites
   start their own server rather than sharing one. `env` may be an object, or
   a function of the base URL for a value that embeds the port (redirect.test
   points REDIRECT_TO at the server itself). */
export async function startServer({ env = {}, from } = {}) {
  const port = await freePort();
  const url = `http://127.0.0.1:${port}`;
  const resolved = typeof env === 'function' ? env(url) : env;

  /* detached:true puts the server in its own process group. `npx next start`
     spawns a `next-server` child that ignores a SIGTERM aimed at the parent —
     killing the group is what actually frees the port.
     `from` starts Next in that folder instead, naming the site's, as a host
     that does not run it from the site's own folder would. */
  const site = fileURLToPath(new URL('..', import.meta.url));
  const [cmd, args] = from
    ? [process.execPath, [`${site}node_modules/next/dist/bin/next`, 'start', site, '-p', String(port)]]
    : ['npx', ['next', 'start', '-p', String(port)]];
  const proc = spawn(cmd, args, {
    env: { ...process.env, ...resolved, NEXT_TELEMETRY_DISABLED: '1' },
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: true,
    ...(from ? { cwd: from } : {}),
  });
  let log = '';
  proc.stdout.on('data', d => { log += d; });
  proc.stderr.on('data', d => { log += d; });

  const stop = () => stopGroup(proc, port);

  for (let i = 0; i < 90; i++) {
    if (proc.exitCode !== null) throw new Error(`server exited early (${proc.exitCode}) on ${port}:\n${log}`);
    try {
      /* redirect:'manual' keeps readiness local: with REDIRECT_TO set, `/`
         answers 308 and following it would make startup depend on the live
         site being reachable. Any answer below 500 means Next is serving. */
      const r = await fetch(url + '/', { redirect: 'manual', signal: AbortSignal.timeout(1500) });
      if (r.status < 500) return { url, port, proc, stop, log: () => log };
    } catch { /* not up yet */ }
    await sleep(500);
  }
  await stop();
  throw new Error(`server did not start on ${port}:\n${log}`);
}

async function portClosed(port) {
  try {
    await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(400) });
    return false;
  } catch { return true; }
}

/* Resolving as soon as the parent exits is not enough: `next-server` outlives
   it briefly, and a leaked server would keep a CI job's stdio open. Wait until
   the port is genuinely closed, escalating to SIGKILL if it will not go. */
async function stopGroup(proc, port) {
  if (proc.exitCode === null) {
    try { process.kill(-proc.pid, 'SIGTERM'); } catch { try { proc.kill('SIGTERM'); } catch {} }
  }
  for (let i = 0; i < 20; i++) {
    if (await portClosed(port)) return;
    await sleep(250);
  }
  try { process.kill(-proc.pid, 'SIGKILL'); } catch {}
  for (let i = 0; i < 12; i++) {
    if (await portClosed(port)) return;
    await sleep(250);
  }
}

/* Supabase-shaped keys for the config-route guard tests. Not real credentials —
   the signature is literal text and the payload is what the guard inspects. */
export function fakeSupabaseKey(role) {
  const b64 = o => Buffer.from(JSON.stringify(o)).toString('base64url');
  return `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64({ iss: 'supabase', ref: 'demo', role })}.sig`;
}

/* A stand-in for the Supabase services the account route talks to —
   GoTrue for the session and the login, PostgREST for the tables — which
   records every request so a test can see what was deleted. `aiUsage`
   decides how the ai_usage table answers: 'rows' (it exists and holds two
   months), 'missing' (database.sql §5 never ran: PostgREST's PGRST205, a
   404), 'old-missing' (Postgres's 42P01 from an older PostgREST) or
   'broken' (any other failure). `sub` is the subscriptions row, if any.

   A browser signs in here too, as the public deletion page does: GoTrue's
   password grant answers a session for `password` and refuses anything
   else, logout ends the sessions its scope names as GoTrue does ('global',
   the default: every session the person has; 'local': the caller's only;
   'others': all but the caller's), and every answer carries CORS headers,
   since the page is on another origin. A session is believed only while it
   is in `tokens`: issued by this stand-in and not ended, or the fixed
   'a-session' the route tests send. */
export async function fakeSupabase() {
  const USER = { id: '0b5c1f7e-8a8b-4b8e-9c55-3f1d2a6b7c80', email: 'Leaver@Example.com', aud: 'authenticated', role: 'authenticated' };
  const fake = { aiUsage: 'rows', sub: null, calls: [], password: 'the right password', tokens: new Set(['a-session']) };
  const b64 = o => Buffer.from(JSON.stringify(o)).toString('base64url');
  const srv = createHttpServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    const cors = {
      'access-control-allow-origin': '*',
      'access-control-allow-methods': 'GET, POST, PUT, PATCH, DELETE, OPTIONS',
      'access-control-allow-headers': req.headers['access-control-request-headers'] || '*',
    };
    const json = (status, body) => { res.writeHead(status, { 'content-type': 'application/json', ...cors }); res.end(JSON.stringify(body)); };
    if (req.method === 'OPTIONS') { res.writeHead(204, cors); res.end(); return; }
    fake.calls.push(`${req.method} ${url.pathname}${url.search}`);
    let raw = '';
    req.on('data', d => { raw += d; });
    req.on('end', () => {
      const bearer = (req.headers.authorization || '').replace(/^Bearer /, '');
      if (req.method === 'POST' && url.pathname === '/auth/v1/token' && url.searchParams.get('grant_type') === 'password') {
        let body = {};
        try { body = JSON.parse(raw); } catch { /* refused below */ }
        if (String(body.email || '').toLowerCase() !== USER.email.toLowerCase() || body.password !== fake.password) {
          return json(400, { code: 400, error_code: 'invalid_credentials', msg: 'Invalid login credentials' });
        }
        const exp = Math.floor(Date.now() / 1000) + 3600;
        /* Each sign-in its own session, as GoTrue's session_id makes it. */
        const token = `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64({ sub: USER.id, email: USER.email.toLowerCase(), role: 'authenticated', aud: 'authenticated', exp, session_id: randomUUID() })}.sig`;
        fake.tokens.add(token);
        return json(200, { access_token: token, token_type: 'bearer', expires_in: 3600, expires_at: exp, refresh_token: 'a-refresh-token', user: USER });
      }
      if (req.method === 'POST' && url.pathname === '/auth/v1/logout') {
        if (!fake.tokens.has(bearer)) return json(401, { code: 401, error_code: 'bad_jwt', msg: 'invalid JWT' });
        const scope = url.searchParams.get('scope') || 'global';
        if (!['global', 'local', 'others'].includes(scope)) return json(400, { code: 400, error_code: 'validation_failed', msg: 'Unsupported scope' });
        for (const t of [...fake.tokens]) {
          if (scope === 'global' || (scope === 'local' ? t === bearer : t !== bearer)) fake.tokens.delete(t);
        }
        res.writeHead(204, cors); res.end(); return;
      }
      if (req.method === 'GET' && url.pathname === '/auth/v1/user') {
        return fake.tokens.has(bearer) ? json(200, USER) : json(401, { code: 401, error_code: 'bad_jwt', msg: 'invalid JWT' });
      }
      if (req.method === 'DELETE' && url.pathname === `/auth/v1/admin/users/${USER.id}`) return json(200, USER);
      if (req.method === 'GET' && url.pathname === '/rest/v1/subscriptions') return json(200, fake.sub ? [fake.sub] : []);
      if (req.method === 'DELETE' && url.pathname === '/rest/v1/workspaces') return json(200, [{ id: USER.id }]);
      if (req.method === 'DELETE' && url.pathname === '/rest/v1/subscriptions') return json(200, []);
      if (req.method === 'DELETE' && url.pathname === '/rest/v1/ai_usage') {
        if (fake.aiUsage === 'missing') return json(404, { code: 'PGRST205', details: null, hint: null, message: "Could not find the table 'public.ai_usage' in the schema cache" });
        if (fake.aiUsage === 'old-missing') return json(404, { code: '42P01', details: null, hint: null, message: 'relation "public.ai_usage" does not exist' });
        if (fake.aiUsage === 'broken') return json(500, { code: 'XX000', details: null, hint: null, message: 'disk on fire' });
        return json(200, [{ email: 'leaver@example.com' }, { email: 'leaver@example.com' }]);
      }
      json(404, { message: 'not in the stand-in' });
    });
  });
  await new Promise(res => srv.listen(0, '127.0.0.1', res));
  return { USER, fake, srv, url: `http://127.0.0.1:${srv.address().port}`, close: () => new Promise(res => { srv.close(() => res()); srv.closeAllConnections(); }) };
}
