'use strict';
// The update feed, worked out with no network: release.yml's publish job
// reads the release and the `updater` release, calls plan(), and uploads what
// it returns. feed.test.cjs runs it against fixtures.
//
// Two kinds of file on the `updater` release:
//
//   latest-<key>.json, one per kind of installation (BUNDLES). Copies from
//   after 0.1.42 read their own first (tauri.conf.json's first endpoint, which the
//   updater plugin fills in as latest-{{target}}-{{arch}}-{{bundle_type}}).
//   Each carries its own version, so a platform whose installer was taken off
//   a release (a failed launch or signature check) keeps its file, and keeps
//   offering the last version it had, until a release that has it again.
//
//   latest.json, one version for every platform: this release's signed
//   installers only. What copies up to 0.1.42 read, and the fallback when a
//   copy's own file is not there. A platform missing from a release is left
//   out of it rather than kept at its older installer: the app insists the
//   signature names the version the feed announces (requireSignedVersion), so
//   an older installer under this release's version would be offered, then
//   refused as "The update's signature did not check out". Left out, the
//   plugin answers "no entry for this computer", which update.rs shows as no
//   update yet.
//
// No file ever moves backwards, and there is never a bare `linux-x86_64` key,
// which would hand a .deb install the AppImage.

/** The five kinds of installation, each with the keys the plugin looks for:
 *  `{os}-{arch}-{installer}` first, then `{os}-{arch}` (a copy whose
 *  installer the plugin cannot tell). */
const BUNDLES = {
  'linux-x86_64-appimage': ['linux-x86_64-appimage'],
  'linux-x86_64-deb': ['linux-x86_64-deb'],
  'windows-x86_64-nsis': ['windows-x86_64-nsis', 'windows-x86_64'],
  'darwin-aarch64-app': ['darwin-aarch64-app', 'darwin-aarch64'],
  'darwin-x86_64-app': ['darwin-x86_64-app', 'darwin-x86_64'],
};

const COMBINED = 'latest.json';
const fileFor = (bundle) => `latest-${bundle}.json`;
/** Every file this feed writes, so the caller knows which to read first. */
const FILES = [...Object.keys(BUNDLES).map(fileFor), COMBINED];

/** Which kind of installation a release asset is, if it is one. */
function bundleOf(name) {
  if (name.endsWith('.AppImage')) return 'linux-x86_64-appimage';
  if (name.endsWith('.deb')) return 'linux-x86_64-deb';
  if (name.endsWith('-setup.exe')) return 'windows-x86_64-nsis';
  if (name.endsWith('.app.tar.gz')) return /aarch64|arm64/.test(name) ? 'darwin-aarch64-app' : 'darwin-x86_64-app';
  return null;
}

/** The feed keys an asset goes under; none for anything else. */
const keysFor = (name) => {
  const b = bundleOf(name);
  return b ? [...BUNDLES[b]] : [];
};

/** a − b as versions: positive when a is newer. Numbers only, as before. */
function compare(a, b) {
  const n = (v) => String(v || '0').replace(/^v/, '').split(/[.-]/).map((x) => Number(x) || 0);
  const [x, y] = [n(a), n(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] || 0) - (y[i] || 0);
    if (d) return d;
  }
  return 0;
}

/**
 * What to write.
 *   version     this release's version, without the v
 *   pubDate     when it was published
 *   installers  [{ name, url, signature }]: this release's assets that have a
 *               .sig, with the signature's text
 *   old         { fileName: parsed feed } for the FILES already on the
 *               `updater` release (absent ones left out)
 * Returns { write: { fileName: feed }, kept: [bundle], log: [line] }.
 * Files not in `write` are left exactly as they are.
 */
function plan({ version, pubDate, installers, old = {} }) {
  const log = [];
  const write = {};
  const kept = [];
  const fresh = {};
  for (const i of installers) {
    const b = bundleOf(i.name);
    if (!b || !i.signature) continue;
    fresh[b] = { signature: i.signature, url: i.url };
  }
  if (!Object.keys(fresh).length) {
    log.push('No signed installers on this release (no update key is configured), so the feed is left alone.');
    return { write, kept, log };
  }
  const feedOf = (v, date, notes, bundles) => {
    const platforms = {};
    for (const [b, entry] of bundles) for (const k of BUNDLES[b]) platforms[k] = { signature: entry.signature, url: entry.url };
    return { version: v, notes: notes || '', pub_date: date, platforms };
  };
  const was = old[COMBINED];

  for (const b of Object.keys(BUNDLES)) {
    const file = fileFor(b);
    // What this kind of installation is offered now: its own file, or,
    // before there were such files, its entry in latest.json.
    let base = old[file] || null;
    let seeded = false;
    if (!base && was && was.platforms && was.platforms[b]) {
      base = feedOf(was.version, was.pub_date, was.notes, [[b, was.platforms[b]]]);
      seeded = true;
    }
    if (fresh[b] && (!base || compare(version, base.version) >= 0)) {
      write[file] = feedOf(version, pubDate, '', [[b, fresh[b]]]);
      log.push(`${b}: ${version}.`);
    } else if (fresh[b]) {
      if (seeded) write[file] = base;
      log.push(`${b}: already offers ${base.version}, newer than ${version}; left there.`);
    } else if (base) {
      if (seeded) write[file] = base;
      kept.push(b);
      log.push(`${b}: not on this release; keeps ${base.version}.`);
    } else {
      log.push(`${b}: not on this release and never offered; no update for it yet.`);
    }
  }

  if (was && compare(was.version, version) > 0) {
    log.push(`${COMBINED}: already offers ${was.version}, newer than ${version}; left alone.`);
  } else {
    write[COMBINED] = feedOf(version, pubDate, '', Object.entries(fresh));
    log.push(`${COMBINED}: ${version} for ${Object.keys(write[COMBINED].platforms).join(', ')}.`);
  }
  return { write, kept, log };
}

module.exports = { BUNDLES, FILES, COMBINED, fileFor, bundleOf, keysFor, compare, plan };
