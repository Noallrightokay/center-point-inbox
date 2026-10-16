#!/usr/bin/env bash
# testflight-test.sh — the tests of testflight.sh: the configuration check in
# every case, that no value is ever printed, the build's --config, the
# signing and upload steps against stand-in `cargo` and `xcrun` (what they
# are given, the key file's permissions, the words a failure ends with),
# `redact`, `explain` and `forget`. Runs on Linux and macOS with bash,
# python3, perl and openssl; nothing reaches Apple. Exit 1 on any failure.
# The workflow runs it on GitHub's Macs (ios-testflight.yml, the made-up
# rehearsal).
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$here/testflight.sh"
# Its real path: macOS's $TMPDIR ends in a slash and lies under /var, a link
# to /private/var, so the path as made is not the one a tool finds itself in.
tmp="$(mktemp -d "${TMPDIR:-/tmp}/rata-testflight-test.XXXXXX")"
tmp="$(cd "$tmp" && pwd -P)"
trap 'rm -rf "$tmp"' EXIT
failures=0
fail() { echo "FAIL: $*"; failures=$((failures + 1)); }
pass() { echo "ok: $*"; }

# Made-up values of the right shapes.
KEY_ID="ABCDE12345"
ISSUER="0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"
TEAM="TEAM987654"
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/ec.pem" 2>/dev/null
P8="$(openssl pkcs8 -topk8 -nocrypt -in "$tmp/ec.pem")"
P8_LINE="$(printf '%s\n' "$P8" | sed -n 2p)"
openssl genpkey -algorithm ed25519 -out "$tmp/licence.key" 2>/dev/null \
  || openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/licence.key" 2>/dev/null
LICENCE="$(openssl pkey -in "$tmp/licence.key" -pubout 2>/dev/null || openssl ec -in "$tmp/licence.key" -pubout 2>/dev/null)"

# A stand-in shell project: the version Tauri reads, and the folder the
# Tauri CLI writes its .ipa to.
TAURI="$tmp/src-tauri"
mkdir -p "$TAURI"
printf '{"productName": "RATA", "version": "0.1.99", "identifier": "org.mailrata.desktop"}\n' > "$TAURI/tauri.conf.json"

# Stand-ins for cargo and xcrun: each writes down how it was called, from
# where, and which of the Apple variables it was given, then says what
# STUB_SAY says and ends with STUB_STATUS.
STUBS="$tmp/stubs"
mkdir -p "$STUBS"
for tool in cargo xcrun; do
  cat > "$STUBS/$tool" <<'STUB'
#!/usr/bin/env bash
seen="$STUB_DIR/$(basename "$0").seen"
{
  echo "args: $*"
  echo "cwd: $(pwd -P)"
  for v in APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_DEVELOPMENT_TEAM \
    APPLE_API_KEY_ID APPLE_API_ISSUER_ID APPLE_API_KEY_P8 APPLE_TEAM_ID API_PRIVATE_KEYS_DIR; do
    if printenv "$v" > /dev/null; then echo "env $v=$(printenv "$v")"; fi
  done
  if [ -n "${APPLE_API_KEY_PATH:-}" ] && [ -f "$APPLE_API_KEY_PATH" ]; then
    python3 -c 'import os, sys; p = sys.argv[1]; print("key mode", oct(os.stat(p).st_mode & 0o777), "folder mode", oct(os.stat(os.path.dirname(p)).st_mode & 0o777), "parent mode", oct(os.stat(os.path.dirname(os.path.dirname(p))).st_mode & 0o777))' "$APPLE_API_KEY_PATH"
    echo "key: $(cat "$APPLE_API_KEY_PATH" | tr '\n' '|')"
  fi
} > "$seen"
if [ -n "${STUB_IPA:-}" ]; then mkdir -p "$(dirname "$STUB_IPA")" && echo signed > "$STUB_IPA"; fi
printf '%b' "${STUB_SAY:-}"
exit "${STUB_STATUS:-0}"
STUB
  chmod +x "$STUBS/$tool"
done

