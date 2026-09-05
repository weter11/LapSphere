#!/usr/bin/env python3
"""Atomic state helper for the lapsphere autonomous manager.

The cron-fired manager does NOT hand-type JSON mutations; it invokes this
helper via `terminal`. All mutations are atomic: write to a sibling tempfile
in the same directory, then os.replace() into place. This is the same
pattern the SteamFlow VPK surgery path uses for big-file atomic writes.

Subcommands (deterministic, no LLM, exit 0 on success / 1 on error):

    init                                    Create .hermes/state.json if absent.
    show                                    Print current state as JSON.
    set-phase <PHASE>                       Mutate the `phase` field.
    add-task --id <id> --title <t> --prio <p> --kind <k>
    pop-task                                Remove + print the highest-priority
                                            pending task (stdout JSON, one
                                            object). Empty stdout on empty queue.
    complete-task --id <id> --status <s>    Move task from backlog to
                                            completed_tasks; append a log line.
    add-log --message <m>                   Append a timestamped log entry.

Backlog ordering rule: priority (lower number = higher urgency), then
`created_at` timestamp (older first). Deterministic across runs.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


# Resolve state path from STATE_PATH env, fall back to .hermes/state.json
# relative to cwd. Cron jobs pin --workdir to the repo root, so cwd is the
# repo root when this script runs.
DEFAULT_STATE_PATH = Path(os.environ.get("LAPSPHERE_STATE", ".hermes/state.json"))


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _atomic_write(path: Path, payload: dict[str, Any]) -> None:
    """Write JSON atomically: tempfile in the same dir, fsync, os.replace.

    Same-directory tempfile guarantees os.replace is atomic on POSIX (it
    becomes a single inode rename on the same filesystem).

    Robustness: if the parent path is an existing FILE (e.g. caller passed
    a temp file path that already exists), we still need a sibling tempfile
    directory. We fall back to the system temp dir in that case so the write
    never fails on EEXIST.
    """
    parent = path.parent
    tmp_dir = parent if (not parent.exists() or parent.is_dir()) else Path(tempfile.gettempdir())
    tmp_dir.mkdir(parents=True, exist_ok=True)
    if not parent.exists():
        parent.mkdir(parents=True, exist_ok=True)
    data = json.dumps(payload, indent=2, sort_keys=False) + "\n"
    fd, tmp_path = tempfile.mkstemp(
        prefix=".state.", suffix=".json.tmp", dir=str(tmp_dir)
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp_path, path)
    except Exception:
        # Best-effort cleanup of the tempfile on failure
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise


def _read_state(path: Path) -> dict[str, Any]:
    if not path.exists():
        # Auto-seed on first access so a fresh repo Just Works.
        seed = {
            "phase": "UNINITIALIZED",
            "day_1_completed": False,
            "backlog": [],
            "completed_tasks": [],
            "logs": [],
        }
        _atomic_write(path, seed)
        return seed
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def cmd_init(args: argparse.Namespace) -> int:
    path = Path(args.path)
    state = _read_state(path)
    # Idempotent: re-init preserves existing data, only ensures file exists.
    print(json.dumps({"path": str(path), "phase": state.get("phase")}))
    return 0


def cmd_show(args: argparse.Namespace) -> int:
    path = Path(args.path)
    state = _read_state(path)
    print(json.dumps(state, indent=2, sort_keys=False))
    return 0


def cmd_set_phase(args: argparse.Namespace) -> int:
    path = Path(args.path)
    state = _read_state(path)
    old = state.get("phase")
    state["phase"] = args.phase
    if args.phase == "READY_FOR_EXECUTION":
        state["day_1_completed"] = True
    state.setdefault("logs", []).append(
        {"ts": _now_iso(), "event": "phase_change", "from": old, "to": args.phase}
    )
    _atomic_write(path, state)
    print(json.dumps({"ok": True, "from": old, "to": args.phase}))
    return 0


# Optional per-task metadata fields that round-trip through add-task,
# add-tasks (bulk JSON), and pop-task. Workers consume these as the
# explicit contract for non-trivial tasks (component audits, refactors).
# Keep keys here in sync with _OPTIONAL_TASK_FIELDS below.
_OPTIONAL_TASK_FIELDS = ("target_file", "doc_output", "acceptance")


def _attach_optional_task_fields(task: dict, source: dict) -> dict:
    """Copy optional metadata fields from `source` onto `task` if present
    and non-empty. Returns the (mutated) task for chaining."""
    for k in _OPTIONAL_TASK_FIELDS:
        v = source.get(k)
        if v:  # skip None and empty string
            task[k] = v
    return task


def cmd_add_task(args: argparse.Namespace) -> int:
    path = Path(args.path)
    state = _read_state(path)
    task = {
        "id": args.id,
        "title": args.title,
        "priority": int(args.prio),
        "kind": args.kind,
        "created_at": _now_iso(),
        "status": "pending",
    }
    _attach_optional_task_fields(task, {
        "target_file": getattr(args, "target_file", None),
        "doc_output": getattr(args, "doc_output", None),
        "acceptance": getattr(args, "acceptance", None),
    })
    state.setdefault("backlog", []).append(task)
    _atomic_write(path, state)
    print(json.dumps({"ok": True, "task": task}))
    return 0


def cmd_pop_task(args: argparse.Namespace) -> int:
    """Pop the highest-priority pending task.

    Selection rule: status == 'pending', then min(priority), then oldest
    created_at. Returns ONE task as JSON on stdout. Empty stdout on empty
    queue (so callers can `if [ -n "$(lapsphere_state.py pop-task)" ]`).

    --reap-stale <SECONDS>: re-queue any in_flight task whose claimed_at is
    older than the given window. This is the recovery mechanism for a tick
    that died before completing its task — the next tick reaps and retries
    rather than letting the queue stall on a phantom in_flight entry.
    """
    path = Path(args.path)
    state = _read_state(path)
    now_ts = _now_iso()
    if getattr(args, "reap_stale", None) is not None:
        from datetime import datetime, timedelta, timezone
        now = datetime.now(timezone.utc)
        threshold = now - timedelta(seconds=args.reap_stale)
        reaped = []
        for t in state.get("backlog", []):
            if t.get("status") == "in_flight" and t.get("claimed_at"):
                claimed = datetime.fromisoformat(t["claimed_at"])
                if claimed < threshold:
                    t["status"] = "pending"
                    t.pop("claimed_at", None)
                    reaped.append(t["id"])
        if reaped:
            state.setdefault("logs", []).append(
                {"ts": now_ts, "event": "reap_stale", "ids": reaped, "window_s": args.reap_stale}
            )
            _atomic_write(path, state)
    backlog = [t for t in state.get("backlog", []) if t.get("status") == "pending"]
    if not backlog:
        return 0
    backlog.sort(key=lambda t: (t.get("priority", 999), t.get("created_at", "")))
    chosen = backlog[0]
    # Mark in-flight so a re-pop within the same tick does not duplicate.
    for t in state["backlog"]:
        if t.get("id") == chosen["id"]:
            t["status"] = "in_flight"
            t["claimed_at"] = _now_iso()
            break
    _atomic_write(path, state)
    print(json.dumps(chosen, indent=2, sort_keys=False))
    return 0


def cmd_complete_task(args: argparse.Namespace) -> int:
    path = Path(args.path)
    state = _read_state(path)
    target_id = args.id
    moved = None
    new_backlog = []
    for t in state.get("backlog", []):
        if t.get("id") == target_id:
            moved = dict(t)
            moved["status"] = args.status
            moved["completed_at"] = _now_iso()
        else:
            new_backlog.append(t)
    if moved is None:
        print(
            json.dumps({"ok": False, "error": f"task {target_id!r} not in backlog"}),
            file=sys.stderr,
        )
        return 1
    state["backlog"] = new_backlog
    state.setdefault("completed_tasks", []).append(moved)
    state.setdefault("logs", []).append(
        {
            "ts": _now_iso(),
            "event": "task_completed",
            "id": target_id,
            "status": args.status,
        }
    )
    _atomic_write(path, state)
    print(json.dumps({"ok": True, "task": moved}))
    return 0


def cmd_add_log(args: argparse.Namespace) -> int:
    path = Path(args.path)
    state = _read_state(path)
    state.setdefault("logs", []).append({"ts": _now_iso(), "message": args.message})
    _atomic_write(path, state)
    print(json.dumps({"ok": True}))
    return 0


def cmd_add_tasks(args: argparse.Namespace) -> int:
    """Bulk-load tasks from a JSON file.

    File shape:
        {
          "tasks": [
            {"id": "task-x", "title": "...", "priority": 1, "kind": "doc"},
            ...
          ]
        }

    Behaviour:
        - Validates each task (id, title, priority int, kind in allowed set).
        - Skips tasks whose id is already in the backlog (idempotent re-runs).
        - Stamps created_at and status=pending.
        - On any validation error, aborts BEFORE any state mutation
          (refuses to half-load).
        - On success, performs ONE atomic write of the full new state.

    Use case: the day-start bootstrap writes a small JSON file with the
    proposed backlog, then calls add-tasks --from <file> in a single
    `terminal` invocation. This replaces 10+ individual `add-task` tool
    calls, which is what blew the agent's context window in RUN_C.
    """
    path = Path(args.path)
    state = _read_state(path)
    src = Path(args.from_file)
    if not src.is_file():
        print(
            json.dumps({"ok": False, "error": f"from-file not found: {src}"}),
            file=sys.stderr,
        )
        return 1
    try:
        payload = json.loads(src.read_text(encoding="utf-8"))
    except json.JSONDecodeError as e:
        print(
            json.dumps({"ok": False, "error": f"invalid JSON in {src}: {e}"}),
            file=sys.stderr,
        )
        return 1
    tasks = payload.get("tasks") if isinstance(payload, dict) else payload
    if not isinstance(tasks, list):
        print(
            json.dumps({"ok": False, "error": "expected {tasks: [...]} or [...]"}),
            file=sys.stderr,
        )
        return 1
    allowed_kinds = {"bug", "refactor", "safety", "test", "doc", "chore"}
    now_ts = _now_iso()
    existing_ids = {t.get("id") for t in state.get("backlog", [])}
    new_tasks = []
    skipped = []
    for i, t in enumerate(tasks):
        if not isinstance(t, dict):
            print(
                json.dumps({"ok": False, "error": f"task[{i}] is not an object"}),
                file=sys.stderr,
            )
            return 1
        tid = t.get("id")
        title = t.get("title")
        prio = t.get("priority")
        kind = t.get("kind")
        if not tid or not isinstance(tid, str):
            print(json.dumps({"ok": False, "error": f"task[{i}].id missing/invalid"}), file=sys.stderr)
            return 1
        if not title or not isinstance(title, str):
            print(json.dumps({"ok": False, "error": f"task[{i}].title missing/invalid"}), file=sys.stderr)
            return 1
        if not isinstance(prio, int):
            print(json.dumps({"ok": False, "error": f"task[{i}].priority not int"}), file=sys.stderr)
            return 1
        if kind not in allowed_kinds:
            print(
                json.dumps({"ok": False, "error": f"task[{i}].kind {kind!r} not in {sorted(allowed_kinds)}"}),
                file=sys.stderr,
            )
            return 1
        if tid in existing_ids:
            skipped.append(tid)
            continue
        new_task = {
            "id": tid,
            "title": title,
            "priority": prio,
            "kind": kind,
            "created_at": now_ts,
            "status": "pending",
        }
        _attach_optional_task_fields(new_task, t)
        new_tasks.append(new_task)
        existing_ids.add(tid)
    state.setdefault("backlog", []).extend(new_tasks)
    state.setdefault("logs", []).append({
        "ts": now_ts,
        "event": "bulk_add_tasks",
        "added": [t["id"] for t in new_tasks],
        "skipped_duplicates": skipped,
        "source": str(src),
    })
    _atomic_write(path, state)
    print(json.dumps({
        "ok": True,
        "added": len(new_tasks),
        "skipped": skipped,
        "ids": [t["id"] for t in new_tasks],
    }))
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="lapsphere_state.py",
        description="Atomic state helper for the lapsphere manager.",
    )
    p.add_argument(
        "--path",
        default=str(DEFAULT_STATE_PATH),
        help="Path to state.json (default: $LAPSPHERE_STATE or .hermes/state.json)",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("init", help="Ensure state file exists (idempotent).")
    sub.add_parser("show", help="Print current state as JSON.")

    sp = sub.add_parser("set-phase", help="Mutate the `phase` field.")
    sp.add_argument("phase")

    sp = sub.add_parser("add-task", help="Append a task to the backlog.")
    sp.add_argument("--id", required=True)
    sp.add_argument("--title", required=True)
    sp.add_argument("--prio", required=True, help="Lower = more urgent.")
    sp.add_argument(
        "--kind",
        required=True,
        choices=["bug", "refactor", "safety", "test", "doc", "chore"],
    )
    sp.add_argument(
        "--target-file",
        default=None,
        help="Optional: path to the source file the task targets (preserved through pop-task).",
    )
    sp.add_argument(
        "--doc-output",
        default=None,
        help="Optional: target path for the deliverable doc (preserved through pop-task).",
    )
    sp.add_argument(
        "--acceptance",
        default=None,
        help="Optional: free-form acceptance criteria string (preserved through pop-task).",
    )

    sp = sub.add_parser(
        "pop-task",
        help="Pop the highest-priority pending task; empty stdout if none.",
    )
    sp.add_argument(
        "--reap-stale",
        type=int,
        default=None,
        metavar="SECONDS",
        help="Re-queue in_flight tasks older than N seconds (e.g. 180 = 3-min budget).",
    )

    sp = sub.add_parser("complete-task", help="Move a task to completed_tasks.")
    sp.add_argument("--id", required=True)
    sp.add_argument(
        "--status",
        default="done",
        choices=["done", "blocked", "wontfix"],
    )

    sp = sub.add_parser("add-log", help="Append a timestamped log entry.")
    sp.add_argument("--message", required=True)

    sp = sub.add_parser(
        "add-tasks",
        help="Bulk-load tasks from a JSON file (replaces 10+ add-task calls).",
    )
    sp.add_argument(
        "--from",
        dest="from_file",
        required=True,
        metavar="PATH",
        help="JSON file with shape {tasks: [{id, title, priority, kind}, ...]}",
    )

    return p


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    handlers = {
        "init": cmd_init,
        "show": cmd_show,
        "set-phase": cmd_set_phase,
        "add-task": cmd_add_task,
        "add-tasks": cmd_add_tasks,
        "pop-task": cmd_pop_task,
        "complete-task": cmd_complete_task,
        "add-log": cmd_add_log,
    }
    return handlers[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
