#!/usr/bin/env bash
# testflight.sh STEP — RATA for iOS, signed for the App Store and sent to
# TestFlight, so the owner can install it on a real iPhone. Each step of
# .github/workflows/ios-testflight.yml is one STEP here, so that the
# rehearsal with made-up values, which runs on every pull request, goes
# through the very code a real upload does. Tests: testflight-test.sh.
#
# Four repository secrets, all or none, like the release workflow's signing
# secrets:
#
#   APPLE_API_KEY_ID     the App Store Connect API key's Key ID
#   APPLE_API_ISSUER_ID  the Issuer ID shown above the keys table
#   APPLE_API_KEY_P8     the key's .p8 file, whole
#   APPLE_TEAM_ID        the developer team's Team ID
#
# The key's three decide whether TestFlight is set up. APPLE_TEAM_ID is not
# TestFlight's alone: release.yml's macOS notarisation reads the same secret
# (NOTARY_TEAM), so it may well be set while TestFlight is not. Alone it is
# no sign of TestFlight; once any of the key's three is set, it is required.
# No message here ever tells the owner to delete it.
#
# And the repository variable RATA_IOS_BUNDLE_ID, the bundle id of the App
# Store Connect app record (org.mailrata.app when unset). The desktop's
# identifier, org.mailrata.desktop, stays in tauri.conf.json: this build is
# given the store's through Tauri's --config, never by editing the config.
# The real RATA_LICENCE_PUBLIC_KEY is required too, since a build without it
# refuses every licence.
#
# Steps, in the order the workflow runs them:
#
#   check          Is TestFlight set up? None of the key's three: say so
#                  and stop, green, whether or not APPLE_TEAM_ID is set.
#                  Some of the four: fail, naming the missing ones, before
#                  anything is built. All: check each one's shape. Never
#                  prints a value. Writes `build` (true or false) and
#                  `bundle_id` to $GITHUB_OUTPUT; on a pull request
#                  (RATA_CHECK_ONLY=1) `build` stays false.
#   rehearse CASE  The same with made-up values, for CI: `none` (nothing,
#                  then APPLE_TEAM_ID alone, as notarisation leaves it),
#                  `half` (part of the key, without and with the team) and
#                  `made-up` run `check` and assert what it must say;
#                  `apple` runs `sign` and `upload` with a made-up key,
#                  which Apple must refuse, and asserts that the job says
#                  plainly what to fix and prints none of the values (when
#                  Apple does not answer at all, it warns instead);
#                  `inspect` runs `inspect` on the unsigned build, as it is
#                  and with made-up profiles of both kinds, which it must
#                  refuse, each time for exactly the reasons it has. On a
#                  fork's pull request (RATA_FORK=1), which GitHub gives no
#                  secrets, `made-up` checks with a made-up licence key and
#                  builds nothing.
#   xcode          Selects the newest Xcode on the runner; App Store Connect
#                  takes only builds made with Xcode 26 and its iOS 26 SDK.
#   licence-key    Hands RATA_LICENCE_PUBLIC_KEY to the Rust build.
#   project        Makes the Xcode project (tauri ios init, prepare.sh)
#                  with the store's bundle id and this run's build number,
#                  which the export is told to keep (export-options).
#   archive        Builds RATA, release, unsigned, for a generic iOS device,
#                  and checks the app: no Apple value is in the environment
#                  while the Rust code and its build scripts compile.
#   sign           Writes the .p8 to a file only this job can read and
#                  builds again with tauri ios build --export-method
#                  app-store-connect, which signs through the API key.
#   inspect        Checks the signed .ipa before it goes anywhere.
#   upload         xcrun altool --upload-app --type ios, with the API key.
#   forget         Deletes the .p8 from the runner (always run last).
#
# Two helpers, for the steps above and the tests: `redact` (a filter that
# replaces the key's, issuer's and team's ids and the key itself) and
# `explain sign|upload LOG` (says what the owner must fix after a failure).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
self="$here/$(basename "${BASH_SOURCE[0]}")"
# The shell project; the tests point it at a stand-in.
TAURI_DIR="${RATA_TAURI_DIR:-$here/../src-tauri}"
WORK="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/rata-testflight"
# The key's folder. altool looks for AuthKey_<Key ID>.p8 in ./private_keys,
# so it runs from here; xcodebuild is given the file's path.
ASC="$WORK/asc"
OVERRIDE="$WORK/override.json"

DEFAULT_BUNDLE="org.mailrata.app"
XCODE_MIN=26
# The App Store Connect key's three, which decide whether TestFlight is set
# up, and with them the team, which macOS notarisation shares (above).
KEY_SECRETS="APPLE_API_KEY_ID APPLE_API_ISSUER_ID APPLE_API_KEY_P8"
SECRETS="$KEY_SECRETS APPLE_TEAM_ID"
SETUP="docs/APP-STORES.md"

die() { echo "::error::$*"; exit 1; }
put() { echo "$1=$2" >> "${GITHUB_OUTPUT:-/dev/null}"; }
trim() {
  local s="$1"
  s="${s#"${s%%[![:space:]]*}"}"
  s="${s%"${s##*[![:space:]]}"}"
  printf '%s' "$s"
}

# The three ids and the bundle id, without the blank space a pasted secret
# can carry. The .p8 is never copied: it is read where it is written out.
load() {
  KEY_ID="$(trim "${APPLE_API_KEY_ID:-}")"
  ISSUER="$(trim "${APPLE_API_ISSUER_ID:-}")"
  TEAM="$(trim "${APPLE_TEAM_ID:-}")"
  BUNDLE="$(trim "${RATA_IOS_BUNDLE_ID:-}")"
  [ -n "$BUNDLE" ] || BUNDLE="$DEFAULT_BUNDLE"
}

ok_key_id() { local re='^[A-Z0-9]{10}$'; [[ $1 =~ $re ]]; }
ok_issuer() { local re='^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'; [[ $1 =~ $re ]]; }
ok_team() { local re='^[A-Z0-9]{10}$'; [[ $1 =~ $re ]]; }
ok_bundle() { local re='^[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+$'; [[ $1 =~ $re ]] && [ "${#1}" -le 155 ]; }
ok_p8() {
  case "$1" in *"-----BEGIN PRIVATE KEY-----"*"-----END PRIVATE KEY-----"*) return 0 ;; esac
  return 1
}
ok_licence() {
  case "$1" in *"-----BEGIN PUBLIC KEY-----"*"-----END PUBLIC KEY-----"*) return 0 ;; esac
  return 1
}