# Runs testflight.sh with nothing in its environment but what is named, so
# no value of this machine's can stand in for a missing one. ${1+"$@"}, not
# "$@": the bash 3.2 macOS runs these with can call an empty "$@" unbound
# under set -u.
run() {
  local out="$tmp/out" env_file="$tmp/env"
  : > "$out"
  : > "$env_file"
  env -i PATH="$STUBS:$PATH" HOME="$tmp/home" RUNNER_TEMP="$tmp/runner" \
    GITHUB_OUTPUT="$out" GITHUB_ENV="$env_file" RATA_TAURI_DIR="$TAURI" STUB_DIR="$STUBS" \
    RATA_BUILD_NUMBER=42 ${1+"$@"} bash "$script" "${ARGS[@]}" > "$tmp/log" 2>&1
}
all_four=(APPLE_API_KEY_ID="$KEY_ID" APPLE_API_ISSUER_ID="$ISSUER" APPLE_API_KEY_P8="$P8" APPLE_TEAM_ID="$TEAM")

# None of the made-up values, in any case, in the files named.
no_values() {
  local f v leaked=""
  for f in "$@"; do
    [ -f "$f" ] || continue
    for v in "$KEY_ID" "$ISSUER" "$TEAM" "$P8_LINE"; do
      grep -qiF -- "$v" "$f" && leaked="$leaked $(basename "$f")"
    done
  done
  [ -z "$leaked" ]
}

check_case() { # name, expected status, expected build output, then env
  local name="$1" want="$2" build="$3" status
  shift 3
  ARGS=(check)
  run ${1+"$@"}
  status=$?
  if [ "$status" -ne "$want" ]; then
    fail "check, $name: status $status, not $want"
    sed 's/^/    /' "$tmp/log"
  elif ! grep -qx "build=$build" "$tmp/out"; then
    fail "check, $name: no build=$build"
  elif ! no_values "$tmp/log" "$tmp/out" "$tmp/env"; then
    fail "check, $name: printed a value"
  else
    pass "check, $name"
  fi
}

# --- The configuration check -------------------------------------------------

check_case "nothing set" 0 false RATA_LICENCE_PUBLIC_KEY="$LICENCE"
grep -qxF "::notice::TestFlight is not set up yet: docs/APP-STORES.md" "$tmp/log" \
  && pass "nothing set says so, in the words docs/ is written for" \
  || fail "nothing set must say \"TestFlight is not set up yet: docs/APP-STORES.md\""
grep -qx "bundle_id=org.mailrata.app" "$tmp/out" && pass "the bundle id is org.mailrata.app when RATA_IOS_BUNDLE_ID is unset" \
  || fail "the default bundle id is org.mailrata.app"

check_case "nothing set, no licence key either" 0 false
check_case "blank values count as unset" 0 false APPLE_API_KEY_ID="  " APPLE_API_KEY_P8=$'\n'

for missing in APPLE_API_KEY_ID APPLE_API_ISSUER_ID APPLE_API_KEY_P8 APPLE_TEAM_ID; do
  given=()
  for kv in "${all_four[@]}"; do
    [ "${kv%%=*}" = "$missing" ] || given+=("$kv")
  done
  check_case "all but $missing" 1 false RATA_LICENCE_PUBLIC_KEY="$LICENCE" "${given[@]}"
  if grep -qF "::error::TestFlight is half set up. Missing: $missing. " "$tmp/log"; then
    pass "all but $missing names $missing alone"
  else
    fail "all but $missing must name $missing alone"
    sed 's/^/    /' "$tmp/log"
  fi
done

check_case "two of four" 1 false APPLE_API_KEY_ID="$KEY_ID" APPLE_API_KEY_P8="$P8"
grep -qF "Missing: APPLE_API_ISSUER_ID, APPLE_TEAM_ID." "$tmp/log" \
  && pass "two of four names the other two, in order" || fail "two of four must name the other two"

check_case "all four" 0 true RATA_LICENCE_PUBLIC_KEY="$LICENCE" "${all_four[@]}"
check_case "all four, with the blank space a paste leaves" 0 true RATA_LICENCE_PUBLIC_KEY="$LICENCE" \
  APPLE_API_KEY_ID=" $KEY_ID"$'\n' APPLE_API_ISSUER_ID="$ISSUER"$'\n' APPLE_API_KEY_P8=$'\r\n'"$P8"$'\r\n' APPLE_TEAM_ID="$TEAM "
