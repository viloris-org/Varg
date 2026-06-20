#!/usr/bin/env python3
"""Fail clearly before Tauri aborts on common Linux dev resource limits."""

from __future__ import annotations

import argparse
import ctypes
import errno
import os
from pathlib import Path
import resource
import sys


IN_CLOEXEC = getattr(os, "O_CLOEXEC", 0)
IN_NONBLOCK = getattr(os, "O_NONBLOCK", 0)
MIN_DEV_OPEN_FILE_LIMIT = 4096
MIN_DEV_INOTIFY_INSTANCES = 4


def open_inotify_instance() -> int:
    libc = ctypes.CDLL(None, use_errno=True)
    descriptor = libc.inotify_init1(IN_CLOEXEC | IN_NONBLOCK)
    if descriptor == -1:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))
    return descriptor


def open_inotify_instances(count: int) -> list[int]:
    descriptors: list[int] = []
    try:
        for _ in range(count):
            descriptors.append(open_inotify_instance())
    except OSError:
        for descriptor in descriptors:
            os.close(descriptor)
        raise
    return descriptors


def read_integer(path: Path) -> int | None:
    try:
        return int(path.read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return None


def count_process_fds(proc_root: Path, pid: int) -> int | None:
    try:
        return len(list((proc_root / str(pid) / "fd").iterdir()))
    except OSError:
        return None


def inotify_usage(proc_root: Path, uid: int) -> tuple[int, list[tuple[int, int, str]]]:
    total = 0
    processes: list[tuple[int, int, str]] = []

    try:
        entries = proc_root.iterdir()
    except OSError:
        return total, processes

    for entry in entries:
        if not entry.name.isdigit():
            continue

        try:
            status = (entry / "status").read_text(encoding="utf-8")
            process_uid = next(
                int(line.split()[1]) for line in status.splitlines() if line.startswith("Uid:")
            )
            if process_uid != uid:
                continue

            count = 0
            for descriptor in (entry / "fd").iterdir():
                try:
                    if os.readlink(descriptor) == "anon_inode:inotify":
                        count += 1
                except OSError:
                    continue

            if count:
                try:
                    name = (entry / "comm").read_text(encoding="utf-8").strip()
                except OSError:
                    name = "unknown"
                total += count
                processes.append((count, int(entry.name), name))
        except (OSError, StopIteration, ValueError):
            continue

    processes.sort(reverse=True)
    return total, processes


def format_open_file_limit_failure(open_fds: int | None, soft_limit: int) -> str:
    usage = f"{open_fds}/{soft_limit}" if open_fds is not None else str(soft_limit)
    return (
        "Aster editor dev startup stopped before Tauri could abort.\n"
        "The shell open-file limit is too low for Tauri/Cargo dev startup "
        f"({usage}).\n"
        "This is a per-process file descriptor limit; it is not a general app or thread limit.\n"
        f"Raise it for this shell, for example: `ulimit -n {MIN_DEV_OPEN_FILE_LIMIT}`."
    )


def format_inotify_failure(
    proc_root: Path = Path("/proc"), required_instances: int = MIN_DEV_INOTIFY_INSTANCES
) -> str:
    soft_limit, _ = resource.getrlimit(resource.RLIMIT_NOFILE)
    open_fds = count_process_fds(proc_root, os.getpid())

    if open_fds is not None and soft_limit != resource.RLIM_INFINITY:
        if open_fds >= max(0, soft_limit - 8):
            return format_open_file_limit_failure(open_fds, soft_limit)

    limit = read_integer(proc_root / "sys/fs/inotify/max_user_instances")
    used, processes = inotify_usage(proc_root, os.getuid())
    usage = f"{used}/{limit}" if limit is not None else str(used)

    lines = [
        "Aster editor dev startup stopped before Tauri could abort.",
        f"Linux file-watch instance limit reached: need {required_instances} spare inotify instances; observed {usage}.",
        "This is only the Linux file-watcher quota, not a general app/process/thread limit.",
    ]
    if used == 0:
        lines.append(
            "/proc usage can under-report in restricted environments; the allocation probe is authoritative."
        )
    if processes:
        consumers = ", ".join(
            f"{name} (pid {pid}: {count})" for count, pid, name in processes[:5]
        )
        lines.append(f"Largest consumers: {consumers}.")
    lines.extend(
        [
            "For local development, free one file-watching process such as an IDE/dev server, then retry.",
            "If this is your development machine, consider raising fs.inotify.max_user_instances outside this project.",
            "Run from repo root with `scripts/dev-editor.sh`, or from editor/ with `bun run dev:tauri`.",
        ]
    )
    return "\n".join(lines)


def format_startup_resource_failure(proc_root: Path = Path("/proc")) -> str | None:
    soft_limit, _ = resource.getrlimit(resource.RLIMIT_NOFILE)
    open_fds = count_process_fds(proc_root, os.getpid())

    if soft_limit != resource.RLIM_INFINITY and soft_limit < MIN_DEV_OPEN_FILE_LIMIT:
        return format_open_file_limit_failure(open_fds, soft_limit)

    if open_fds is not None and soft_limit != resource.RLIM_INFINITY:
        if open_fds >= max(0, soft_limit - 8):
            return format_open_file_limit_failure(open_fds, soft_limit)

    return None


def has_startup_resource_failure() -> bool:
    return format_startup_resource_failure() is not None


def has_inotify_capacity(required_instances: int = MIN_DEV_INOTIFY_INSTANCES) -> bool:
    descriptors: list[int] = []
    try:
        descriptors = open_inotify_instances(required_instances)
    except OSError as error:
        if error.errno == errno.EMFILE:
            return False
        raise
    finally:
        for descriptor in descriptors:
            os.close(descriptor)

    return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--force-linux",
        action="store_true",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Only use the exit code; do not print diagnostics.",
    )
    parser.add_argument(
        "--startup-only",
        action="store_true",
        help="Only check non-degradable startup resource limits.",
    )
    parser.add_argument(
        "--inotify-only",
        action="store_true",
        help="Only check Linux inotify capacity.",
    )
    args = parser.parse_args()

    if args.startup_only and args.inotify_only:
        parser.error("--startup-only and --inotify-only are mutually exclusive")

    if not args.force_linux and not sys.platform.startswith("linux"):
        return 0

    if not args.inotify_only:
        if failure := format_startup_resource_failure():
            if not args.quiet:
                print(failure, file=sys.stderr)
            return 1

    if args.startup_only:
        return 0

    try:
        has_capacity = has_inotify_capacity(MIN_DEV_INOTIFY_INSTANCES)
    except OSError as error:
        print(f"Aster editor inotify preflight failed: {error}", file=sys.stderr)
        return 1

    if has_capacity:
        return 0

    if not args.quiet:
        print(format_inotify_failure(), file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
