"""Print the newest Xcode in /Applications whose iOS Simulator SDK has a
simulator runtime installed to run on (the Tauri CLI refuses one without).
Release versions only (Xcode_26.0.app, not Xcode_26.1_beta.app). Every
Xcode found, and its SDK, goes to stderr. For .github/workflows/ios.yml."""

import glob
import json
import os
import plistlib
import re
import subprocess
import sys


def version(s):
    return tuple(int(x) for x in re.findall(r"\d+", s))


def shown(v):
    return ".".join(map(str, v))


listed = json.loads(subprocess.check_output(["xcrun", "simctl", "list", "runtimes", "-j"]))
runtimes = [
    version(r["version"])
    for r in listed["runtimes"]
    if r.get("isAvailable") and r.get("name", "").startswith("iOS")
]
newest_runtime = max(runtimes, default=())
print(f"iOS simulator runtimes: {', '.join(map(shown, sorted(runtimes)))}", file=sys.stderr)

found = []
for app in glob.glob("/Applications/Xcode_*.app"):
    m = re.fullmatch(r"/Applications/Xcode_(\d+(?:\.\d+)*)\.app", app)
    settings = (
        app
        + "/Contents/Developer/Platforms/iPhoneSimulator.platform"
        + "/Developer/SDKs/iPhoneSimulator.sdk/SDKSettings.plist"
    )
    if not m or not os.path.exists(settings):
        continue
    with open(settings, "rb") as f:
        sdk = version(plistlib.load(f)["Version"])
    found.append((version(m.group(1)), sdk, os.path.realpath(app)))
found.sort(reverse=True)
for xcode, sdk, app in found:
    print(f"Xcode {shown(xcode)} ({app}): Simulator SDK {shown(sdk)}", file=sys.stderr)

usable = [f for f in found if newest_runtime >= f[1]]
if not usable:
    sys.exit("No Xcode here has a simulator runtime for its SDK.")
if usable[0] != found[0]:
    print(
        f"::warning::Xcode {shown(found[0][0])} has no simulator runtime for its SDK; "
        f"using Xcode {shown(usable[0][0])}",
        file=sys.stderr,
    )
print(usable[0][2])
