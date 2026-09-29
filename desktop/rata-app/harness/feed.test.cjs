'use strict';
// node --test desktop/rata-app/harness/feed.test.cjs
// The update feed's rules (feed.cjs), against fixtures shaped like what
// release.yml reads from GitHub.
const test = require('node:test');
const assert = require('node:assert/strict');
const { plan, keysFor, FILES, BUNDLES } = require('./feed.cjs');

const REPO = 'https://github.com/Noallrightokay/center-point-inbox/releases/download';
const ASSETS = {
  'linux-x86_64-appimage': (v) => `RATA_${v}_amd64.AppImage`,
  'linux-x86_64-deb': (v) => `RATA_${v}_amd64.deb`,
  'windows-x86_64-nsis': (v) => `RATA_${v}_x64-setup.exe`,
  'darwin-aarch64-app': (v) => `RATA_aarch64.app.tar.gz`,
  'darwin-x86_64-app': (v) => `RATA_x64.app.tar.gz`,
};
const installer = (bundle, v) => {
  const name = ASSETS[bundle](v);
  return { name, url: `${REPO}/v${v}/${name}`, signature: `sig ${bundle} ${v}` };
};
const release = (v, bundles) => bundles.map((b) => installer(b, v));
const ALL = Object.keys(BUNDLES);
const NO_DARWIN = ALL.filter((b) => !b.startsWith('darwin'));

/** latest.json as the workflow before this one wrote it: every platform of
 *  one release, windows and darwin under their short keys too. */
function oldCombined(v, bundles = ALL) {
  const platforms = {};
  for (const i of release(v, bundles)) for (const k of keysFor(i.name)) platforms[k] = { signature: i.signature, url: i.url };
  return { version: v, notes: '', pub_date: '2026-09-28T10:00:00Z', platforms };
}