check() {
  load
  local name set=0 missing="" bad=0 keep=""
  for name in $KEY_SECRETS; do
    if [ -n "$(trim "${!name:-}")" ]; then
      set=$((set + 1))
    else
      missing="${missing:+$missing, }$name"
    fi
  done
  [ -n "$TEAM" ] || missing="${missing:+$missing, }APPLE_TEAM_ID"
  put bundle_id "$BUNDLE"
  if [ "$set" -eq 0 ]; then
    echo "::notice::TestFlight is not set up yet: $SETUP"
    echo "None of APPLE_API_KEY_ID, APPLE_API_ISSUER_ID and APPLE_API_KEY_P8 is set, so this job builds nothing and uploads nothing."
    if [ -n "$TEAM" ]; then
      echo "APPLE_TEAM_ID is set. The desktop release's macOS notarisation uses it, and TestFlight will use the same secret once the App Store Connect key's three are added."
    fi
    if [ "${RATA_FORK:-}" = 1 ]; then
      echo "This pull request comes from a fork, and GitHub gives a fork's pull request none of this repository's secrets, so this says nothing of whether they are set."
    fi
    put build false
    return 0
  fi
  if [ -n "$missing" ]; then
    [ -z "$TEAM" ] || keep=" Keep APPLE_TEAM_ID either way: the desktop release's macOS notarisation uses it too."
    echo "::error::TestFlight is half set up. Missing: $missing. The App Store Connect key's three secrets, APPLE_API_KEY_ID, APPLE_API_ISSUER_ID and APPLE_API_KEY_P8, go together, and with them APPLE_TEAM_ID, the same Team ID macOS notarisation uses. Add the missing ones in the repository's Settings > Secrets and variables > Actions, or delete the key's three to switch TestFlight off ($SETUP).$keep"
    put build false
    return 1
  fi
  ok_key_id "$KEY_ID" || {
    echo "::error::APPLE_API_KEY_ID is not a Key ID. It is the 10 capital letters and digits in the Key ID column of App Store Connect, Users and Access > Integrations > App Store Connect API."
    bad=1
  }
  ok_issuer "$ISSUER" || {
    echo "::error::APPLE_API_ISSUER_ID is not an Issuer ID. It is the long id with dashes shown above the keys table in App Store Connect, Users and Access > Integrations > App Store Connect API."
    bad=1
  }
  ok_p8 "${APPLE_API_KEY_P8:-}" || {
    echo "::error::APPLE_API_KEY_P8 is not a .p8 key. Paste the whole AuthKey_<Key ID>.p8 file Apple let you download once, from -----BEGIN PRIVATE KEY----- to -----END PRIVATE KEY-----."
    bad=1
  }
  ok_team "$TEAM" || {
    echo "::error::APPLE_TEAM_ID is not a Team ID. It is the 10 capital letters and digits under Membership details at developer.apple.com/account."
    bad=1
  }
  ok_bundle "$BUNDLE" || {
    echo "::error::The repository variable RATA_IOS_BUNDLE_ID is \"$BUNDLE\", which is not a bundle id: letters, digits and hyphens between full stops, such as $DEFAULT_BUNDLE, exactly as on the App Store Connect app record."
    bad=1
  }
  if [ -z "${RATA_LICENCE_PUBLIC_KEY:-}" ]; then
    echo "::error::RATA_LICENCE_PUBLIC_KEY is not set. A TestFlight build without it refuses every licence. It is the repository secret the release workflow uses (rata-next/LICENSING.md)."
    bad=1
  elif ! ok_licence "$RATA_LICENCE_PUBLIC_KEY"; then
    echo "::error::RATA_LICENCE_PUBLIC_KEY does not look like a PEM public key."
    bad=1
  fi
  if [ "$bad" -ne 0 ]; then
    put build false
    return 1
  fi
  echo "TestFlight is set up: the App Store Connect key's three secrets and APPLE_TEAM_ID are present, and each has the right shape. Bundle id: $BUNDLE."
  if [ "${RATA_CHECK_ONLY:-}" = 1 ]; then
    echo "::notice::On a pull request this job stops after the configuration check; it builds, signs and uploads only from main or when started by hand."
    put build false
  else
    put build true
  fi
}

# --- Made-up values, for the rehearsals --------------------------------------

# A Key ID, an Issuer ID and a Team ID of the right shapes, and a real P-256
# key in the .p8's format, none of them anybody's. Masked like secrets, so
# the log reads as a real run's would.
made_up() {
  local dir="$WORK/made-up" line
  rm -rf "$dir"
  (umask 077 && mkdir -p "$dir")
  MU_KEY_ID="$(python3 -c 'import secrets, string; print("".join(secrets.choice(string.ascii_uppercase + string.digits) for _ in range(10)))')"
  MU_TEAM="$(python3 -c 'import secrets, string; print("".join(secrets.choice(string.ascii_uppercase + string.digits) for _ in range(10)))')"
  MU_ISSUER="$(python3 -c 'import uuid; print(uuid.uuid4())')"
  openssl ecparam -name prime256v1 -genkey -noout -out "$dir/ec.pem" 2>/dev/null
  openssl pkcs8 -topk8 -nocrypt -in "$dir/ec.pem" -out "$dir/made-up.p8"
  rm -f "$dir/ec.pem"
  MU_P8="$(cat "$dir/made-up.p8")"
  # A licence key of the right shape, for a fork's pull request only, which
  # GitHub gives no secrets: it is never compiled into anything.
  MU_LICENCE="$(openssl pkey -in "$dir/made-up.p8" -pubout 2>/dev/null)"
  echo "::add-mask::$MU_KEY_ID"
  echo "::add-mask::$MU_TEAM"
  echo "::add-mask::$MU_ISSUER"
  while IFS= read -r line; do
    case "$line" in -----*|"") ;; *) echo "::add-mask::$line" ;; esac
  done < "$dir/made-up.p8"
}

# Fails, naming which, when any made-up value is in one of the files.
none_of_them_in() {
  local f v line found=""
  for f in "$@"; do
    for v in "$MU_KEY_ID" "$MU_TEAM" "$MU_ISSUER"; do
      if grep -qiF -- "$v" "$f"; then found="$found $(basename "$f")"; fi
    done
    while IFS= read -r line; do
      case "$line" in -----*|"") continue ;; esac
      if grep -qF -- "$line" "$f"; then found="$found $(basename "$f")(key)"; fi
    done <<< "$MU_P8"
  done
  [ -z "$found" ] || { echo "::error::A made-up value was printed in:$found"; return 1; }
}

