/* Fail the build on a critical advisory that actually reaches this app.

   `npm audit` knows which packages are installed; it cannot know which of their
   code paths a deployment runs. So a critical that provably cannot reach RATA
   may be named below with the reason and the test that keeps that reason true,
   and anything critical that is not named fails the build. That is stricter
   than lowering the threshold, which goes blind to every future critical.

   The list is empty, and should stay that way. It held two entries while RATA
   was on Next 14 — an image-optimizer RCE and a Windows-only RCE — and the
   Next 16 upgrade cleared both along with the other 25 advisories. An entry
   here is a debt, not a resting place. */

import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const ACCEPTED = {};

const run = promisify(execFile);
let report;
try {
  const { stdout } = await run('npm', ['audit', '--omit=dev', '--json'], { maxBuffer: 32 * 1024 * 1024 });
  report = JSON.parse(stdout);
} catch (e) {
  /* npm audit exits non-zero whenever it finds anything, so the JSON arrives
     on stdout of a "failed" call. Only a genuinely unparseable result is an
     error here. */
  try { report = JSON.parse(e.stdout || ''); }
  catch { console.error('audit-gate: could not read npm audit output\n' + (e.stderr || e.message)); process.exit(2); }
}

const unaccepted = [];
const accepted = [];

/* A package's `severity` is the rollup of its worst advisory, so every one of
   Next's two dozen findings reads "critical" at that level. The per-advisory
   severity inside `via` is the one to judge by. */
for (const [name, v] of Object.entries(report.vulnerabilities || {})) {
  for (const via of v.via || []) {
    if (typeof via !== 'object') continue;
    if (via.severity !== 'critical') continue;
    const id = (via.url || '').split('/').pop();
    const row = { pkg: name, id, title: via.title };
    if (ACCEPTED[id]) accepted.push(row); else unaccepted.push(row);
  }
}

for (const a of accepted) console.log(`accepted  ${a.id}  ${a.pkg}: ${a.title}\n          → ${ACCEPTED[a.id]}`);

if (!unaccepted.length) {
  console.log(`\nNo unaccepted critical advisories. (${accepted.length} accepted, documented in scripts/audit-gate.mjs)`);
  process.exit(0);
}

console.error('\nCritical advisories with no documented exception:\n');
for (const u of unaccepted) console.error(`  ${u.id}  ${u.pkg}: ${u.title}`);
console.error('\nFix the dependency, or add the advisory to ACCEPTED in scripts/audit-gate.mjs with the reason it cannot reach this app.');
process.exit(1);
