#!/usr/bin/env python3

import shlex
import subprocess
import sys


COMMANDS = {
    "p2p-frame": {
        "unit": ["cargo", "test", "-p", "p2p-frame"],
        "dv": ["cargo", "run", "-p", "cyfs-p2p-test", "--", "all-in-one"],
        "integration": ["cargo", "test", "--workspace"],
    },
    "cyfs-p2p": {
        "unit": ["cargo", "test", "-p", "cyfs-p2p"],
        "dv": ["cargo", "run", "-p", "cyfs-p2p-test", "--", "all-in-one"],
        "integration": ["cargo", "test", "--workspace"],
    },
    "cyfs-p2p-test": {
        "unit": ["cargo", "test", "-p", "cyfs-p2p-test"],
        "dv": ["cargo", "run", "-p", "cyfs-p2p-test", "--", "all-in-one"],
        "integration": ["cargo", "test", "--workspace"],
    },
    "sn-miner": {
        "unit": ["cargo", "test", "-p", "sn-miner"],
        "dv": ["cargo", "run", "-p", "sn-miner", "--", "--help"],
        "integration": ["cargo", "test", "--workspace"],
    },
    "desc-tool": {
        "unit": ["cargo", "test", "-p", "desc-tool"],
        "dv": ["cargo", "run", "-p", "desc-tool", "--", "--help"],
        "integration": ["cargo", "test", "--workspace"],
    },
}


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: python3 ./harness/scripts/test-run.py <module> <unit|dv|integration>", file=sys.stderr)
        return 2

    module, level = sys.argv[1], sys.argv[2]
    module_commands = COMMANDS.get(module)
    if module_commands is None:
        print(f"unknown module: {module}", file=sys.stderr)
        print(f"known modules: {', '.join(sorted(COMMANDS))}", file=sys.stderr)
        return 2

    command = module_commands.get(level)
    if command is None:
        print(f"unknown level: {level}", file=sys.stderr)
        print("known levels: unit, dv, integration", file=sys.stderr)
        return 2

    print("+", shlex.join(command))
    completed = subprocess.run(command, check=False)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