# What a step printed, shown under the rehearsal with each line indented, so
# that its ::error:: and ::notice:: lines read as text and are not taken
# for this job's own.
show() {
  echo "--- what the owner would see (${2:-}) ---"
  sed 's/^/  | /' "$1"
  echo "--- end ---"
}

rehearse() {
  case "${1:-}" in
    none | half | made-up) rehearse_check "$1" ;;
    apple) rehearse_apple ;;
    inspect) rehearse_inspect ;;
    *) die "rehearse what? none, half, made-up, apple or inspect" ;;
  esac
}

# Runs `check` as the workflow would with only the NAME=VALUE pairs given
# among the four secrets, into $WORK/rehearsal-LABEL.log and .out, shows
# what it printed, and leaves its status in $status.
run_check() {
  local label="$1"
  shift
  log="$WORK/rehearsal-$label.log"
  outputs="$WORK/rehearsal-$label.out"
  : > "$outputs"
  set +e
  (
    # shellcheck disable=SC2086 # the four names, split on purpose
    unset $SECRETS RATA_CHECK_ONLY RATA_FORK
    export GITHUB_OUTPUT="$outputs"
    # NAME=VALUE pairs; the count first, for the bash 3.2 of macOS, whose
    # set -u calls an empty "$@" unbound.
    # shellcheck disable=SC2163
    [ "$#" -eq 0 ] || export "$@"
    exec bash "$self" check
  ) > "$log" 2>&1
  status=$?
  set -e
  show "$log" "check, $label"
}

# Each sentence the check says, by what it must and must not say.
says() { grep -qF -- "$1" "$log" || { echo "::error::$2"; bad=1; }; }
never_says() { if grep -qiE -- "$1" "$log"; then echo "::error::$2"; bad=1; fi; }
ends() {
  [ "$status" -eq "$1" ] || { echo "::error::$3 the check must end $1, and it ended $status."; bad=1; }
  grep -qx "build=$2" "$outputs" || { echo "::error::$3 the check must say build=$2."; bad=1; }
  none_of_them_in "$log" "$outputs" || bad=1
}

rehearse_check() {
  local case="$1" log outputs status bad=0 licence="${RATA_LICENCE_PUBLIC_KEY:-}" fork="${RATA_FORK:-}"
  mkdir -p "$WORK"
  made_up
  case "$case" in
    none)
      run_check none
      ends 0 false "With nothing set"
      says "::notice::TestFlight is not set up yet: $SETUP" "With nothing set the check must say \"TestFlight is not set up yet: $SETUP\"."
      # As the desktop release's notarisation leaves it: the team, no key.
      run_check team-alone APPLE_TEAM_ID="$MU_TEAM"
      ends 0 false "With APPLE_TEAM_ID alone, as macOS notarisation sets it,"
      says "::notice::TestFlight is not set up yet: $SETUP" "With APPLE_TEAM_ID alone the check must still say \"TestFlight is not set up yet: $SETUP\"."
      says "macOS notarisation uses it" "With APPLE_TEAM_ID alone the check must say that notarisation uses it."
      never_says '^::error::' "With APPLE_TEAM_ID alone the check must find nothing wrong."
      ;;
    half)
      run_check half APPLE_API_KEY_ID="$MU_KEY_ID" APPLE_API_KEY_P8="$MU_P8"
      ends 1 false "Half set up,"
      says "::error::TestFlight is half set up. Missing: APPLE_API_ISSUER_ID, APPLE_TEAM_ID." "Half set up, the check must name exactly the two missing secrets."
      # The owner with notarisation set up, adding the key one secret at a
      # time: told what is missing, and to keep the team.
      run_check half-with-team APPLE_TEAM_ID="$MU_TEAM" APPLE_API_KEY_ID="$MU_KEY_ID"
      ends 1 false "With APPLE_TEAM_ID and one of the key's three,"
      says "::error::TestFlight is half set up. Missing: APPLE_API_ISSUER_ID, APPLE_API_KEY_P8." "With APPLE_TEAM_ID and the Key ID, the check must name the issuer and the .p8 as missing, and nothing else."
      says "Keep APPLE_TEAM_ID either way" "With APPLE_TEAM_ID set, the check must say to keep it."
      never_says "delete (the others|APPLE_TEAM_ID|all four)" "The check must never tell the owner to delete APPLE_TEAM_ID, which notarisation uses."
      ;;
    made-up)
      # A fork's pull request gets no secrets, the licence key's included.
      [ "$fork" != 1 ] || licence="$MU_LICENCE"
      run_check made-up APPLE_API_KEY_ID="$MU_KEY_ID" APPLE_API_ISSUER_ID="$MU_ISSUER" \
        APPLE_API_KEY_P8="$MU_P8" APPLE_TEAM_ID="$MU_TEAM" RATA_LICENCE_PUBLIC_KEY="$licence"
      ends 0 true "With all four set"
      ;;
  esac
  [ "$bad" -eq 0 ] || exit 1
  echo "The check said what it must with $case values, and printed none of them."
  put bundle_id "$(sed -n 's/^bundle_id=//p' "$outputs" | tail -n 1)"
  # The made-up rehearsal goes on to build, sign and upload; the other two
  # end here, as a real run would. So does the made-up one on a fork's pull
  # request: without the real licence key nothing is built.
  if [ "$case" = made-up ] && [ "$fork" = 1 ]; then
    echo "::notice::This pull request comes from a fork, which GitHub gives none of this repository's secrets, so the made-up rehearsal ends after the check. The build, the signing with a made-up key and the checks of the signed app run on the pull requests of this repository's own branches."
    put build false
  elif [ "$case" = made-up ]; then
    put build true
  else
    put build false
  fi
}

