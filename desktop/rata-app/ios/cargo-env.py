#!/usr/bin/env python3
"""cargo-env.py NAME... -- gives an iOS build of RATA the variables NAME...
cargo-env.py --remove -- takes them away again

Each NAME's value is read from this script's own environment (never from its
arguments, so no value is on a command line) and written into an [env]
table in a Cargo config of RATA's shell alone, desktop/rata-app/src-tauri/
.cargo/config.toml, which git ignores. That is the only way a value reaches
the Rust code of an iOS build: Xcode compiles it (`tauri ios xcode-script`),
and the Tauri CLI hands that build only the variables named TAURI*, WRY*,
CARGO_*, RUST_*, PATH and TMPDIR (tauri-cli's mobile/mod.rs, env_vars), so
RATA_LICENCE_PUBLIC_KEY, say, in the environment alone never arrives, and
option_env! reads nothing. Cargo reads the .cargo/config.toml of the folder
it runs in and of each folder above it, and xcode-script runs it in
src-tauri: Xcode starts the script in gen/apple, which it leaves for the
folder two above (tauri-cli 2.12.0, mobile/ios/xcode_script.rs).

While the file is there, every Cargo build in src-tauri reads it, a
computer's too, and a build given a throwaway licence key that way trusts
only that key and refuses every real licence. So on a Mac of your own,
remove it as soon as the iOS build is made: `cargo-env.py --remove`. On
.github/workflows/ios.yml's runner, thrown away after the job, nothing
needs removing. Cargo's own config ($CARGO_HOME/config.toml), which every
build on the machine reads, is never written.

Refuses when the file is already there (remove it first), a NAME that is
not set, and a NAME that is not RATA's (RATA_ and capitals, digits and
underscores). --remove takes away only a file this script wrote.
Standard library only.
"""

import json
import os
import re
import sys

CONFIG = os.path.normpath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", ".cargo", "config.toml")
)
# The file's first line, by which --remove knows the file is this script's.
MARK = "# Written by desktop/rata-app/ios/cargo-env.py for an iOS build: remove it with cargo-env.py --remove.\n"


def remove():
    try:
        with open(CONFIG) as f:
            held = f.read()
    except FileNotFoundError:
        print(f"cargo-env.py: there is no {CONFIG}")
        return
    if not held.startswith(MARK):
        sys.exit(f"cargo-env.py: {CONFIG} was not written by this script, so it is left as it is")
    os.remove(CONFIG)
    try:
        os.rmdir(os.path.dirname(CONFIG))
    except OSError:
        pass  # something else is in .cargo
    print(f"cargo-env.py: {CONFIG} removed")


def main(args):
    if args == ["--remove"]:
        remove()
        return
    if not args:
        sys.exit("cargo-env.py: name at least one variable, or --remove")
    lines = []
    for name in args:
        if not re.fullmatch(r"RATA_[A-Z0-9_]+", name):
            sys.exit(f"cargo-env.py: {name!r} is not a RATA_ variable")
        value = os.environ.get(name)
        if not value:
            sys.exit(f"cargo-env.py: {name} is not set")
        # A JSON string is a TOML basic string too.
        lines.append(f"{name} = {json.dumps(value)}")
    os.makedirs(os.path.dirname(CONFIG), exist_ok=True)
    try:
        with open(CONFIG, "x") as f:
            f.write(MARK + "[env]\n" + "\n".join(lines) + "\n")
    except FileExistsError:
        sys.exit(f"cargo-env.py: {CONFIG} is there already: cargo-env.py --remove first")
    print(f"cargo-env.py: {', '.join(args)} in {CONFIG}'s [env]")
    print("cargo-env.py: every Cargo build in src-tauri reads it until cargo-env.py --remove")


if __name__ == "__main__":
    main(sys.argv[1:])
