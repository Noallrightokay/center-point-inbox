#!/usr/bin/env bash
# check-app.sh RATA.app [IDENTIFIER] — what a built iOS app must carry from
# tauri.ios.conf.json and prepare.sh: RATA's identifier (IDENTIFIER, else
# RATA_BUNDLE, else org.mailrata.desktop), iOS 16.4 as its minimum, iPhone
# only, the Info.plist keys prepare.sh adds, ITSAppUsesNonExemptEncryption
# as RATA_EXPORT_FRANCE says (left out unless it is left-out; prepare.sh
# says why), the committed privacy manifest at the top of the bundle, where
# iOS reads it, and RATA's own icon, not Tauri's. Exit 1 on anything
# missing. For .github/workflows/ios.yml and a store's build (TestFlight's
# names its identifier as the argument, and must give this the same
# RATA_EXPORT_FRANCE as prepare.sh).
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
# The committed identifier, unless one is named: a store's build sets
# org.mailrata.mail with --config (D9).
want CFBundleIdentifier "${2:-${RATA_BUNDLE:-org.mailrata.desktop}}"
want MinimumOSVersion 16.4
want UIFileSharingEnabled true
want LSSupportsOpeningDocumentsInPlace true
# NO only once the owner has left France out (prepare.sh); until then App
# Store Connect must ask, so the key must not be there to answer for it.
case "${RATA_EXPORT_FRANCE:-}" in
  left-out) want ITSAppUsesNonExemptEncryption false ;;
  "")
    if got="$(get ITSAppUsesNonExemptEncryption)"; then
      echo "::error::ITSAppUsesNonExemptEncryption is \"$got\", but no decision on France is recorded (RATA_EXPORT_FRANCE)"
      bad=1
    else
      echo "ITSAppUsesNonExemptEncryption: left out, so App Store Connect asks"
    fi
    ;;
  *)
    echo "::error::RATA_EXPORT_FRANCE is \"$RATA_EXPORT_FRANCE\": only left-out is understood"
    bad=1
    ;;
esac
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