# Signing and uploading with a made-up key: both reach Apple, which refuses
# the key. The job must say so in words the owner can act on, and print none
# of the values. When Apple does not answer at all, the rehearsal cannot see
# that and says so in a warning, rather than failing a pull request for
# Apple's servers.
rehearse_apple() {
  local bad=0 status log errors unanswered=""
  [ -f "$OVERRIDE" ] || die "rehearse apple runs after project and archive"
  [ -f "$WORK/unsigned/RATA.ipa" ] || die "rehearse apple needs the unsigned .ipa archive makes"
  made_up
  for step in sign upload; do
    log="$WORK/rehearsal-$step.log"
    set +e
    (
      export APPLE_API_KEY_ID="$MU_KEY_ID" APPLE_API_ISSUER_ID="$MU_ISSUER" \
        APPLE_API_KEY_P8="$MU_P8" APPLE_TEAM_ID="$MU_TEAM" \
        IPA="$WORK/unsigned/RATA.ipa" GITHUB_ENV=/dev/null
      exec bash "$self" "$step"
    ) > "$log" 2>&1
    status=$?
    set -e
    show "$log" "$step, with a made-up key"
    none_of_them_in "$log" || bad=1
    # grep -c ends 1 when it counts none, which set -e would take for a failure.
    errors="$(grep -c '^::error::' "$log" || true)"
    if [ "$status" -ne 0 ] && [ "$errors" -eq 1 ] && grep -qF "::error::$NO_ANSWER" "$log"; then
      echo "::warning::Apple's servers did not answer $step, so this run could not see Apple refuse the made-up key. That is Apple's side, not this change: the next run tries again."
      unanswered="$unanswered $step"
      continue
    fi
    [ "$status" -ne 0 ] || { echo "::error::$step with a made-up key must fail, and it passed."; bad=1; }
    grep -qF "::error::$KEY_REFUSED" "$log" || {
      echo "::error::$step with a made-up key must say that Apple did not accept the key."
      bad=1
    }
    # And nothing else: what else Apple refused follows from the key, and an
    # owner told to register the bundle id or wait for Apple would look in
    # the wrong place.
    [ "$errors" -eq 1 ] || {
      echo "::error::$step with a made-up key must say only that Apple did not accept the key, and it said $errors things."
      bad=1
    }
  done
  [ "$bad" -eq 0 ] || exit 1
  if [ -n "$unanswered" ]; then
    echo "Apple did not answer:$unanswered. What did run failed as it must, said why, and printed none of the values."
  else
    echo "Signing and uploading with a made-up key failed as they must, said why, and printed none of the values."
  fi
}

# A provisioning profile for the rehearsal of inspect: Apple's plist, signed
# (CMS, as Apple's are) with a certificate made here, which nothing trusts.
# KIND is `store` (what an App Store profile holds) or `development` (a
# debuggable profile listing a device, for another app).
made_up_profile() {
  local kind="$1" out="$2" dir
  dir="$(dirname "$out")"
  mkdir -p "$dir"
  KIND="$kind" TEAM="$MU_TEAM" BUNDLE="$BUNDLE" python3 -c '
import datetime, os, plistlib, sys, uuid
team, bundle, store = os.environ["TEAM"], os.environ["BUNDLE"], os.environ["KIND"] == "store"
p = {
    "AppIDName": "RATA rehearsal",
    "Name": "Made up for the rehearsal, by nobody at Apple",
    "TeamIdentifier": [team],
    "UUID": str(uuid.uuid4()),
    "ExpirationDate": datetime.datetime(2030, 1, 1),
    "Entitlements": {
        "application-identifier": team + "." + (bundle if store else "org.example.other"),
        "com.apple.developer.team-identifier": team,
        "get-task-allow": not store,
    },
}
if not store:
    p["ProvisionedDevices"] = ["00008110-000A1B2C3D4E5F6A"]
with open(sys.argv[1], "wb") as fh:
    plistlib.dump(p, fh)
' "$dir/$kind.plist"
  [ -f "$dir/signer.pem" ] || openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=RATA rehearsal, not Apple" \
    -keyout "$dir/signer.key" -out "$dir/signer.pem" 2>/dev/null
  openssl smime -sign -binary -nodetach -outform DER -in "$dir/$kind.plist" \
    -signer "$dir/signer.pem" -inkey "$dir/signer.key" -out "$out"
}

# The .ipa at $1 with the profile at $2 inside its app, as $3.
with_profile() {
  local d="$WORK/repack" app
  rm -rf "$d" && mkdir -p "$d"
  ditto -x -k "$1" "$d"
  app="$(find "$d/Payload" -maxdepth 1 -type d -name '*.app' | head -n 1)"
  cp "$2" "$app/embedded.mobileprovision"
  rm -f "$3"
  (cd "$d" && ditto -c -k --keepParent Payload "$3")
  rm -rf "$d"
}

# inspect, on GitHub's Macs, against the unsigned build: as it is (no
# signature, no profile), and with made-up profiles of both kinds, so that
# each of its macOS commands (ditto, codesign, security cms, plutil) runs
# on a real app and each finding it can make is seen. Every run must fail,
# for exactly the reasons the build has.
rehearse_inspect() {
  local unsigned="$WORK/unsigned/RATA.ipa" dir="$WORK/made-up-profiles" bad=0 status log run ipa allowed line
  local not_signed="::error::The app is not signed with an Apple Distribution certificate." \
    no_seal="::error::The app's signature does not verify." \
    no_profile="::error::The signed app has no provisioning profile" \
    debug="::error::The provisioning profile allows debugging" \
    devices="::error::The provisioning profile lists devices" \
    other_app="::error::The provisioning profile is not for"
  [ -f "$unsigned" ] || die "rehearse inspect needs the unsigned .ipa archive makes"
  load
  made_up
  rm -rf "$dir"
  made_up_profile store "$dir/store.mobileprovision"
  made_up_profile development "$dir/development.mobileprovision"
  with_profile "$unsigned" "$dir/store.mobileprovision" "$WORK/with-store-profile.ipa"
  with_profile "$unsigned" "$dir/development.mobileprovision" "$WORK/with-development-profile.ipa"
  for run in as-built store development; do
    log="$WORK/rehearsal-inspect-$run.log"
    ipa="$unsigned"
    [ "$run" = as-built ] || ipa="$WORK/with-$run-profile.ipa"
    set +e
    (export IPA="$ipa" && exec bash "$self" inspect) > "$log" 2>&1
    status=$?
    set -e
    show "$log" "inspect, the unsigned build, $run"
    [ "$status" -eq 1 ] || { echo "::error::inspect must refuse the unsigned build ($run), and it ended $status."; bad=1; }
    # What the archive step found, it finds again in the unpacked .ipa.
    grep -qF "The licence's public key is compiled in." "$log" || { echo "::error::inspect did not judge the app in the .ipa ($run) as the archive step does."; bad=1; }
    grep -qxF "$not_signed" "$log" || { echo "::error::inspect must say the unsigned build ($run) is not signed with a distribution certificate."; bad=1; }
    # Each ::error:: line must be one this run explains.
    allowed="$not_signed|$no_seal"
    case "$run" in
      as-built)
        grep -qF "$no_profile" "$log" || { echo "::error::inspect must say the unsigned build has no provisioning profile."; bad=1; }
        allowed="$allowed|$no_profile"
        ;;
      store)
        grep -qxF "The provisioning profile is a distribution one (get-task-allow false)." "$log" \
          && grep -qxF "The profile is for $BUNDLE." "$log" \
          || { echo "::error::inspect must read an App Store profile as one, for $BUNDLE."; bad=1; }
        ;;
      development)
        for line in "$debug" "$devices" "$other_app"; do
          grep -qF "$line" "$log" || { echo "::error::inspect must find in a development profile: ${line#::error::}"; bad=1; }
        done
        allowed="$allowed|$debug|$devices|$other_app"
        ;;
    esac
    if grep '^::error::' "$log" | grep -vF "$(printf '%s\n' "$allowed" | tr '|' '\n')" | grep -q .; then
      echo "::error::inspect said something of the unsigned build ($run) that is not so:"
      grep '^::error::' "$log" | grep -vF "$(printf '%s\n' "$allowed" | tr '|' '\n')" | sed 's/^::error::/  | /'
      bad=1
    fi
    none_of_them_in "$log" || bad=1
  done
  rm -rf "$dir" "$WORK"/with-*-profile.ipa
  [ "$bad" -eq 0 ] || exit 1
  echo "inspect refused the unsigned build, as built and with made-up profiles of both kinds, for exactly the reasons it has, and printed none of the values."
}

