/* Refreshes public/vendor/. Run with `npm run vendor:libs`.
 *
 * RATA loads nothing from a third-party CDN at runtime: every engine is served
 * from our own origin and precached by the service worker. That means these
 * files do not update themselves — this script is how they move.
 *
 * After changing a version here you must ALSO update, in the same commit:
 *   - public/app.html  → BR_LIBS paths (and the supa() loader for supabase)
 *   - public/sw.js     → the ENGINES list, and bump V so clients refetch
 * A vendored file the service worker doesn't know about is a file nobody gets.
 */
import { writeFile, mkdir } from 'node:fs/promises';
import { join } from 'node:path';

const OUT = 'public/vendor';

const LIBS = [
  { file: 'supabase-js-2.112.4.js',
    url: 'https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2.112.4/dist/umd/supabase.js' },
  { file: 'mammoth-1.8.0.browser.min.js',
    url: 'https://cdn.jsdelivr.net/npm/mammoth@1.8.0/mammoth.browser.min.js' },
  /* NOT the npm build. npm's `xlsx` is frozen at 0.18.5, which is vulnerable to
     prototype pollution when reading a crafted file (CVE-2023-30533, fixed in
     0.19.3). SheetJS publishes only to their own CDN now. The Bridge reads
     user-supplied spreadsheets, so this must stay on the SheetJS build. */
  { file: 'xlsx-0.20.3.full.min.js',
    url: 'https://cdn.sheetjs.com/xlsx-0.20.3/package/dist/xlsx.full.min.js' },
  { file: 'jspdf-2.5.1.umd.min.js',
    url: 'https://cdn.jsdelivr.net/npm/jspdf@2.5.1/dist/jspdf.umd.min.js' },
];

await mkdir(OUT, { recursive: true });

let failed = 0;
for (const { file, url } of LIBS) {
  process.stdout.write(`${file.padEnd(34)} `);
  try {
    const r = await fetch(url);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    const buf = Buffer.from(await r.arrayBuffer());
    if (buf.length < 10_000) throw new Error(`suspiciously small (${buf.length} bytes)`);
    await writeFile(join(OUT, file), buf);
    console.log(`ok  ${buf.length.toLocaleString()} bytes`);
  } catch (e) {
    console.log(`FAILED — ${e.message}`);
    failed++;
  }
}

if (failed) {
  console.error(`\n${failed} of ${LIBS.length} failed. Existing files were left untouched.`);
  process.exit(1);
}
console.log(`\nAll ${LIBS.length} vendored. Now bump V in public/sw.js so installed clients refetch.`);
