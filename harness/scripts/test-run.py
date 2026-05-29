#!/usr/bin/env python3

import shlex
import subprocess
import sys

LEVEL_ORDER = ("unit", "dv", "integration")

COMMANDS = {
    "p2p-frame": {
        "unit": ["cargo", "test", "-p", "p2p-frame"],
        "integration": ["cargo", "test", "--workspace"],
    },
    "cyfs-p2p": {
        "unit": ["cargo", "test", "-p", "cyfs-p2p"],
        "integration": ["cargo", "test", "--workspace"],
    },
    "cyfs-p2p-test": {
        "unit": ["cargo", "test", "-p", "cyfs-p2p-test"],
        "dv": ["cargo", "run", "-p", "cyfs-p2p-test", "--", "--help"],
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


def run_command(command: list[str]) -> int:
    print("+", shlex.join(command))
    completed = subprocess.run(command, check=False)
    return completed.returncode


def run_module(module: str, level: str) -> int:
    module_commands = COMMANDS.get(module)
    if module_commands is None:
        print(f"unknown module: {module}", file=sys.stderr)
        print(f"known modules: {', '.join(sorted(COMMANDS))}", file=sys.stderr)
        return 2

    if level == "all":
        for current_level in LEVEL_ORDER:
            command = module_commands.get(current_level)
            if command is None:
                continue
            result = run_command(command)
            if result != 0:
                return result
        return 0

    command = module_commands.get(level)
    if command is None:
        print(f"unknown level: {level}", file=sys.stderr)
        print(f"known levels for {module}: all, {', '.join(sorted(module_commands))}", file=sys.stderr)
        return 2

    return run_command(command)


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: python3 ./harness/scripts/test-run.py <module|all> <unit|dv|integration|all>", file=sys.stderr)
        return 2

    module, level = sys.argv[1], sys.argv[2]

    if level not in {"unit", "dv", "integration", "all"}:
        print(f"unknown level: {level}", file=sys.stderr)
        return 2

    if module == "all":
        for current_module in sorted(COMMANDS):
            result = run_module(current_module, level)
            if result != 0:
                return result
        return 0

    return run_module(module, level)


if __name__ == "__main__":
    raise SystemExit(main())
