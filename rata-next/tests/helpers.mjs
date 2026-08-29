/* Shared test scaffolding: assertions, and starting/stopping a real
   `next start` so suites exercise the production build rather than dev. */
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
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

/* Each config-route case needs its own process environment, so suites start
   their own server rather than sharing one. `env` may be an object, or a
   function of the base URL for values like APP_URL that embed the port. */
export async function startServer({ env = {} } = {}) {
  const port = await freePort();
  const url = `http://127.0.0.1:${port}`;
  const resolved = typeof env === 'function' ? env(url) : env;

  /* detached:true puts the server in its own process group. `npx next start`
     spawns a `next-server` child that ignores a SIGTERM aimed at the parent —
     killing the group is what actually frees the port. */
  const proc = spawn('npx', ['next', 'start', '-p', String(port)], {
    env: { ...process.env, ...resolved, NEXT_TELEMETRY_DISABLED: '1' },
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: true,
  });
  let log = '';
  proc.stdout.on('data', d => { log += d; });
  proc.stderr.on('data', d => { log += d; });

  const stop = () => stopGroup(proc, port);

  for (let i = 0; i < 90; i++) {
    if (proc.exitCode !== null) throw new Error(`server exited early (${proc.exitCode}) on ${port}:\n${log}`);
    try {
      const r = await fetch(url + '/', { signal: AbortSignal.timeout(1500) });
      if (r.ok || r.status === 404) return { url, port, proc, stop };
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
