#!/usr/bin/env python3
"""cargo-env.py NAME... -- gives an iOS build of RATA the variables NAME...

Each NAME's value is read from this script's own environment (never from its
arguments, so no value is on a command line) and written into an [env]
table in Cargo's own config, $CARGO_HOME/config.toml (~/.cargo/config.toml
without CARGO_HOME). That is the only way a value reaches the Rust code of an
iOS build: Xcode compiles it (`tauri ios xcode-script`), and the Tauri CLI
hands that build only the variables named TAURI*, WRY*, CARGO_*, RUST_*, PATH
and TMPDIR (tauri-cli's mobile/mod.rs, env_vars), so RATA_LICENCE_PUBLIC_KEY,
say, in the environment alone never arrives, and option_env! reads nothing.

For .github/workflows/ios.yml, on a runner that is thrown away after the job.
Refuses a config that already has an [env] table, a NAME that is not set, and
a NAME that is not RATA's (RATA_ and capitals, digits and underscores).
Standard library only.
"""

import json
import os
import re
import sys


def main(names):
    if not names:
        sys.exit("cargo-env.py: name at least one variable")
    lines = []
    for name in names:
        if not re.fullmatch(r"RATA_[A-Z0-9_]+", name):
            sys.exit(f"cargo-env.py: {name!r} is not a RATA_ variable")
        value = os.environ.get(name)
        if not value:
            sys.exit(f"cargo-env.py: {name} is not set")
        # A JSON string is a TOML basic string too.
        lines.append(f"{name} = {json.dumps(value)}")
    home = os.environ.get("CARGO_HOME") or os.path.join(os.path.expanduser("~"), ".cargo")
    config = os.path.join(home, "config.toml")
    try:
        with open(config) as f:
            held = f.read()
    except FileNotFoundError:
        held = ""
    if re.search(r"^\s*\[env\]\s*$", held, re.MULTILINE):
        sys.exit(f"cargo-env.py: {config} already has an [env] table")
    os.makedirs(home, exist_ok=True)
    with open(config, "a") as f:
        f.write("\n[env]\n" + "\n".join(lines) + "\n")
    print(f"cargo-env.py: {', '.join(names)} in {config}'s [env]")


if __name__ == "__main__":
    main(sys.argv[1:])