# --- The build ---------------------------------------------------------------

xcode() {
  local xcode version sdk
  xcode="$(python3 "$here/newest-xcode.py")"
  sudo xcode-select -s "$xcode/Contents/Developer"
  xcodebuild -version
  version="$(xcodebuild -version | sed -n 's/^Xcode \([0-9][0-9.]*\).*/\1/p' | head -n 1)"
  sdk="$(xcrun --sdk iphoneos --show-sdk-version)"
  echo "iOS SDK $sdk"
  case "${version%%.*}" in '' | *[!0-9]*) die "Could not read the Xcode version from xcodebuild -version." ;; esac
  case "${sdk%%.*}" in '' | *[!0-9]*) die "Could not read the iOS SDK version." ;; esac
  if [ "${version%%.*}" -lt "$XCODE_MIN" ] || [ "${sdk%%.*}" -lt "$XCODE_MIN" ]; then
    die "App Store Connect takes only apps built with Xcode $XCODE_MIN or later and its iOS $XCODE_MIN SDK, and the newest Xcode this runner can use is $version (iOS SDK $sdk). That is GitHub's macOS image, not RATA: wait until macos-latest carries Xcode $XCODE_MIN, or name an image that does in runs-on."
  fi
}

# The licence's public key, for licence.rs's option_env!. Xcode compiles the
# Rust code through `tauri ios xcode-script`, and the Tauri CLI hands that
# build only TAURI*, WRY*, CARGO_*, RUST_*, PATH and TMPDIR, so the variable
# in the environment alone never arrives (ios.yml does the same for its
# throwaway key). Cargo's own [env], in the runner's Cargo home, does.
licence_key() {
  local config="${CARGO_HOME:-$HOME/.cargo}/config.toml"
  [ -n "${RATA_LICENCE_PUBLIC_KEY:-}" ] || die "RATA_LICENCE_PUBLIC_KEY is not set: a build without it refuses every licence."
  ok_licence "$RATA_LICENCE_PUBLIC_KEY" || die "RATA_LICENCE_PUBLIC_KEY does not look like a PEM public key."
  if grep -qs '^\[env\]' "$config"; then die "$config already has an [env] table"; fi
  mkdir -p "$(dirname "$config")"
  # A JSON string is a TOML basic string too.
  python3 -c 'import json, os; print("\n[env]\nRATA_LICENCE_PUBLIC_KEY = " + json.dumps(os.environ["RATA_LICENCE_PUBLIC_KEY"]))' >> "$config"
  echo "The licence's public key is handed to the Rust build."
}

version() {
  python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$TAURI_DIR/tauri.conf.json"
}

# CFBundleVersion: this workflow's run number, so every upload is new (App
# Store Connect refuses a number it has seen for the same version). A plain
# integer, since Apple takes at most three numbers there and the Tauri CLI's
# --build-number would append a fourth to the version.
build_number() {
  local n="${RATA_BUILD_NUMBER:-${GITHUB_RUN_NUMBER:-}}" re='^[1-9][0-9]{0,8}$'
  [[ $n =~ $re ]] || die "The build number must be a whole number above 0 (RATA_BUILD_NUMBER, or GitHub's run number), not \"$n\"."
  printf '%s' "$n"
}

# What the store's build differs in from the committed config, for Tauri's
# --config: the bundle id and the build number. The marketing version stays
# tauri.conf.json's.
write_override() {
  load
  ok_bundle "$BUNDLE" || die "RATA_IOS_BUNDLE_ID \"$BUNDLE\" is not a bundle id."
  local n
  n="$(build_number)"
  mkdir -p "$WORK"
  BUNDLE="$BUNDLE" N="$n" python3 -c 'import json, os; print(json.dumps({"identifier": os.environ["BUNDLE"], "bundle": {"iOS": {"bundleVersion": os.environ["N"]}}}))' > "$OVERRIDE"
  echo "$OVERRIDE: $(cat "$OVERRIDE")"
}

project() {
  write_override
  (cd "$TAURI_DIR" && cargo tauri ios init --ci --skip-targets-install --config "$OVERRIDE")
  bash "$here/prepare.sh"
  export_options
}

