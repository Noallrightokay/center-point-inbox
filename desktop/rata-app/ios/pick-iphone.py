"""Print the UDID of an iPhone simulator to run RATA on: the newest iOS
runtime's, and an ordinary-sized phone before the largest, the smallest or
a cheaper model, since the screenshots show what most people see. What was
picked goes to stderr. For .github/workflows/ios.yml."""

import json
import re
import subprocess
import sys


def version(s):
    return tuple(int(x) for x in re.findall(r"\d+", s))


listed = json.loads(subprocess.check_output(["xcrun", "simctl", "list", "devices", "available", "-j"]))
phones = []
for runtime, devices in listed["devices"].items():
    if ".iOS-" not in runtime:
        continue
    for d in devices:
        if not d.get("isAvailable") or not d["name"].startswith("iPhone"):
            continue
        words = d["name"].split()
        ordinary = not any(w in words for w in ("Max", "Plus", "SE", "mini", "Air")) and not d[
            "name"
        ].endswith("e")
        phones.append((version(runtime.split(".iOS-")[-1]), ordinary, d["name"], d["udid"]))
phones.sort(reverse=True)
if not phones:
    sys.exit("No iPhone simulator on this machine.")
runtime, _, name, udid = phones[0]
print(f"{name}, iOS {'.'.join(map(str, runtime))}: {udid}", file=sys.stderr)
print(udid)