check_case "all four, on a pull request" 0 false RATA_LICENCE_PUBLIC_KEY="$LICENCE" RATA_CHECK_ONLY=1 "${all_four[@]}"
grep -qF "On a pull request this job stops after the configuration check" "$tmp/log" \
  && pass "a pull request says it stops after the check" || fail "a pull request must say it stops after the check"
check_case "all four and a bundle id" 0 true RATA_LICENCE_PUBLIC_KEY="$LICENCE" RATA_IOS_BUNDLE_ID=org.example.rata "${all_four[@]}"
grep -qx "bundle_id=org.example.rata" "$tmp/out" && pass "RATA_IOS_BUNDLE_ID is the bundle id" || fail "RATA_IOS_BUNDLE_ID must be the bundle id"

shape() { # name, the message's start, then the one value to change
  local name="$1" says="$2"
  shift 2
  check_case "$name" 1 false RATA_LICENCE_PUBLIC_KEY="$LICENCE" "${all_four[@]}" "$@"
  if grep -qF "::error::$says" "$tmp/log"; then pass "$name says so"; else fail "$name must say \"$says\""; sed 's/^/    /' "$tmp/log"; fi
}
shape "a key id in small letters" "APPLE_API_KEY_ID is not a Key ID." APPLE_API_KEY_ID="abcde12345"
shape "a key id too short" "APPLE_API_KEY_ID is not a Key ID." APPLE_API_KEY_ID="ABCDE1234"
shape "an issuer id that is not one" "APPLE_API_ISSUER_ID is not an Issuer ID." APPLE_API_ISSUER_ID="issuer"
shape "a .p8 that is a public key" "APPLE_API_KEY_P8 is not a .p8 key." APPLE_API_KEY_P8="$LICENCE"
shape "a .p8 cut short" "APPLE_API_KEY_P8 is not a .p8 key." APPLE_API_KEY_P8="${P8%-----END*}"
shape "a team id with a space" "APPLE_TEAM_ID is not a Team ID." APPLE_TEAM_ID="TEAM 87654"
shape "a bundle id with an underscore" "The repository variable RATA_IOS_BUNDLE_ID is \"org.mailrata_app\"" RATA_IOS_BUNDLE_ID=org.mailrata_app
shape "a bundle id of one word" "The repository variable RATA_IOS_BUNDLE_ID is \"rata\"" RATA_IOS_BUNDLE_ID=rata
check_case "all four, no licence key" 1 false "${all_four[@]}"
grep -qF "::error::RATA_LICENCE_PUBLIC_KEY is not set." "$tmp/log" && pass "no licence key says so" || fail "no licence key must say so"
check_case "all four, a licence key that is not one" 1 false RATA_LICENCE_PUBLIC_KEY="key" "${all_four[@]}"
grep -qF "::error::RATA_LICENCE_PUBLIC_KEY does not look like a PEM public key." "$tmp/log" \
  && pass "a licence key that is not one says so" || fail "a licence key that is not one must say so"
check_case "every value wrong at once" 1 false APPLE_API_KEY_ID=a APPLE_API_ISSUER_ID=b APPLE_API_KEY_P8=c APPLE_TEAM_ID=d RATA_IOS_BUNDLE_ID=e
[ "$(grep -c '^::error::' "$tmp/log")" -eq 6 ] && pass "every value wrong at once is said once each" \
  || fail "every value wrong at once must give six errors, not $(grep -c '^::error::' "$tmp/log")"

# --- The build's --config ----------------------------------------------------

ARGS=(override)
run RATA_IOS_BUNDLE_ID=org.example.rata
if python3 -c 'import json, sys; d = json.load(open(sys.argv[1])); assert d == {"identifier": "org.example.rata", "bundle": {"iOS": {"bundleVersion": "42"}}}, d' "$tmp/runner/rata-testflight/override.json"; then
  pass "the build is given the store's bundle id and the run number, and nothing else"
else
  fail "override.json must hold the bundle id and bundleVersion alone"
fi
ARGS=(override)
run RATA_BUILD_NUMBER=0
[ $? -eq 1 ] && pass "a build number of 0 is refused" || fail "a build number of 0 must be refused"
run RATA_BUILD_NUMBER=1.2
[ $? -eq 1 ] && pass "a build number with a dot is refused" || fail "a build number with a dot must be refused"