# The export's options. The Tauri CLI merges gen/apple/ExportOptions.plist
# into what it gives xcodebuild -exportArchive, and neither says
# manageAppVersionAndBuildNumber, which for App Store Connect defaults to
# YES: Xcode asks App Store Connect for the build numbers it holds and may
# raise CFBundleVersion past this run's number (on a re-run, or after a
# build uploaded by hand). Then inspect would find a number this run did not
# give, rather than Apple refusing a number it has, which explain() says
# how to fix. So the run's number is what ships, or Apple says why not.
export_options() {
  local plist="$TAURI_DIR/gen/apple/ExportOptions.plist"
  [ -f "$plist" ] || die "tauri ios init made no gen/apple/ExportOptions.plist, which the Tauri CLI merges into the export's options."
  python3 -c '
import plistlib, sys
with open(sys.argv[1], "rb") as fh:
    options = plistlib.load(fh)
options["manageAppVersionAndBuildNumber"] = False
with open(sys.argv[1], "wb") as fh:
    plistlib.dump(options, fh)
' "$plist"
  echo "gen/apple/ExportOptions.plist: manageAppVersionAndBuildNumber is NO, so Xcode keeps this run's build number."
}

# What every iOS build of RATA for the store must be, signed or not.
judge() {
  local app="$1" n v got bad=0 b64
  n="$(build_number)"
  v="$(version)"
  load
  bash "$here/check-app.sh" "$app" "$BUNDLE" || bad=1
  got="$(plutil -extract CFBundleVersion raw -o - "$app/Info.plist" 2>/dev/null || true)"
  echo "CFBundleVersion = $got"
  [ "$got" = "$n" ] || { echo "::error::CFBundleVersion is \"$got\", not this run's number $n"; bad=1; }
  got="$(plutil -extract CFBundleShortVersionString raw -o - "$app/Info.plist" 2>/dev/null || true)"
  echo "CFBundleShortVersionString = $got"
  [ "$got" = "$v" ] || { echo "::error::CFBundleShortVersionString is \"$got\", not tauri.conf.json's $v"; bad=1; }
  # The key's own line of base64, as licence.rs holds the PEM.
  b64="$(printf '%s\n' "${RATA_LICENCE_PUBLIC_KEY:-}" | sed -n 2p)"
  if [ -n "$b64" ] && grep -qaF -- "$b64" "$app/RATA"; then
    echo "The licence's public key is compiled in."
  else
    echo "::error::The app has no licence key compiled in: it would refuse every licence."
    bad=1
  fi
  bash "$here/../harness/verify-release.sh" --no-updater "$app/RATA" || bad=1
  bash "$here/../harness/verify-release.sh" --no-smoke "$app/RATA" || bad=1
  return "$bad"
}

archive() {
  local found arc ipa
  if grep -qs "distribution: 'store'" "$here/../ui/config.js"; then
    echo "The interface is the store build's (sync-ui.sh, RATA_DISTRIBUTION=store)."
  else
    echo "The interface is the direct build's: this branch's sync-ui.sh has no store build yet."
  fi
  rm -rf "$TAURI_DIR/gen/apple/build"
  (cd "$TAURI_DIR" && cargo tauri ios build --target aarch64 --no-sign --ci --config "$OVERRIDE" -- --no-default-features)
  found="$(find "$TAURI_DIR/gen/apple/build" -maxdepth 1 -type d -name '*.xcarchive' | head -n 1)"
  [ -n "$found" ] || die "no .xcarchive under gen/apple/build"
  arc="$found/Products/Applications/RATA.app"
  [ -d "$arc" ] || die "no RATA.app in $found"
  ipa="$(find "$TAURI_DIR/gen/apple/build/arm64" -maxdepth 1 -type f -name '*.ipa' | head -n 1)"
  [ -n "$ipa" ] || die "no .ipa under gen/apple/build/arm64"
  mkdir -p "$WORK/unsigned"
  mv "$ipa" "$WORK/unsigned/RATA.ipa"
  echo "Archived for a generic iOS device: $found"
  lipo -info "$arc/RATA"
  judge "$arc"
}

# The .p8, in a folder only this job's user can read, under the name altool
# looks for. Its contents never pass through a command line or the log.
write_key() {
  KEY_FILE="$ASC/private_keys/AuthKey_$KEY_ID.p8"
  (umask 077 && mkdir -p "$ASC/private_keys")
  chmod 700 "$ASC" "$ASC/private_keys"
  (umask 077 && printf '%s\n' "$APPLE_API_KEY_P8" | tr -d '\r' > "$KEY_FILE")
  chmod 600 "$KEY_FILE"
}

# Replaces, in what a tool prints, the three ids and any line of the key with
# a label. GitHub hides its secrets in the log already; this keeps them out
# of the copies saved here too, and holds for the rehearsal's made-up values,
# which are no secrets of GitHub's.
redact() {
  perl -e '
    $| = 1;
    my %label = (APPLE_API_KEY => "[key id]", APPLE_API_ISSUER => "[issuer id]", APPLE_DEVELOPMENT_TEAM => "[team id]");
    my @subs;
    for my $name (sort keys %label) {
      my $v = $ENV{$name} // "";
      $v =~ s/^\s+|\s+$//g;
      push @subs, [qr/\Q$v\E/i, $label{$name}] if length $v >= 4;
    }
    if (my $path = $ENV{APPLE_API_KEY_PATH}) {
      if (open my $fh, "<", $path) {
        while (my $l = <$fh>) {
          $l =~ s/\s+//g;
          next if $l =~ /^-----/ || length $l < 16;
          push @subs, [qr/\Q$l\E/, "[private key]"];
        }
      }
    }
    while (my $line = <STDIN>) {
      $line =~ s/$_->[0]/$_->[1]/g for @subs;
      print $line;
    }
  '
}

# --- Saying what to fix ------------------------------------------------------

