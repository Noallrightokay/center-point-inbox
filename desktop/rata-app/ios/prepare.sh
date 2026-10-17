#!/usr/bin/env bash
# prepare.sh [GEN_APPLE] — RATA's own changes to the Xcode project that
# `tauri ios init` makes in desktop/rata-app/src-tauri/gen/apple. That
# project is not committed: the iOS job (.github/workflows/ios.yml) makes it
# afresh on every run, then runs this, then `tauri ios build`. Running it
# again changes nothing more. macOS only: it needs xcodegen and plutil.
#
# What it changes, and why:
#
# - Info.plist
#   - UIFileSharingEnabled, LSSupportsOpeningDocumentsInPlace: on iOS RATA
#     saves attachments into the app's Documents folder (core::save_folder),
#     and these make that folder show in the Files app. Without them a saved
#     file is somewhere nobody can open.
#   - ITSAppUsesNonExemptEncryption false: RATA's only encryption is the
#     system's and standard TLS (rustls), which is exempt. The owner
#     confirms the export answer in App Store Connect.
#   - NSCameraUsageDescription and NSMicrophoneUsageDescription: the
#     composer's Attach is <input type="file" multiple> with no `accept`,
#     and for such an input WKWebView offers "Take Photo or Video" beside
#     the photo library and Files (WebKit's WKFileUploadPanel.mm: no
#     accepted types allows images and video, and the camera is offered
#     when the device has one). iOS ends an app that opens the camera, or
#     records a video's sound, without these sentences. The Format Bridge's
#     input and Import JSON accept documents only, so they offer no camera.
#
# - PrivacyInfo.xcprivacy (beside this script), in the app's resources: put
#   beside Info.plist, in the folder project.yml already takes in whole, and
#   the project made again with xcodegen so that it is in Copy Bundle
#   Resources. Info.plist is edited after that, since xcodegen writes it
#   afresh.
#
# - RATA.storekit (beside this script), the App Store's two subscriptions
#   with placeholder prices, as the StoreKit configuration of the app's
#   scheme: Run in Xcode then buys, restores and renews against it, with no
#   App Store account, while the app talks to mailrata.org as it always
#   does. Put beside project.yml, named in its file groups (in the project,
#   in no target, so never in the app) and as the target scheme's
#   `storeKitConfiguration`. Only a launch from Xcode uses it: `simctl
#   launch`, TestFlight and the App Store never do.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
apple="${1:-$here/../src-tauri/gen/apple}"
die() { echo "prepare.sh: $*" >&2; exit 1; }

[ -f "$apple/project.yml" ] || die "no project at $apple: run \`cargo tauri ios init\` in src-tauri first"
command -v xcodegen >/dev/null || die "xcodegen is not installed (brew install xcodegen)"

# The app target's folder, <name>_iOS, which holds Info.plist.
shopt -s nullglob
found=("$apple"/*_iOS/Info.plist)
shopt -u nullglob
[ "${#found[@]}" -eq 1 ] || die "expected one <name>_iOS/Info.plist in $apple, found ${#found[@]}"
plist="${found[0]}"
target_dir="$(dirname "$plist")"
pbxproj=("$apple"/*.xcodeproj/project.pbxproj)
[ -f "${pbxproj[0]}" ] || die "no .xcodeproj in $apple"

# The privacy manifest, in the bundle's resources.
if ! cmp -s "$here/PrivacyInfo.xcprivacy" "$target_dir/PrivacyInfo.xcprivacy"; then
  cp "$here/PrivacyInfo.xcprivacy" "$target_dir/PrivacyInfo.xcprivacy"
fi

# The StoreKit configuration, for Run in Xcode.
if ! cmp -s "$here/RATA.storekit" "$apple/RATA.storekit"; then
  cp "$here/RATA.storekit" "$apple/RATA.storekit"
fi
target_name="$(basename "$target_dir")"
python3 - "$apple/project.yml" "$target_name" <<'PY'
import re, sys
path, target = sys.argv[1], sys.argv[2]
lines = open(path).read().split('\n')
changed = False
# In the project's file groups: a flow list, `fileGroups: [a, b]`.
for i, line in enumerate(lines):
    m = re.match(r'^fileGroups:\s*\[(.*)\]\s*$', line)
    if m:
        groups = [g.strip() for g in m.group(1).split(',') if g.strip()]
        if 'RATA.storekit' not in groups:
            lines[i] = 'fileGroups: [' + ', '.join(groups + ['RATA.storekit']) + ']'
            changed = True
        break
else:
    sys.exit('prepare.sh: project.yml has no fileGroups list')
# The app target's own scheme: `  <target>:` under `targets:`, then its
# `    scheme:`, whose first key the configuration goes before.
try:
    start = lines.index('targets:')
    at = lines.index('  ' + target + ':', start)
    scheme = next(j for j in range(at + 1, len(lines))
                  if lines[j] == '    scheme:' or re.match(r'^  \S', lines[j]))
except (ValueError, StopIteration):
    sys.exit('prepare.sh: project.yml has no target ' + target)
if lines[scheme] != '    scheme:':
    sys.exit('prepare.sh: the target ' + target + ' in project.yml has no scheme')
key = '      storeKitConfiguration: RATA.storekit'
if lines[scheme + 1] != key:
    lines.insert(scheme + 1, key)
    changed = True
if changed:
    open(path, 'w').write('\n'.join(lines))
PY

scheme_file="$(dirname "${pbxproj[0]}")/xcshareddata/xcschemes/$target_name.xcscheme"
if ! grep -q 'PrivacyInfo.xcprivacy in Resources' "${pbxproj[0]}" \
  || ! grep -qs 'StoreKitConfigurationFileReference' "$scheme_file"; then
  (cd "$apple" && xcodegen generate --spec project.yml)
fi
grep -q 'PrivacyInfo.xcprivacy in Resources' "${pbxproj[0]}" \
  || die "xcodegen did not put PrivacyInfo.xcprivacy in the app's resources"
grep -A1 'StoreKitConfigurationFileReference' "$scheme_file" | grep -q 'RATA.storekit' \
  || die "xcodegen did not make RATA.storekit the StoreKit configuration of $scheme_file"
if grep -q 'RATA.storekit in Resources' "${pbxproj[0]}"; then
  die "RATA.storekit is in the app's resources: it is for Xcode only"
fi

# Info.plist. `plutil -replace` adds a key or replaces it, so a second run
# leaves the file as the first left it.
plutil -replace UIFileSharingEnabled -bool YES "$plist"
plutil -replace LSSupportsOpeningDocumentsInPlace -bool YES "$plist"
plutil -replace ITSAppUsesNonExemptEncryption -bool NO "$plist"
plutil -replace NSCameraUsageDescription -string \
  "RATA uses the camera only when you choose Take Photo or Video to attach to a message." "$plist"
plutil -replace NSMicrophoneUsageDescription -string \
  "RATA uses the microphone only to record the sound of a video you take to attach to a message." "$plist"
plutil -lint "$plist" >/dev/null

echo "prepare.sh: $(basename "$target_dir") has its privacy manifest and Info.plist keys:"
for key in UIFileSharingEnabled LSSupportsOpeningDocumentsInPlace ITSAppUsesNonExemptEncryption \
  NSCameraUsageDescription NSMicrophoneUsageDescription; do
  echo "  $key = $(plutil -extract "$key" raw -o - "$plist")"
done
echo "prepare.sh: Run in Xcode buys against RATA.storekit (scheme $target_name)"