# --- Signing -----------------------------------------------------------------

ARGS=(override)
run
ARGS=(sign)
run "${all_four[@]}" STUB_STATUS=65 \
  STUB_SAY="Command line invocation: xcodebuild -authenticationKeyID $KEY_ID -authenticationKeyIssuerID $ISSUER DEVELOPMENT_TEAM=$TEAM\nerror: exportArchive: Cloud signing permission error\n"
status=$?
seen="$STUBS/cargo.seen"
[ "$status" -eq 1 ] && pass "signing refused fails the step" || fail "signing refused must fail the step, not $status"
grep -qF "::error::The API key may not sign in Apple's cloud." "$tmp/log" \
  && pass "a cloud signing refusal says the key needs the Admin role" || fail "a cloud signing refusal must say what to fix"
grep -qF -- "-authenticationKeyID [key id] -authenticationKeyIssuerID [issuer id] DEVELOPMENT_TEAM=[team id]" "$tmp/log" \
  && pass "the ids xcodebuild prints are replaced in the log" || fail "the ids xcodebuild prints must be replaced"
no_values "$tmp/log" "$tmp/out" "$tmp/env" "$tmp/runner/rata-testflight/sign.log" \
  && pass "signing prints no value" || fail "signing printed a value"
grep -qx "args: tauri ios build --target aarch64 --export-method app-store-connect --ci --config $tmp/runner/rata-testflight/override.json -- --no-default-features" "$seen" \
  && pass "tauri ios build exports for App Store Connect, without self-update or the smoke self-test" \
  || { fail "tauri ios build was not called as it must be"; cat "$seen"; }
grep -qx "cwd: $TAURI" "$seen" && pass "the Tauri CLI runs in the shell's folder" || fail "the Tauri CLI must run in src-tauri"
keyfile="$tmp/runner/rata-testflight/asc/private_keys/AuthKey_$KEY_ID.p8"
grep -qx "env APPLE_API_KEY=$KEY_ID" "$seen" && grep -qx "env APPLE_API_ISSUER=$ISSUER" "$seen" \
  && grep -qx "env APPLE_API_KEY_PATH=$keyfile" "$seen" && grep -qx "env APPLE_DEVELOPMENT_TEAM=$TEAM" "$seen" \
  && pass "the Tauri CLI is given the key under the names its documentation gives" \
  || { fail "the Tauri CLI was not given APPLE_API_KEY, APPLE_API_ISSUER, APPLE_API_KEY_PATH and APPLE_DEVELOPMENT_TEAM"; cat "$seen"; }
if grep -qE '^env (APPLE_API_KEY_P8|APPLE_API_KEY_ID|APPLE_API_ISSUER_ID|APPLE_TEAM_ID)=' "$seen"; then
  fail "the secrets themselves reached the Tauri CLI's environment"
else
  pass "the .p8 and the secrets' own names are out of the environment the build runs in"
fi
grep -qx "key mode 0o600 folder mode 0o700 parent mode 0o700" "$seen" \
  && pass "the key file is readable by this user alone, in folders only it can open" \
  || { fail "the key file's permissions"; grep '^key mode' "$seen"; }
[ "$(grep '^key: ' "$seen")" = "key: $(printf '%s\n' "$P8" | tr '\n' '|')" ] \
  && pass "the key file holds the .p8 as given" || fail "the key file must hold the .p8"
ARGS=(sign)
run "${all_four[@]}" APPLE_API_KEY_P8=$'\r\n'"${P8//$'\n'/$'\r\n'}"$'\r\n' STUB_STATUS=1
grep -q $'\r' "$keyfile" && fail "a .p8 pasted with Windows line ends keeps its carriage returns" \
  || pass "a .p8 pasted with Windows line ends is written with plain ones"
[ ! -f "$tmp/env" ] || ! grep -q '^IPA=' "$tmp/env" && pass "a failed signing names no .ipa" || fail "a failed signing must name no .ipa"

ARGS=(sign)
run "${all_four[@]}" STUB_IPA="$TAURI/gen/apple/build/arm64/RATA.ipa"
status=$?
[ "$status" -eq 0 ] && pass "a signed build passes" || { fail "a signed build must pass, not $status"; sed 's/^/    /' "$tmp/log"; }
grep -qx "IPA=$tmp/runner/rata-testflight/out/RATA-0.1.99-42.ipa" "$tmp/env" && [ -f "$tmp/runner/rata-testflight/out/RATA-0.1.99-42.ipa" ] \
  && pass "the signed .ipa is kept, named by version and build number" || fail "the signed .ipa must be kept as RATA-0.1.99-42.ipa"