KEY_REFUSED="Apple did not accept the App Store Connect API key."
NO_ANSWER="Apple's servers did not answer."
explain() {
  local what="${1:?sign or upload}" log="${2:?a log}" said=0 v
  load
  v="$(version 2>/dev/null || echo "this version")"
  hit() { grep -qiE -- "$1" "$log"; }
  tell() { echo "::error::$*"; said=1; }
  # The causes that explain everything else come first, and each is said
  # alone. When Apple refuses the key, an agreement is waiting or the key's
  # role is too low, every later request Xcode and altool make fails too, in
  # words that point elsewhere: "No profiles for …", "Cannot determine the
  # Apple ID from Bundle ID", and Xcode's "Communication with Apple failed",
  # which it says for a 401 as much as for no answer (the made-up rehearsal
  # gets all three). Naming those as well would send the owner to fix what is
  # not broken.
  if hit 'agreement|PLA Update|have not (yet )?agreed|required contract'; then
    tell "An Apple agreement is waiting to be accepted. The Account Holder signs in at developer.apple.com/account and accepts the latest Apple Developer Program License Agreement, and in App Store Connect, Business, the Paid Apps agreement. Then run this job again."
    return 0
  fi
  if hit "NOT_AUTHORIZED|Authentication credentials are missing or invalid|Authentication failed|Failed to authenticate|Unable to authenticate|Failed to log in|unauthori[sz]ed|(status|HTTP|code)[^0-9]{0,4}401|invalid (api )?key|Could not (find|load)[^.]*AuthKey|private key[^.]*(invalid|could not|not found)"; then
    tell "$KEY_REFUSED APPLE_API_KEY_ID, APPLE_API_ISSUER_ID and APPLE_API_KEY_P8 must be the same key's: its Key ID, the Issuer ID above the keys table, and the .p8 file downloaded when it was made (App Store Connect, Users and Access > Integrations). A revoked key is refused too: make a new one and replace all three. Any other error above follows from this one."
    return 0
  fi
  if hit 'Cloud signing permission error|cloud-managed distribution certificate|No signing certificate'; then
    tell "The API key may not sign in Apple's cloud. xcodebuild signs with a cloud-managed distribution certificate, and Apple lets an API key use one only when the key has the Admin role: a key with App Manager is told \"Cloud signing permission error\". A key's role cannot be changed, so make a new key with the Admin role in App Store Connect, Users and Access > Integrations, and replace APPLE_API_KEY_ID and APPLE_API_KEY_P8."
    return 0
  fi
  if hit 'FORBIDDEN|forbidden for security reasons|not allowed to perform this operation|does not have (the )?(required )?(role|permission|access)|insufficient (privileges|permissions)'; then
    tell "Apple refused what the API key asked for, because the key's role is too low. Uploading needs App Manager or Admin, and signing in Apple's cloud needs Admin. Make a new key with that role in App Store Connect, Users and Access > Integrations, and replace APPLE_API_KEY_ID and APPLE_API_KEY_P8."
    return 0
  fi
  if hit 'timed out|network connection was lost|could not connect to|Internet connection appears to be offline'; then
    tell "$NO_ANSWER Nothing needs fixing in RATA or its secrets: run this job again in a while (developer.apple.com/system-status says whether Apple knows)."
    return 0
  fi
  if [ "$what" = sign ] && hit "No App ID|No profiles for|An App ID with Identifier|(App ID|identifier)[^.]{0,80}(is not available|cannot be registered|unavailable)"; then
    tell "The bundle id $BUNDLE is not registered to this team. In Certificates, Identifiers & Profiles > Identifiers, register an explicit App ID with exactly that bundle id, make the App Store Connect app record with it, and set the repository variable RATA_IOS_BUNDLE_ID to it."
  fi
  if [ "$what" = sign ] && hit 'No Account for Team|No team (was )?found|team[^.]*(does not exist|not found)'; then
    tell "APPLE_TEAM_ID is not a team this API key can act for. It is the 10-character Team ID under Membership details at developer.apple.com/account, of the team the key was made in."
  fi
  if [ "$what" = upload ] && hit 'No suitable application records|Cannot determine the Apple ID from Bundle ID|could not find (an|the) app|no app (record )?(was )?found'; then
    tell "App Store Connect has no app with the bundle id $BUNDLE. Make one in App Store Connect, Apps, + > New App, with that bundle id (registered first as an App ID), or set the repository variable RATA_IOS_BUNDLE_ID to the bundle id of the app record that exists."
  fi
  if [ "$what" = upload ] && hit 'bundle version must be higher|has already been used|Redundant Binary Upload|ITMS-90189|ITMS-4238'; then
    tell "App Store Connect already has a build with this number. The number is this workflow's run number, so re-running an earlier run repeats it: start a new run instead (a merge to main, or Run workflow on the Actions tab)."
  fi
  if [ "$what" = upload ] && hit 'ITMS-90062|ITMS-90186|train[^.]*closed|must contain a higher version'; then
    tell "App Store Connect takes no more builds of version $v: it was approved or released. Raise the version (tauri.conf.json and the other places CLAUDE.md lists) and merge."
  fi
  # Xcode's word for any refusal from Apple's developer portal, and for no
  # answer, so it says which only when nothing above did.
  if [ "$said" -eq 0 ] && hit 'Communication with Apple failed'; then
    tell "Xcode could not get what it needed from Apple (\"Communication with Apple failed\"), which Xcode says when Apple refuses a request as well as when it does not answer. Check that the agreements are accepted at developer.apple.com/account and that the API key has the Admin role; if both are so, run this job again in a while."
  fi
  if [ "$what" = upload ] && [ "$said" -eq 0 ] && hit 'ITMS-[0-9]+'; then
    tell "App Store Connect refused the build. The ITMS code above says why; Apple lists them at developer.apple.com/help/app-store-connect."
  fi
  if [ "$said" -eq 0 ]; then
    if [ "$what" = sign ]; then
      tell "Signing failed for a reason this job does not recognise. The lines above are xcodebuild's own: the first one with \"error:\" is what Xcode or Apple answered."
    else
      tell "The upload failed for a reason this job does not recognise. The lines above are altool's own."
    fi
  fi
}

# --- Signing and uploading ---------------------------------------------------

