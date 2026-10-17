#!/usr/bin/env bash
# check-app.sh RATA.app — what a built iOS app must carry from
# tauri.ios.conf.json and prepare.sh: RATA's identifier (RATA_BUNDLE, else
# org.mailrata.desktop), iOS 15 as its minimum, iPhone only, the Info.plist
# keys prepare.sh adds, the committed privacy manifest at the top of the
# bundle, where iOS reads it, and RATA's own icon, not Tauri's. Exit 1 on
# anything missing. For .github/workflows/ios.yml.
set -uo pipefail
app="${1:?give the built .app}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
plist="$app/Info.plist"
bad=0
get() { plutil -extract "$1" raw -o - "$plist" 2>/dev/null; }
want() {
  local got
  got="$(get "$1")"
  echo "$1 = $got"
  [ "$got" = "$2" ] || { echo "::error::$1 is \"$got\", not \"$2\""; bad=1; }
}
# The committed identifier, or RATA_BUNDLE: a store's build sets
# org.mailrata.mail with --config (D9).
want CFBundleIdentifier "${RATA_BUNDLE:-org.mailrata.desktop}"
want MinimumOSVersion 15.0
want UIFileSharingEnabled true
want LSSupportsOpeningDocumentsInPlace true
want ITSAppUsesNonExemptEncryption false
for key in NSCameraUsageDescription NSMicrophoneUsageDescription; do
  got="$(get "$key")"
  echo "$key = $got"
  [ -n "$got" ] || { echo "::error::no $key"; bad=1; }
done
# iPhone (1) alone: Xcode writes this from TARGETED_DEVICE_FAMILY.
family="$(plutil -extract UIDeviceFamily json -o - "$plist" 2>/dev/null | tr -d ' \n')"
echo "UIDeviceFamily = $family"
[ "$family" = "[1]" ] || { echo "::error::UIDeviceFamily is $family, not [1] (iPhone only)"; bad=1; }
if cmp -s "$app/PrivacyInfo.xcprivacy" "$here/PrivacyInfo.xcprivacy"; then
  echo "PrivacyInfo.xcprivacy: in the bundle, as committed"
else
  echo "::error::the bundle has no PrivacyInfo.xcprivacy, or not the committed one"
  bad=1
fi
# The icon the home screen shows, which Xcode made from the asset catalog,
# looks like RATA's (make-icon.py --like). Read through sips first, which
# also reads an iPhone-optimised PNG.
want CFBundleIcons.CFBundlePrimaryIcon.CFBundleIconName AppIcon
icon="$(find "$app" -maxdepth 1 -name 'AppIcon60x60@2x.png' | head -n 1)"
[ -n "$icon" ] || icon="$(find "$app" -maxdepth 1 -name 'AppIcon*.png' | sort | tail -n 1)"
if [ -z "$icon" ]; then
  echo "::error::the bundle has no app icon"
  bad=1
else
  plain="$(mktemp -d)/icon.png"
  if sips -s format png "$icon" --out "$plain" >/dev/null 2>&1 \
    && python3 "$here/make-icon.py" --like "$plain"; then
    echo "$(basename "$icon"): RATA's icon"
  else
    echo "::error::$(basename "$icon") is not RATA's icon"
    bad=1
  fi
fi
exit "$bad"