test('a release missing darwin keeps darwin at the version it had', () => {
  const old = { 'latest.json': oldCombined('0.1.42') };
  const { write, kept } = plan({ version: '0.1.43', pubDate: '2026-09-29T10:00:00Z', installers: release('0.1.43', NO_DARWIN), old });
  assert.deepEqual(kept.sort(), ['darwin-aarch64-app', 'darwin-x86_64-app']);
  // darwin's own files are seeded from the old latest.json, still 0.1.42,
  // still pointing at 0.1.42's installer and its signature.
  const mac = write['latest-darwin-aarch64-app.json'];
  assert.equal(mac.version, '0.1.42');
  assert.deepEqual(Object.keys(mac.platforms).sort(), ['darwin-aarch64', 'darwin-aarch64-app']);
  assert.equal(mac.platforms['darwin-aarch64-app'].url, `${REPO}/v0.1.42/RATA_aarch64.app.tar.gz`);
  assert.equal(mac.platforms['darwin-aarch64-app'].signature, 'sig darwin-aarch64-app 0.1.42');
  assert.equal(write['latest-darwin-x86_64-app.json'].version, '0.1.42');
  // The rest move on.
  for (const b of NO_DARWIN) {
    const f = write[`latest-${b}.json`];
    assert.equal(f.version, '0.1.43', b);
    assert.equal(f.platforms[b].signature, `sig ${b} 0.1.43`);
  }
  // latest.json carries one version, so no darwin under 0.1.43: an 0.1.42
  // installer announced as 0.1.43 would fail requireSignedVersion.
  const all = write['latest.json'];
  assert.equal(all.version, '0.1.43');
  assert.ok(!Object.keys(all.platforms).some((k) => k.startsWith('darwin')), Object.keys(all.platforms).join());
  for (const [k, p] of Object.entries(all.platforms)) assert.match(p.url, /\/v0\.1\.43\//, k);
});

test('a platform with its own file keeps it untouched while it is missing', () => {
  const mine = { version: '0.1.43', notes: '', pub_date: 'x', platforms: { 'darwin-aarch64-app': { signature: 's', url: 'u' } } };
  const old = { 'latest.json': oldCombined('0.1.43', NO_DARWIN), 'latest-darwin-aarch64-app.json': mine };
  const { write, kept } = plan({ version: '0.1.44', pubDate: 'y', installers: release('0.1.44', NO_DARWIN), old });
  assert.ok(!('latest-darwin-aarch64-app.json' in write), 'left exactly as it is');
  assert.ok(kept.includes('darwin-aarch64-app'));
  // darwin-x86_64 had neither its own file nor a latest.json entry: nothing
  // is invented for it.
  assert.ok(!('latest-darwin-x86_64-app.json' in write));
  assert.ok(!kept.includes('darwin-x86_64-app'));
});

test('darwin comes back when a release has it again', () => {
  const old = { 'latest-darwin-aarch64-app.json': { version: '0.1.42', platforms: {} }, 'latest.json': oldCombined('0.1.43', NO_DARWIN) };
  const { write, kept } = plan({ version: '0.1.44', pubDate: 'z', installers: release('0.1.44', ALL), old });
  assert.deepEqual(kept, []);
  assert.equal(write['latest-darwin-aarch64-app.json'].version, '0.1.44');
  assert.ok('darwin-aarch64-app' in write['latest.json'].platforms);
});

test('nothing moves backwards', () => {
  const old = { 'latest.json': oldCombined('0.1.44'), 'latest-linux-x86_64-deb.json': { version: '0.1.44', platforms: {} } };
  const { write } = plan({ version: '0.1.43', pubDate: 'z', installers: release('0.1.43', ALL), old });
  assert.ok(!('latest.json' in write));
  assert.ok(!('latest-linux-x86_64-deb.json' in write));
  // A file seeded from the newer latest.json keeps the newer version.
  for (const [f, feed] of Object.entries(write)) assert.equal(feed.version, '0.1.44', f);
  // 1.0.0 is newer than 0.1.44, and 0.1.10 than 0.1.9.
  assert.equal(plan({ version: '1.0.0', pubDate: 'z', installers: release('1.0.0', ALL), old }).write['latest.json'].version, '1.0.0');
  const nine = { 'latest.json': oldCombined('0.1.9') };
  assert.equal(plan({ version: '0.1.10', pubDate: 'z', installers: release('0.1.10', ALL), old: nine }).write['latest.json'].version, '0.1.10');
});

test('a re-run of the same version writes it again', () => {
  const old = { 'latest.json': oldCombined('0.1.43') };
  const { write } = plan({ version: '0.1.43', pubDate: 'z', installers: release('0.1.43', ALL), old });
  assert.deepEqual(Object.keys(write).sort(), [...FILES].sort());
});

test('no signed installers leaves the feed alone', () => {
  const unsigned = release('0.1.43', ALL).map((i) => ({ ...i, signature: '' }));
  const other = [{ name: 'RATA_0.1.43_x64.dmg', url: 'u', signature: 's' }];
  for (const installers of [[], unsigned, other]) {
    const { write } = plan({ version: '0.1.43', pubDate: 'z', installers, old: { 'latest.json': oldCombined('0.1.42') } });
    assert.deepEqual(write, {});
  }
});

test('each installer goes under its own keys, never a bare linux-x86_64', () => {
  const { write } = plan({ version: '0.1.43', pubDate: 'z', installers: release('0.1.43', ALL) });
  for (const feed of Object.values(write)) assert.ok(!('linux-x86_64' in feed.platforms));
  const all = write['latest.json'].platforms;
  assert.deepEqual(Object.keys(all).sort(), [
    'darwin-aarch64', 'darwin-aarch64-app', 'darwin-x86_64', 'darwin-x86_64-app',
    'linux-x86_64-appimage', 'linux-x86_64-deb', 'windows-x86_64', 'windows-x86_64-nsis',
  ]);
  assert.match(all['linux-x86_64-deb'].url, /\.deb$/);
  assert.match(all['linux-x86_64-appimage'].url, /\.AppImage$/);
  // Each file holds only its own installation's keys.
  assert.deepEqual(Object.keys(write['latest-linux-x86_64-deb.json'].platforms), ['linux-x86_64-deb']);
  assert.deepEqual(Object.keys(write['latest-windows-x86_64-nsis.json'].platforms), ['windows-x86_64-nsis', 'windows-x86_64']);
  // .sig files, .dmg and the rest are not installers the updater takes.
  for (const n of ['RATA_0.1.43_amd64.deb.sig', 'RATA_0.1.43_x64.dmg', 'latest.json']) assert.deepEqual(keysFor(n), [], n);
});