sign() {
  local n v status log="$WORK/sign.log" ipa
  load
  [ -n "$KEY_ID" ] && [ -n "$ISSUER" ] && [ -n "$TEAM" ] && [ -n "${APPLE_API_KEY_P8:-}" ] \
    || die "sign needs all four of $SECRETS"
  [ -f "$OVERRIDE" ] || die "sign runs after project"
  n="$(build_number)"
  v="$(version)"
  write_key
  # From here on only the file holds the key, and the ids reach the Tauri
  # CLI under the names its documentation gives (v2.tauri.app/distribute/
  # sign/ios: APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_PATH), which
  # it passes to xcodebuild as -authenticationKeyID, -authenticationKeyIssuerID
  # and -authenticationKeyPath with -allowProvisioningUpdates. The team is
  # APPLE_DEVELOPMENT_TEAM, which the CLI writes into the project.
  unset APPLE_API_KEY_P8 APPLE_API_KEY_ID APPLE_API_ISSUER_ID APPLE_TEAM_ID
  export APPLE_API_KEY="$KEY_ID" APPLE_API_ISSUER="$ISSUER" APPLE_API_KEY_PATH="$KEY_FILE" \
    APPLE_DEVELOPMENT_TEAM="$TEAM"
  # The unsigned build's .ipa must not be taken for this one.
  rm -f "$TAURI_DIR"/gen/apple/build/arm64/*.ipa
  set +e
  (cd "$TAURI_DIR" && cargo tauri ios build --target aarch64 --export-method app-store-connect --ci \
    --config "$OVERRIDE" -- --no-default-features) 2>&1 | redact | tee "$log"
  status="${PIPESTATUS[0]}"
  set -e
  if [ "$status" -ne 0 ]; then
    explain sign "$log"
    return 1
  fi
  ipa="$(find "$TAURI_DIR/gen/apple/build/arm64" -maxdepth 1 -type f -name '*.ipa' 2>/dev/null | head -n 1)"
  [ -n "$ipa" ] || die "The build said it was signed, but there is no .ipa under gen/apple/build/arm64."
  mkdir -p "$WORK/out"
  cp "$ipa" "$WORK/out/RATA-$v-$n.ipa"
  echo "IPA=$WORK/out/RATA-$v-$n.ipa" >> "${GITHUB_ENV:-/dev/null}"
  echo "Signed for App Store Connect: RATA-$v-$n.ipa"
}

inspect() {
  local ipa="${IPA:-}" dir="$WORK/signed" app authority profile="$WORK/profile.plist" bad=0
  [ -f "$ipa" ] || die "no signed .ipa to inspect"
  load
  rm -rf "$dir" && mkdir -p "$dir"
  ditto -x -k "$ipa" "$dir" || die "The signed .ipa could not be unpacked: it is not the zip an .ipa is. The signing step made it; read that step's log."
  app="$(find "$dir/Payload" -maxdepth 1 -type d -name '*.app' 2>/dev/null | head -n 1)"
  [ -n "$app" ] || die "The signed .ipa holds no app under Payload/. The signing step made it; read that step's log."
  judge "$app" || bad=1
  if codesign --verify --deep --strict "$app"; then echo "The signature verifies."; else echo "::error::The app's signature does not verify."; bad=1; fi
  # Printed as the kind of certificate only: the line also names the team.
  # An app signed by nobody makes codesign fail, which is an answer here.
  authority="$(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -n 1 || true)"
  case "$authority" in
    "Apple Distribution"* | "iPhone Distribution"*) echo "Signed with a distribution certificate." ;;
    *) echo "::error::The app is not signed with an Apple Distribution certificate."; bad=1 ;;
  esac
  if [ ! -f "$app/embedded.mobileprovision" ]; then
    echo "::error::The signed app has no provisioning profile (embedded.mobileprovision), so it was not signed for the App Store and App Store Connect would refuse it. The signing step made it; read that step's log."
    return 1
  fi
  if ! security cms -D -i "$app/embedded.mobileprovision" > "$profile"; then
    echo "::error::The app's provisioning profile could not be read (security cms -D): it is not a signed profile. The signing step made it; read that step's log."
    return 1
  fi
  if [ "$(plutil -extract Entitlements.get-task-allow raw -o - "$profile" 2>/dev/null)" = false ]; then
    echo "The provisioning profile is a distribution one (get-task-allow false)."
  else
    echo "::error::The provisioning profile allows debugging: it is not an App Store one."; bad=1
  fi
  if plutil -extract ProvisionedDevices xml1 -o - "$profile" >/dev/null 2>&1; then
    echo "::error::The provisioning profile lists devices: it is an ad hoc or development one, not an App Store one."; bad=1
  fi
  case "$(plutil -extract Entitlements.application-identifier raw -o - "$profile" 2>/dev/null)" in
    *".$BUNDLE") echo "The profile is for $BUNDLE." ;;
    *) echo "::error::The provisioning profile is not for $BUNDLE."; bad=1 ;;
  esac
  return "$bad"
}

upload() {
  local ipa="${IPA:-}" status log="$WORK/upload.log" key n v
  load
  [ -n "$KEY_ID" ] && [ -n "$ISSUER" ] || die "upload needs APPLE_API_KEY_ID and APPLE_API_ISSUER_ID"
  [ -f "$ipa" ] || die "no .ipa to upload"
  key="$ASC/private_keys/AuthKey_$KEY_ID.p8"
  [ -f "$key" ] || die "no key file: the signing step writes it"
  export APPLE_API_KEY="$KEY_ID" APPLE_API_ISSUER="$ISSUER" APPLE_API_KEY_PATH="$key"
  [ -z "$TEAM" ] || export APPLE_DEVELOPMENT_TEAM="$TEAM"
  set +e
  (cd "$ASC" && API_PRIVATE_KEYS_DIR="$ASC/private_keys" xcrun altool --upload-app --type ios \
    --file "$ipa" --apiKey "$KEY_ID" --apiIssuer "$ISSUER") 2>&1 | redact | tee "$log"
  status="${PIPESTATUS[0]}"
  set -e
  # altool has said FAILED with a status of 0 before now, so its words count
  # as much as its status.
  if [ "$status" -ne 0 ] || grep -qiE 'UPLOAD FAILED|ERROR ITMS-|\*\*\* Error' "$log"; then
    explain upload "$log"
    return 1
  fi
  n="$(build_number)"
  v="$(version)"
  echo "::notice::RATA $v ($n) is uploaded to App Store Connect as $BUNDLE. Apple processes it, usually within half an hour, and then it is in TestFlight for the internal testers ($SETUP)."
}

forget() {
  rm -rf "$ASC" "$WORK/made-up"
  echo "No App Store Connect key is left on this runner."
}

case "${1:-}" in
  check) check ;;
  rehearse) shift; rehearse ${1+"$@"} ;;
  xcode) xcode ;;
  licence-key) licence_key ;;
  override) write_override ;;
  project) project ;;
  export-options) export_options ;;
  archive) archive ;;
  sign) sign ;;
  inspect) inspect ;;
  upload) upload ;;
  forget) forget ;;
  redact) redact ;;
  explain) shift; explain ${1+"$@"} ;;
  *)
    echo "usage: testflight.sh check|rehearse CASE|xcode|licence-key|override|project|export-options|archive|sign|inspect|upload|forget|redact|explain sign|upload LOG" >&2
    exit 2
    ;;
esac
