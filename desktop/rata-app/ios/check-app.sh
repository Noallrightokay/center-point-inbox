#!/usr/bin/env bash
# check-app.sh RATA.app [IDENTIFIER] — what a built iOS app must carry from
# tauri.ios.conf.json and prepare.sh: RATA's identifier (org.mailrata.desktop
# unless another is named, as the TestFlight build's store bundle id is), iOS 15
# as its minimum, the Info.plist keys prepare.sh adds, and the committed privacy
# manifest at the top of the bundle, where iOS reads it. Exit 1 on anything
# missing. For .github/workflows/ios.yml and ios-testflight.yml.
set -uo pipefail
app="${1:?give the built .app}"
identifier="${2:-org.mailrata.desktop}"
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
want CFBundleIdentifier "$identifier"
want MinimumOSVersion 15.0
want UIFileSharingEnabled true
want LSSupportsOpeningDocumentsInPlace true
want ITSAppUsesNonExemptEncryption false
for key in NSCameraUsageDescription NSMicrophoneUsageDescription; do
  got="$(get "$key")"
  echo "$key = $got"
  [ -n "$got" ] || { echo "::error::no $key"; bad=1; }
done
if cmp -s "$app/PrivacyInfo.xcprivacy" "$here/PrivacyInfo.xcprivacy"; then
  echo "PrivacyInfo.xcprivacy: in the bundle, as committed"
else
  echo "::error::the bundle has no PrivacyInfo.xcprivacy, or not the committed one"
  bad=1
fi
exit "$bad"