mkdir -p "$TAURI/gen/apple/build/arm64" && echo unsigned > "$TAURI/gen/apple/build/arm64/RATA.ipa"
ARGS=(sign)
run "${all_four[@]}"
[ $? -eq 1 ] && grep -qF "there is no .ipa" "$tmp/log" \
  && pass "an unsigned .ipa left by the archive step is not taken for the signed one" \
  || fail "the archive step's unsigned .ipa must not pass for the signed one"

ARGS=(sign)
run APPLE_API_KEY_ID="$KEY_ID"
[ $? -eq 1 ] && pass "signing with part of the key refuses at once" || fail "signing with part of the key must refuse"

# --- Uploading ---------------------------------------------------------------

ipa="$tmp/runner/rata-testflight/out/RATA-0.1.99-42.ipa"
ARGS=(upload)
run APPLE_API_KEY_ID="$KEY_ID" APPLE_API_ISSUER_ID="$ISSUER" IPA="$ipa" STUB_STATUS=1 \
  STUB_SAY="*** Error: Failed to upload package. NOT_AUTHORIZED Authentication credentials are missing or invalid. (key $KEY_ID)\n"
status=$?
seen="$STUBS/xcrun.seen"
[ "$status" -eq 1 ] && pass "a refused upload fails the step" || fail "a refused upload must fail the step"
grep -qF "::error::Apple did not accept the App Store Connect API key." "$tmp/log" \
  && pass "a refused key says to check the three key secrets" || fail "a refused key must say what to fix"
grep -qx "args: altool --upload-app --type ios --file $ipa --apiKey $KEY_ID --apiIssuer $ISSUER" "$seen" \
  && pass "altool uploads the .ipa with the API key" || { fail "altool was not called as it must be"; cat "$seen"; }
grep -qx "cwd: $tmp/runner/rata-testflight/asc" "$seen" && [ -f "$tmp/runner/rata-testflight/asc/private_keys/AuthKey_$KEY_ID.p8" ] \
  && pass "altool runs where ./private_keys/AuthKey_<Key ID>.p8 is" || fail "altool must run beside private_keys"
grep -qx "env API_PRIVATE_KEYS_DIR=$tmp/runner/rata-testflight/asc/private_keys" "$seen" \
  && pass "altool is told the key's folder too" || fail "API_PRIVATE_KEYS_DIR must name the key's folder"
no_values "$tmp/log" "$tmp/runner/rata-testflight/upload.log" && pass "uploading prints no value" || fail "uploading printed a value"

ARGS=(upload)
run APPLE_API_KEY_ID="$KEY_ID" APPLE_API_ISSUER_ID="$ISSUER" IPA="$ipa" STUB_SAY="UPLOAD FAILED with 1 error\nERROR ITMS-90060: something\n"
[ $? -eq 1 ] && pass "altool saying FAILED with a status of 0 is a failure" || fail "altool saying FAILED must fail the step"
ARGS=(upload)
run APPLE_API_KEY_ID="$KEY_ID" APPLE_API_ISSUER_ID="$ISSUER" IPA="$ipa" STUB_SAY="UPLOAD SUCCEEDED with no errors\n"
[ $? -eq 0 ] && grep -qF "::notice::RATA 0.1.99 (42) is uploaded to App Store Connect as org.mailrata.app." "$tmp/log" \
  && pass "an upload says which version and build went, under which bundle id" || { fail "a good upload must pass and say so"; sed 's/^/    /' "$tmp/log"; }

# --- What a failure says -----------------------------------------------------

