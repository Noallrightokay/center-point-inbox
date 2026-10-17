#!/usr/bin/env bash
# required-reasons.sh PROGRAM MANIFEST — which of Apple's required-reason
# API categories a built iOS program uses, and whether its privacy manifest
# (PrivacyInfo.xcprivacy) declares exactly those.
#
# Apple asks every app to give a reason for each category it uses
# (developer.apple.com, "Describing use of required reason API"). Used is
# read from the program itself, never from the source:
#   - its undefined symbols (nm -u): the C functions (stat, mach_absolute_time,
#     statfs…), Foundation's constants (NSFileModificationDate…) and the
#     Objective-C classes it links to (NSUserDefaults);
#   - the class and method names it asks for by name at run time (strings),
#     which is how Rust's Objective-C bindings reach a class or a method.
# Give it a release build, as the store gets: a debug build for iOS exports
# every symbol, so its linker keeps each of the thousands of bindings
# objc2's crates generate (UITextInputMode's activeInputModes among them)
# whether anything calls it or not.
# Exit 0 when the manifest declares exactly the categories found, 1 when a
# category is used and not declared, or declared and not used.
set -euo pipefail
program="${1:?give the program inside the app, e.g. RATA.app/RATA}"
manifest="${2:?give its PrivacyInfo.xcprivacy}"
[ -f "$program" ] || { echo "required-reasons.sh: no program at $program" >&2; exit 1; }
[ -f "$manifest" ] || { echo "required-reasons.sh: no manifest at $manifest" >&2; exit 1; }

symbols="$(mktemp)"; names="$(mktemp)"
trap 'rm -f "$symbols" "$names"' EXIT
xcrun nm -u -j "$program" | sort -u > "$symbols"
strings -a "$program" | sort -u > "$names"

python3 - "$symbols" "$names" "$manifest" <<'PY'
import plistlib, sys

symbols = {l.strip() for l in open(sys.argv[1]) if l.strip()}
names = {l.strip() for l in open(sys.argv[2]) if l.strip()}
manifest = plistlib.load(open(sys.argv[3], 'rb'))

# Apple's list, by category: the C functions and Foundation constants as
# the linker names them, then the Objective-C names asked for at run time.
c = lambda *f: ['_' + x for x in f] + ['_' + x + '$INODE64' for x in f]
CATEGORIES = {
    'NSPrivacyAccessedAPICategoryFileTimestamp': (
        c('stat', 'fstat', 'lstat', 'fstatat', 'getattrlist', 'fgetattrlist',
          'getattrlistat', 'getattrlistbulk')
        + ['_NSFileCreationDate', '_NSFileModificationDate',
           '_NSURLContentModificationDateKey', '_NSURLCreationDateKey'],
        ['creationDate', 'modificationDate', 'fileModificationDate',
         'contentModificationDate'],
    ),
    'NSPrivacyAccessedAPICategorySystemBootTime': (
        ['_mach_absolute_time'],
        ['systemUptime'],
    ),
    'NSPrivacyAccessedAPICategoryDiskSpace': (
        c('statfs', 'statvfs', 'fstatfs', 'fstatvfs', 'getattrlist',
          'fgetattrlist', 'getattrlistat')
        + ['_NSFileSystemFreeSize', '_NSFileSystemSize',
           '_NSURLVolumeAvailableCapacityKey',
           '_NSURLVolumeAvailableCapacityForImportantUsageKey',
           '_NSURLVolumeAvailableCapacityForOpportunisticUsageKey',
           '_NSURLVolumeTotalCapacityKey'],
        ['volumeAvailableCapacity', 'volumeAvailableCapacityForImportantUsage',
         'volumeAvailableCapacityForOpportunisticUsage', 'volumeTotalCapacity'],
    ),
    'NSPrivacyAccessedAPICategoryActiveKeyboards': (
        [],
        ['activeInputModes'],
    ),
    'NSPrivacyAccessedAPICategoryUserDefaults': (
        ['_OBJC_CLASS_$_NSUserDefaults'],
        ['NSUserDefaults', 'standardUserDefaults'],
    ),
}

used = {}
for category, (syms, objc) in CATEGORIES.items():
    hits = sorted(s for s in syms if s in symbols) + sorted(n for n in objc if n in names)
    if hits:
        used[category] = hits

declared = {}
for entry in manifest.get('NSPrivacyAccessedAPITypes', []):
    declared[entry.get('NSPrivacyAccessedAPIType')] = entry.get('NSPrivacyAccessedAPITypeReasons', [])

bad = False
for category in CATEGORIES:
    short = category.replace('NSPrivacyAccessedAPICategory', '')
    if category in used and category in declared:
        reasons = declared[category]
        verdict = 'declared ' + ', '.join(reasons) if reasons else 'declared WITHOUT A REASON'
        bad |= not reasons
    elif category in used:
        verdict = 'USED, NOT DECLARED'
        bad = True
    elif category in declared:
        verdict = 'DECLARED, NOT USED'
        bad = True
    else:
        verdict = 'not used'
    print(f'{short}: {verdict}' + (f" (from {', '.join(used[category])})" if category in used else ''))
for category in declared:
    if category not in CATEGORIES:
        print(f'{category}: declared, and not a category this script knows')
        bad = True
for key in ('NSPrivacyTracking', 'NSPrivacyTrackingDomains', 'NSPrivacyCollectedDataTypes'):
    if key not in manifest:
        print(f'{key}: MISSING from the manifest')
        bad = True
if manifest.get('NSPrivacyTracking') is not False or manifest.get('NSPrivacyTrackingDomains'):
    print('the manifest says RATA tracks, which it does not')
    bad = True
print('required reasons: ' + ('FAIL' if bad else 'pass'))
sys.exit(1 if bad else 0)
PY
