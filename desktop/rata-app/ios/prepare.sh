#!/usr/bin/env bash
# prepare.sh [GEN_APPLE] — RATA's own changes to the Xcode project that
# `tauri ios init` makes in desktop/rata-app/src-tauri/gen/apple. That
# project is not committed: the iOS job (.github/workflows/ios.yml) makes it
# afresh on every run, then runs this, then `tauri ios build`. Running it
# again changes nothing more. macOS only: it needs xcodegen and plutil.
#
# What it changes, and why:
#
# - project.yml: TARGETED_DEVICE_FAMILY "1", iPhone only. xcodegen's own
#   default is "1,2", which makes RATA an iPad app too, and Xcode writes
#   UIDeviceFamily from this setting into the built Info.plist, whatever the
#   source one says. The first store submission is the iPhone's; an iPad
#   brings its own layout and screenshots into review (the owner's call
#   later).
#
# - The app icon: AppIcon-1024.png (beside this script, made by
#   make-icon.py from the desktop's own artwork) as a single-size app icon,
#   from which Xcode makes every size a phone shows. `tauri ios init`
#   copies Tauri's own icons into the project, never RATA's.
#
# - Info.plist
#   - UIFileSharingEnabled, LSSupportsOpeningDocumentsInPlace: on iOS RATA
#     saves attachments into the app's Documents folder (core::save_folder),
#     and these make that folder show in the Files app. Without them a saved
#     file is somewhere nobody can open.
#   - ITSAppUsesNonExemptEncryption NO. Apple: "Set the value to NO if your
#     app—including any third-party libraries it links against—doesn't use
#     encryption, or if it only uses forms of encryption that are exempt
#     from export compliance documentation requirements" ("Complying with
#     Encryption Export Regulations", developer.apple.com/documentation/
#     security). RATA's own encryption is TLS for its connections (rustls
#     over aws-lc-rs: standard algorithms, for authentication and
#     transport), which runs in the app rather than in Apple's operating
#     system. Apple's table for that case ("Export compliance documentation
#     for encryption", App Store Connect Help): "Your app uses an industry
#     standard algorithm, not provided within the Apple operating system":
#     no US documentation (that is for proprietary algorithms not accepted
#     by IEEE, IETF or ITU), and "Upload your French encryption declaration
#     in App Store Connect", which is "only required if you're distributing
#     your app on the App Store in France". So NO holds for every App Store
#     country but France; distributing in France means filing that
#     declaration and setting the key as App Store Connect then directs.
#     The owner answers this in App Store Connect (plan risk 17), along
#     with the year-end self-classification report Apple says exempt
#     encryption may still need.
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

# iPhone only: beside the target's other settings in project.yml.
python3 - "$apple/project.yml" <<'PY'
import sys
path = sys.argv[1]
lines = open(path).read().split('\n')
if not any(l.strip().startswith('TARGETED_DEVICE_FAMILY:') for l in lines):
    for i, line in enumerate(lines):
        if line.strip() == 'ENABLE_BITCODE: false':
            indent = line[: len(line) - len(line.lstrip())]
            lines.insert(i + 1, indent + 'TARGETED_DEVICE_FAMILY: "1"')
            break
    else:
        sys.exit("prepare.sh: project.yml has no target settings (ENABLE_BITCODE) to add the device family to")
    open(path, 'w').write('\n'.join(lines))
PY

# RATA's icon, as the only one in the asset catalog's app icon set.
icons="$apple/Assets.xcassets/AppIcon.appiconset"
[ -d "$icons" ] || die "no app icon set at $icons"
[ -f "$here/AppIcon-1024.png" ] || die "no AppIcon-1024.png beside prepare.sh: run make-icon.py"
find "$icons" -maxdepth 1 -type f -name '*.png' ! -name 'AppIcon-1024.png' -delete
if ! cmp -s "$here/AppIcon-1024.png" "$icons/AppIcon-1024.png"; then
  cp "$here/AppIcon-1024.png" "$icons/AppIcon-1024.png"
fi
cat > "$icons/Contents.json" <<'JSON'
{
  "images" : [
    {
      "filename" : "AppIcon-1024.png",
      "idiom" : "universal",
      "platform" : "ios",
      "size" : "1024x1024"
    }
  ],
  "info" : {
    "author" : "xcode",
    "version" : 1
  }
}
JSON

# The privacy manifest, in the bundle's resources.
if ! cmp -s "$here/PrivacyInfo.xcprivacy" "$target_dir/PrivacyInfo.xcprivacy"; then
  cp "$here/PrivacyInfo.xcprivacy" "$target_dir/PrivacyInfo.xcprivacy"
fi

# The project made again when it lacks either.
if ! grep -q 'PrivacyInfo.xcprivacy in Resources' "${pbxproj[0]}" \
  || ! grep -q 'TARGETED_DEVICE_FAMILY = 1;' "${pbxproj[0]}"; then
  (cd "$apple" && xcodegen generate --spec project.yml)
fi
grep -q 'PrivacyInfo.xcprivacy in Resources' "${pbxproj[0]}" \
  || die "xcodegen did not put PrivacyInfo.xcprivacy in the app's resources"
grep -q 'TARGETED_DEVICE_FAMILY = 1;' "${pbxproj[0]}" \
  || die "xcodegen did not make the app iPhone only"
if grep -q 'TARGETED_DEVICE_FAMILY = "1,2";' "${pbxproj[0]}"; then
  die "the project still names the iPad too"
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

echo "prepare.sh: $(basename "$target_dir") is iPhone only, has RATA's icon, its privacy manifest and Info.plist keys:"
for key in UIFileSharingEnabled LSSupportsOpeningDocumentsInPlace ITSAppUsesNonExemptEncryption \
  NSCameraUsageDescription NSMicrophoneUsageDescription; do
  echo "  $key = $(plutil -extract "$key" raw -o - "$plist")"
done