explains() { # step, a line from a tool, the start of the sentence it must give
  printf '%s\n' "$2" > "$tmp/said.log"
  ARGS=(explain "$1" "$tmp/said.log")
  run RATA_IOS_BUNDLE_ID=org.example.rata
  if grep -qF "::error::$3" "$tmp/log"; then pass "$1: \"$2\" is explained"; else fail "$1: \"$2\" must be explained as \"$3\""; sed 's/^/    /' "$tmp/log"; fi
}
explains sign 'error: exportArchive: Cloud signing permission error' "The API key may not sign in Apple's cloud."
explains sign 'error: exportArchive: No signing certificate "iOS Distribution" found' "The API key may not sign in Apple's cloud."
explains sign '"code" : "FORBIDDEN_ERROR", "title" : "This request is forbidden for security reasons"' "Apple refused what the API key asked for, because the key's role is too low."
explains sign 'error: exportArchive: PLA Update available: You need to accept the latest Program License Agreement.' "An Apple agreement is waiting to be accepted."
explains sign "error: No profiles for 'org.example.rata' were found" "The bundle id org.example.rata is not registered to this team."
explains sign 'error: No Account for Team "ABCDE12345".' "APPLE_TEAM_ID is not a team this API key can act for."
explains sign 'Error Domain=IDEDistributionErrorDomain Code=10 "Failed to log in."' "Apple did not accept the App Store Connect API key."
explains sign 'error: exportArchive: Communication with Apple failed' "Apple's servers did not answer."
explains sign 'error: linker command failed' "Signing failed for a reason this job does not recognise."
explains upload 'ERROR: A required agreement is missing or has expired.' "An Apple agreement is waiting to be accepted."
explains upload 'No suitable application records were found. Verify your bundle identifier is correct.' "App Store Connect has no app with the bundle id org.example.rata."
explains upload 'ERROR ITMS-90189: "Redundant Binary Upload."' "App Store Connect already has a build with this number."
explains upload 'ERROR ITMS-90186: "Invalid Pre-Release Train. The train version 0.1.99 is closed for new build submissions"' "App Store Connect takes no more builds of version 0.1.99"
explains upload 'ERROR ITMS-90717: "Invalid large app icon."' "App Store Connect refused the build. The ITMS code above says why"
explains upload 'status code 401' "Apple did not accept the App Store Connect API key."
explains upload 'ENTITY_ERROR something odd' "The upload failed for a reason this job does not recognise."

# --- redact and forget -------------------------------------------------------

mkdir -p "$tmp/k" && printf '%s\n' "$P8" > "$tmp/k/AuthKey.p8"
said="$(printf 'id %s issuer %s team %s line %s plain words\n' "$KEY_ID" "$(printf '%s' "$ISSUER" | tr a-f A-F)" "$TEAM" "$P8_LINE" \
  | env -i PATH="$PATH" APPLE_API_KEY="$KEY_ID" APPLE_API_ISSUER="$ISSUER" APPLE_DEVELOPMENT_TEAM="$TEAM" APPLE_API_KEY_PATH="$tmp/k/AuthKey.p8" bash "$script" redact)"
[ "$said" = "id [key id] issuer [issuer id] team [team id] line [private key] plain words" ] \
  && pass "redact replaces each id, in any case, and the key's lines" || fail "redact gave: $said"
said="$(printf 'nothing to hide\n' | env -i PATH="$PATH" bash "$script" redact)"
[ "$said" = "nothing to hide" ] && pass "redact with no values passes text through" || fail "redact with no values gave: $said"

mkdir -p "$tmp/runner/rata-testflight/asc/private_keys" && touch "$tmp/runner/rata-testflight/asc/private_keys/AuthKey_$KEY_ID.p8"
ARGS=(forget)
run
[ ! -e "$tmp/runner/rata-testflight/asc" ] && pass "forget deletes the key" || fail "forget must delete the key"

# --- The rehearsals of the check ---------------------------------------------

for case in none half made-up; do
  ARGS=(rehearse "$case")
  run RATA_LICENCE_PUBLIC_KEY="$LICENCE"
  if [ $? -eq 0 ] && ! grep -q '^::error::' "$tmp/log"; then
    pass "rehearsal $case passes, its own ::error:: lines shown as text"
  else
    fail "rehearsal $case"
    sed 's/^/    /' "$tmp/log"
  fi
done
grep -qx "build=true" "$tmp/out" && pass "the made-up rehearsal goes on to build" || fail "the made-up rehearsal must build"

echo
if [ "$failures" -eq 0 ]; then echo "testflight-test.sh: all passed"; else echo "testflight-test.sh: $failures failed"; fi
[ "$failures" -eq 0 ]
