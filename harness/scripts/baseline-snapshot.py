#!/usr/bin/env python3
"""Capture and compare task-start working-tree baselines.

The baseline copies only tracked files that were already modified when the task
started and non-ignored untracked files that already existed. Clean tracked
files need no backup because Git reports any later modification directly.
Harness/docs paths and ignored files are excluded. Neither mode creates or
mutates Git indexes, trees, or commits.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path, PurePosixPath


TASK_ID_PART_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")
WORKTREE_BASELINE_KIND = "working-tree-content-baseline"
HASH_BASELINE_KIND = "content-hash-baseline"
LEGACY_DIRECT_CONTENT_KIND = "direct-content-snapshot"
EXCLUDED_ROOTS = {".harness", "harness", "docs"}


def fail(message: str) -> None:
    print(f"baseline-snapshot: {message}", file=sys.stderr)
    raise SystemExit(1)


def normalize(path: str) -> str:
    normalized = path.replace("\\", "/").lstrip("\ufeff")
    if normalized.startswith("./"):
        normalized = normalized[2:]
    candidate = PurePosixPath(normalized)
    if not normalized or candidate.is_absolute() or ".." in candidate.parts:
        fail(f"path must stay inside the repository: {path}")
    return candidate.as_posix()


def repo_path(
    root: Path, value: str, label: str, *, must_exist: bool = False
) -> tuple[str, Path]:
    relative = normalize(value)
    root_resolved = root.resolve()
    candidate = root_resolved / Path(*PurePosixPath(relative).parts)
    resolved = candidate.resolve(strict=False)
    try:
        resolved.relative_to(root_resolved)
    except ValueError:
        fail(f"{label} resolves outside the repository: {value}")
    if must_exist and not candidate.is_file():
        fail(f"{label} is not a file: {relative}")
    return relative, candidate


def canonical_source(root: Path, path: str) -> tuple[str, Path]:
    return repo_path(root, path, "baseline source", must_exist=True)


def git_bytes(root: Path, args: list[str], *, required: bool = True) -> bytes:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    except FileNotFoundError:
        fail("git executable not found")
    if required and result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        fail(detail or f"git {' '.join(args)} failed")
    return result.stdout if result.returncode == 0 else b""


def git_text(root: Path, args: list[str], *, required: bool = True) -> str:
    return git_bytes(root, args, required=required).decode(
        "utf-8", errors="surrogateescape"
    ).strip()


def require_git_root(root: Path) -> Path:
    resolved = root.resolve()
    top = Path(git_text(resolved, ["rev-parse", "--show-toplevel"])).resolve()
    if top != resolved:
        fail(f"--root must be the Git repository root: {top}")
    return resolved


def decode_path(raw: bytes) -> str:
    return normalize(raw.decode("utf-8", errors="surrogateescape"))


def nul_paths(output: bytes) -> set[str]:
    return {decode_path(raw) for raw in output.split(b"\0") if raw}


def excluded_path(path: str) -> bool:
    return PurePosixPath(path).parts[0] in EXCLUDED_ROOTS


def ignored_paths(root: Path, paths: set[str]) -> set[str]:
    if not paths:
        return set()
    encoded = b"\0".join(
        path.encode("utf-8", errors="surrogateescape") for path in sorted(paths)
    ) + b"\0"
    result = subprocess.run(
        ["git", "check-ignore", "--no-index", "-z", "--stdin"],
        cwd=root,
        input=encoded,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode not in {0, 1}:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        fail(detail or "git check-ignore failed")
    return nul_paths(result.stdout)


def eligible_paths(root: Path, paths: set[str]) -> set[str]:
    candidates = {path for path in paths if not excluded_path(path)}
    return candidates - ignored_paths(root, candidates)


def tracked_paths(root: Path) -> set[str]:
    """All eligible tracked paths, retained for legacy baseline comparison."""
    return eligible_paths(root, nul_paths(git_bytes(root, ["ls-files", "-z", "--"])))


def modified_tracked_paths(root: Path) -> set[str]:
    unstaged = nul_paths(git_bytes(root, ["diff", "--name-only", "-z", "--"]))
    staged = nul_paths(
        git_bytes(root, ["diff", "--cached", "--name-only", "-z", "--"])
    )
    return eligible_paths(root, unstaged | staged)


def untracked_paths(root: Path) -> set[str]:
    return eligible_paths(
        root,
        nul_paths(
            git_bytes(root, ["ls-files", "--others", "--exclude-standard", "-z", "--"])
        ),
    )


def task_directory(root: Path, task_id: str) -> Path:
    normalized = normalize(task_id)
    parts = PurePosixPath(normalized).parts
    if len(parts) < 2 or any(not TASK_ID_PART_RE.fullmatch(part) for part in parts):
        fail(f"invalid task id: {task_id}")
    return root.resolve() / ".harness" / "baselines" / Path(*parts)


def copy_regular(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def snapshot_record(
    root: Path,
    relative: str,
    task_dir: Path,
    namespace: str,
) -> dict[str, object]:
    """Copy one pre-existing dirty/untracked path for later comparison."""
    normalized, source = repo_path(root, relative, "workspace path")
    snapshot_relative = PurePosixPath(namespace) / PurePosixPath(normalized)
    destination = task_dir / Path(*snapshot_relative.parts)
    try:
        metadata = source.lstat()
    except FileNotFoundError:
        return {"path": normalized, "kind": "absent"}
    if stat.S_ISLNK(metadata.st_mode):
        return {
            "path": normalized,
            "kind": "symlink",
            "target": os.readlink(source),
        }
    if stat.S_ISREG(metadata.st_mode):
        copy_regular(source, destination)
        return {
            "path": normalized,
            "kind": "file",
            "snapshot": snapshot_relative.as_posix(),
            "executable": bool(metadata.st_mode & 0o111),
        }
    if stat.S_ISDIR(metadata.st_mode):
        return {"path": normalized, "kind": "directory"}
    return {
        "path": normalized,
        "kind": "other",
        "mode": stat.S_IFMT(metadata.st_mode),
    }


def snapshot_path(manifest: Path, raw_snapshot: object) -> Path:
    if not isinstance(raw_snapshot, str) or not raw_snapshot:
        fail("baseline record requires a snapshot path")
    relative = normalize(raw_snapshot)
    snapshot = manifest.parent / Path(*PurePosixPath(relative).parts)
    resolved = snapshot.resolve(strict=False)
    try:
        resolved.relative_to(manifest.parent.resolve())
    except ValueError:
        fail(f"baseline snapshot resolves outside its task directory: {raw_snapshot}")
    return snapshot


def directory_entries(path: Path) -> dict[str, tuple[object, ...]]:
    if not path.is_dir():
        return {}
    entries: dict[str, tuple[object, ...]] = {}
    for child in sorted(path.rglob("*")):
        relative = child.relative_to(path).as_posix()
        if any(part in {".git", ".harness", "__pycache__"} for part in child.relative_to(path).parts):
            continue
        metadata = child.lstat()
        if stat.S_ISLNK(metadata.st_mode):
            entries[relative] = ("symlink", os.readlink(child))
        elif stat.S_ISREG(metadata.st_mode):
            entries[relative] = (
                "file",
                bool(metadata.st_mode & 0o111),
                child.read_bytes(),
            )
        elif stat.S_ISDIR(metadata.st_mode):
            entries[relative] = ("directory",)
        else:
            entries[relative] = ("other", stat.S_IFMT(metadata.st_mode))
    return entries


def record_matches(root: Path, manifest: Path, record: dict[str, object]) -> bool:
    raw_path = record.get("path")
    kind = record.get("kind")
    if not isinstance(raw_path, str) or not isinstance(kind, str):
        fail("baseline contains a malformed workspace record")
    _, current = repo_path(root, raw_path, "workspace path")
    try:
        metadata = current.lstat()
    except FileNotFoundError:
        return kind == "absent"
    if kind == "symlink":
        return stat.S_ISLNK(metadata.st_mode) and os.readlink(current) == record.get("target")
    if kind == "file":
        if isinstance(record.get("sha256"), str):
            return (
                stat.S_ISREG(metadata.st_mode)
                and metadata.st_size == record.get("size")
                and bool(metadata.st_mode & 0o111) == record.get("executable")
                and sha256_file(current) == record.get("sha256")
            )
        snapshot = snapshot_path(manifest, record.get("snapshot"))
        return (
            stat.S_ISREG(metadata.st_mode)
            and snapshot.is_file()
            and bool(metadata.st_mode & 0o111) == record.get("executable")
            and current.read_bytes() == snapshot.read_bytes()
        )
    if kind == "directory":
        if "snapshot" not in record:
            return stat.S_ISDIR(metadata.st_mode)
        snapshot = snapshot_path(manifest, record.get("snapshot"))
        return stat.S_ISDIR(metadata.st_mode) and directory_entries(current) == directory_entries(snapshot)
    if kind == "other":
        return stat.S_IFMT(metadata.st_mode) == record.get("mode")
    return False


def file_record(root: Path, relative: str, task_dir: Path) -> dict[str, str]:
    normalized, source = canonical_source(root, relative)
    snapshot_relative = PurePosixPath("files") / PurePosixPath(normalized)
    destination = task_dir / Path(*snapshot_relative.parts)
    copy_regular(source, destination)
    return {"path": normalized, "snapshot": snapshot_relative.as_posix()}


def task_binding(root: Path, raw_task: str) -> dict[str, str]:
    supplied = Path(raw_task)
    if supplied.is_absolute():
        try:
            raw_task = supplied.resolve().relative_to(root.resolve()).as_posix()
        except ValueError:
            fail(f"task manifest resolves outside the repository: {raw_task}")
    relative, _ = repo_path(root, raw_task, "task manifest", must_exist=True)
    return {"path": relative}


def readable_content(path: Path) -> dict[str, object]:
    if not path.is_file():
        return {"present": False}
    return {
        "present": True,
        "content": path.read_bytes().decode("utf-8", errors="surrogateescape"),
    }


def ignore_binding(
    root: Path,
    tracked: set[str],
    untracked: set[str],
) -> dict[str, object]:
    files: list[dict[str, object]] = []
    for relative in sorted(
        path
        for path in tracked | untracked
        if PurePosixPath(path).name == ".gitignore"
    ):
        _, path = repo_path(root, relative, "ignore file")
        files.append({"path": relative, **readable_content(path)})

    info_exclude = git_text(root, ["rev-parse", "--git-path", "info/exclude"])
    info_path = Path(info_exclude)
    if not info_path.is_absolute():
        info_path = root / info_path
    files.append({"path": "<git-info-exclude>", **readable_content(info_path)})

    global_exclude = git_text(
        root, ["config", "--path", "--get", "core.excludesFile"], required=False
    )
    if global_exclude:
        files.append(
            {
                "path": "<core-excludes-file>",
                "configured_path": global_exclude,
                **readable_content(Path(global_exclude)),
            }
        )
    return {"files": files}


def capture(root: Path, task_id: str, paths: list[str]) -> Path:
    """Legacy explicit content snapshot used by line-level checks."""
    if not paths:
        fail("at least one --path is required")
    task_dir = task_directory(root, task_id)
    if task_dir.exists():
        fail(f"baseline already exists and will not be overwritten: {task_dir}")
    sources = list(dict.fromkeys(normalize(path) for path in paths))
    records = [file_record(root, path, task_dir) for path in sources]
    manifest = task_dir / "manifest.json"
    manifest.write_text(
        json.dumps(
            {"schema": 1, "task_id": task_id, "files": records},
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    return manifest


def capture_hybrid(
    root: Path,
    task_id: str,
    raw_task: str,
    copy_paths: list[str] | None = None,
) -> Path:
    root = require_git_root(root)
    task_dir = task_directory(root, task_id)
    if task_dir.exists():
        fail(f"baseline already exists and will not be overwritten: {task_dir}")

    tracked = modified_tracked_paths(root)
    untracked = untracked_paths(root)
    tracked_records = [
        snapshot_record(root, path, task_dir, "workspace/tracked")
        for path in sorted(tracked)
    ]
    untracked_records = [
        snapshot_record(root, path, task_dir, "workspace/untracked")
        for path in sorted(untracked)
    ]
    requested_copies = list(dict.fromkeys(normalize(path) for path in copy_paths or []))
    eligible_copies = eligible_paths(root, set(requested_copies))
    files = [
        file_record(root, path, task_dir)
        for path in requested_copies
        if path in eligible_copies
    ]
    payload = {
        "schema": 5,
        "kind": WORKTREE_BASELINE_KIND,
        "task_id": task_id,
        "task_manifest": task_binding(root, raw_task),
        "ignore_rules": ignore_binding(root, tracked, untracked),
        "tracked": tracked_records,
        "untracked": untracked_records,
        "files": files,
    }
    task_dir.mkdir(parents=True, exist_ok=True)
    manifest = task_dir / "manifest.json"
    manifest.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return manifest


def load_hybrid(root: Path, raw_manifest: str) -> tuple[Path, dict[str, object]]:
    relative, manifest = repo_path(
        root, raw_manifest, "baseline manifest", must_exist=True
    )
    if not relative.startswith(".harness/baselines/"):
        fail("baseline manifest must live under .harness/baselines/")
    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"invalid baseline manifest {relative}: {error}")
    supported = (
        isinstance(payload, dict)
        and (
            (
                payload.get("schema") == 5
                and payload.get("kind") == WORKTREE_BASELINE_KIND
            )
            or (payload.get("schema") == 4 and payload.get("kind") == HASH_BASELINE_KIND)
            or (
                payload.get("schema") == 3
                and payload.get("kind") == LEGACY_DIRECT_CONTENT_KIND
            )
        )
    )
    if not supported:
        fail(f"baseline manifest {relative} is not a supported repository baseline")
    return manifest, payload


def verify_copied_files(root: Path, manifest: Path, payload: dict[str, object]) -> None:
    files = payload.get("files")
    if not isinstance(files, list):
        fail("baseline files must be a list")
    for record in files:
        if not isinstance(record, dict):
            fail("baseline contains a malformed copied-file record")
        raw_path = record.get("path")
        raw_snapshot = record.get("snapshot")
        if not all(
            isinstance(value, str) and value for value in (raw_path, raw_snapshot)
        ):
            fail("baseline contains an incomplete copied-file record")
        repo_path(root, raw_path, "copied baseline source")
        snapshot = snapshot_path(manifest, raw_snapshot)
        if not snapshot.is_file():
            fail(f"baseline snapshot does not exist: {raw_path}")


def verify_workspace_snapshots(manifest: Path, payload: dict[str, object]) -> None:
    for label in ("tracked", "untracked"):
        for record in records_by_path(payload.get(label), label).values():
            if record.get("kind") != "file" or "snapshot" not in record:
                continue
            snapshot = snapshot_path(manifest, record.get("snapshot"))
            if not snapshot.is_file():
                fail(f"baseline snapshot does not exist: {record.get('path')}")


def verify_hybrid(
    root: Path, raw_manifest: str, raw_task: str
) -> tuple[Path, dict[str, object]]:
    root = require_git_root(root)
    manifest, payload = load_hybrid(root, raw_manifest)
    if payload.get("task_manifest") != task_binding(root, raw_task):
        fail("task manifest path does not match the captured baseline")
    verify_copied_files(root, manifest, payload)
    verify_workspace_snapshots(manifest, payload)
    return manifest, payload


def records_by_path(raw: object, label: str) -> dict[str, dict[str, object]]:
    if not isinstance(raw, list):
        fail(f"baseline {label} must be a list")
    records: dict[str, dict[str, object]] = {}
    for record in raw:
        if not isinstance(record, dict) or not isinstance(record.get("path"), str):
            fail(f"baseline contains a malformed {label} record")
        path = normalize(str(record["path"]))
        if path in records:
            fail(f"baseline contains a duplicate {label} path: {path}")
        records[path] = record
    return records


def hybrid_changed_paths(
    root: Path, manifest: Path, payload: dict[str, object]
) -> list[str]:
    current_tracked = (
        modified_tracked_paths(root)
        if payload.get("schema") == 5
        else tracked_paths(root)
    )
    current_untracked = untracked_paths(root)
    baseline_tracked = records_by_path(payload.get("tracked"), "tracked")
    baseline_untracked = records_by_path(payload.get("untracked"), "untracked")

    changed: set[str] = set()
    all_paths = (
        set(baseline_tracked)
        | set(baseline_untracked)
        | current_tracked
        | current_untracked
    )
    for path in all_paths:
        tracked_before = baseline_tracked.get(path)
        untracked_before = baseline_untracked.get(path)
        if tracked_before is None and untracked_before is None:
            changed.add(path)
            continue
        if tracked_before is not None:
            if path not in current_tracked or path in current_untracked:
                changed.add(path)
                continue
            if not record_matches(root, manifest, tracked_before):
                changed.add(path)
            continue
        if path not in current_untracked or path in current_tracked:
            changed.add(path)
            continue
        assert untracked_before is not None
        if not record_matches(root, manifest, untracked_before):
            changed.add(path)
    return sorted(changed)


def write_changed_paths(root: Path, raw_output: str, paths: list[str]) -> Path:
    relative, output = repo_path(root, raw_output, "changed paths output")
    if not relative.startswith(".harness/evidence/") or not relative.endswith(
        ".paths"
    ):
        fail("changed paths output must live under .harness/evidence/ and end with .paths")
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(output.name + ".tmp")
    temporary.write_text(
        "".join(f"{path}\n" for path in paths), encoding="utf-8"
    )
    os.replace(temporary, output)
    return output


def diff_hybrid(
    root: Path, raw_manifest: str, raw_task: str, raw_output: str
) -> tuple[Path, list[str]]:
    root = require_git_root(root)
    manifest, payload = verify_hybrid(root, raw_manifest, raw_task)
    paths = hybrid_changed_paths(root, manifest, payload)
    return write_changed_paths(root, raw_output, paths), paths


def command_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    capture_parser = subparsers.add_parser("capture")
    capture_parser.add_argument("--root", default=".")
    capture_parser.add_argument("--task-id", required=True)
    capture_parser.add_argument("--task", required=True)
    capture_parser.add_argument("--copy-path", action="append", default=[])
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--root", default=".")
    verify_parser.add_argument("--manifest", required=True)
    verify_parser.add_argument("--task", required=True)
    diff_parser = subparsers.add_parser("diff")
    diff_parser.add_argument("--root", default=".")
    diff_parser.add_argument("--manifest", required=True)
    diff_parser.add_argument("--task", required=True)
    diff_parser.add_argument("--output", required=True)
    args = parser.parse_args(argv)
    if args.command == "capture":
        manifest = capture_hybrid(
            Path(args.root), args.task_id, args.task, args.copy_path
        )
        print(f"baseline-snapshot: wrote {manifest}")
    elif args.command == "verify":
        manifest, _ = verify_hybrid(
            Path(args.root), args.manifest, args.task
        )
        print(f"baseline-snapshot: verified {manifest}")
    else:
        output, paths = diff_hybrid(
            Path(args.root), args.manifest, args.task, args.output
        )
        print(
            f"baseline-snapshot: wrote {output} ({len(paths)} changed path(s))"
        )
    return 0


def legacy_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--task-id", required=True)
    parser.add_argument("--path", action="append", dest="paths", required=True)
    args = parser.parse_args(argv)
    manifest = capture(Path(args.root), args.task_id, args.paths)
    print(f"baseline-snapshot: wrote {manifest}")
    return 0


def main() -> int:
    argv = sys.argv[1:]
    if argv and argv[0] in {"capture", "verify", "diff"}:
        return command_main(argv)
    return legacy_main(argv)


if __name__ == "__main__":
    raise SystemExit(main())
