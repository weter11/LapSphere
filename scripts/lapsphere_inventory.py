#!/usr/bin/env python3
"""LapSphere repository inventory.

Deterministic, no LLM, no network. Runs in well under 100ms on the
lapsphere repo (daemon/gui/common workspace, ~16k LOC, no target/).

Output: a single JSON file at .hermes/inventory.json (relative to cwd
unless --output is given). Stable key order, no timestamps in the
payload itself — only the file mtime tells you when the inventory was
taken. The agent is expected to read this file and never re-walk the
filesystem; if a key is missing, that's a bug to fix here, not in the
prompt.

Subcommands:
    (no args)        write inventory to .hermes/inventory.json and print a
                     one-line summary on stdout
    --output PATH    write to PATH instead
    --print          print inventory to stdout instead of writing a file
                     (useful for `hermes cron create --script` injection)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path


# Files / dirs to never recurse into. Same exclusions as `cargo` and
# `git status` by convention.
SKIP_DIRS = {
    "target", ".git", "node_modules", "__pycache__", ".cache",
    "Cargo.lock.tmp", "vendor", "third_party",
}

# Per-file extensions that count as "source".
SOURCE_EXTS = {".rs", ".py", ".sh", ".md", ".toml", ".yml", ".yaml", ".json"}


def _is_excluded(path: Path) -> bool:
    return any(part in SKIP_DIRS for part in path.parts)


def _walk_sources(root: Path) -> list[Path]:
    out: list[Path] = []
    for dirpath, dirnames, filenames in os.walk(root):
        # Prune excluded dirs in-place so os.walk skips them.
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        for fn in filenames:
            p = Path(dirpath) / fn
            if p.suffix in SOURCE_EXTS:
                out.append(p)
    return out


def _read_toml(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def _parse_workspace_members(root: Path) -> list[str]:
    """Return the list of workspace member crate dirs from root Cargo.toml.

    Naive parser — handles `members = ["a", "b", "c"]` only. LapSphere's
    Cargo.toml uses exactly this form, so we don't pull in a toml lib.
    """
    text = _read_toml(root / "Cargo.toml")
    m = re.search(r"members\s*=\s*\[([^\]]*)\]", text)
    if not m:
        return []
    return [s.strip().strip('"').strip("'") for s in m.group(1).split(",") if s.strip()]


def _parse_dependencies(crate_dir: Path) -> dict[str, list[str]]:
    """Pull [dependencies] and [dev-dependencies] from a crate Cargo.toml.

    Returns two lists of `name = "version"` strings (preserves formatting
    for the agent's reference).
    """
    text = _read_toml(crate_dir / "Cargo.toml")
    sections: dict[str, list[str]] = {"dependencies": [], "dev-dependencies": [], "build-dependencies": []}
    current = None
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            name = stripped.strip("[]").strip()
            current = name if name in sections else None
        elif current and "=" in stripped and not stripped.startswith("#"):
            sections[current].append(stripped)
    return sections


def _doc_coverage(crate_src: Path) -> dict[str, object]:
    """Crude /// doc-comment coverage: doc lines vs pub items.

    A "pub item" is one of: `pub fn | pub struct | pub enum | pub trait |
    pub type | pub const | pub static | pub mod | pub use`. Doc lines are
    lines starting with `///` (excluding `//!` inner docs).
    """
    if not crate_src.is_dir():
        return {"pub_items": 0, "doc_lines": 0, "ratio_pct": None, "files": {}}
    pub_item_re = re.compile(r"^\s*pub\s+(fn|struct|enum|trait|type|const|static|mod|use)\s")
    doc_re = re.compile(r"^\s*///")
    files: dict[str, dict[str, int]] = {}
    total_items = 0
    total_docs = 0
    for rs in sorted(crate_src.rglob("*.rs")):
        if _is_excluded(rs):
            continue
        items = docs = 0
        for line in rs.read_text(encoding="utf-8", errors="replace").splitlines():
            if pub_item_re.match(line):
                items += 1
            if doc_re.match(line):
                docs += 1
        if items or docs:
            rel = str(rs.relative_to(crate_src.parent))
            files[rel] = {"items": items, "doc_lines": docs}
            total_items += items
            total_docs += docs
    ratio = round(total_docs / total_items * 100, 1) if total_items else None
    return {"pub_items": total_items, "doc_lines": total_docs, "ratio_pct": ratio, "files": files}


def _find_systemd_units(root: Path) -> list[str]:
    out: list[str] = []
    for p in root.rglob("*.service"):
        if not _is_excluded(p):
            out.append(str(p.relative_to(root)))
    return sorted(out)


def _extract_sysfs_paths(root: Path) -> list[dict[str, str]]:
    """Find literal "/sys/..." string literals in Rust source.

    Returns [{file, line, path}, ...] sorted by path. The agent uses this
    to scope hardware-control patches.
    """
    out: list[dict[str, str]] = []
    pat = re.compile(r'"(/sys/[A-Za-z0-9_/.\-{}\$]+)"')
    for rs in (root / "daemon").rglob("*.rs") if (root / "daemon").is_dir() else []:
        if _is_excluded(rs):
            continue
        try:
            text = rs.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for lineno, line in enumerate(text.splitlines(), 1):
            for m in pat.finditer(line):
                out.append({"file": str(rs.relative_to(root)), "line": lineno, "path": m.group(1)})
    # Dedupe by (file, line, path) preserving order
    seen = set()
    uniq = []
    for r in out:
        k = (r["file"], r["line"], r["path"])
        if k not in seen:
            seen.add(k)
            uniq.append(r)
    return sorted(uniq, key=lambda r: r["path"])


def _extract_dbus_names(root: Path) -> list[str]:
    """Pull D-Bus bus/interface names from Rust source and .service files."""
    out: set[str] = set()
    for p in root.rglob("*.rs"):
        if _is_excluded(p):
            continue
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for m in re.finditer(r'"(io\.[A-Za-z0-9_.]+|org\.[A-Za-z0-9_.]+)"', text):
            out.add(m.group(1))
    for p in root.rglob("*.service"):
        if _is_excluded(p):
            continue
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for m in re.finditer(r'(io\.[A-Za-z0-9_.]+|org\.[A-Za-z0-9_.]+)', text):
            out.add(m.group(1))
    return sorted(out)


def _git_head(root: Path) -> str | None:
    head = root / ".git" / "HEAD"
    if not head.is_file():
        return None
    try:
        return head.read_text(encoding="utf-8", errors="replace").strip()
    except OSError:
        return None


def build_inventory(root: Path) -> dict[str, object]:
    members = _parse_workspace_members(root)
    crates: dict[str, object] = {}
    for m in members:
        cdir = root / m
        if not cdir.is_dir():
            continue
        deps = _parse_dependencies(cdir)
        cov = _doc_coverage(cdir / "src")
        loc = 0
        for rs in (cdir / "src").rglob("*.rs") if (cdir / "src").is_dir() else []:
            try:
                loc += sum(1 for _ in rs.open("rb"))
            except OSError:
                pass
        crates[m] = {
            "path": m,
            "rust_loc": loc,
            "doc_coverage": cov,
            "dependencies": deps["dependencies"],
            "dev_dependencies": deps["dev-dependencies"],
            "build_dependencies": deps["build-dependencies"],
        }

    # Workspace-level deps
    workspace_deps_text = ""
    wtoml = _read_toml(root / "Cargo.toml")
    m = re.search(r"\[workspace\.dependencies\](.*?)(?=\n\[|$)", wtoml, re.DOTALL)
    if m:
        workspace_deps_text = m.group(1).strip()

    return {
        "schema_version": 1,
        "root": str(root),
        "git_head": _git_head(root),
        "workspace": {
            "members": members,
            "shared_dependencies": workspace_deps_text,
        },
        "crates": crates,
        "systemd_units": _find_systemd_units(root),
        "sysfs_paths": _extract_sysfs_paths(root),
        "dbus_names": _extract_dbus_names(root),
        "docs_inventory": sorted(
            str(p.relative_to(root)) for p in root.rglob("*.md")
            if not _is_excluded(p)
        ),
    }


# Modules whose audit feeds directly into hardware safety / IPC contract.
# Used both to flag P1 in the reference catalog and to seed the active
# backlog with 4-5 highest-risk doc tasks. Explicit set (not substring
# matching) so we don't accidentally tag a random file with "power" in
# the path.
CRITICAL_MODULES = {
    "hardware_control", "hardware_detection", "tuxedo_io",
    "battery_control", "dbus_interface", "sysfs",
    "fan_curve", "fan_curve_editor",
}


# Standard acceptance checklist for a component-audit doc task. Workers
# are expected to deliver against every bullet — see
# docs/components/README.md (to be created by the first such task).
ACCEPTANCE_CHECKLIST = (
    "1) Module responsibility & architecture (purpose, callers, data flow). "
    "2) Full API/struct breakdown (every pub item: signature, invariants, error paths). "
    "3) Panic & unwrap() hazard audit (list every .unwrap()/.expect()/panic!/unreachable!, "
    "classify Safe/Bounds/Unsafe, propose safer pattern). "
    "4) Hardware/IPC side-effects (sysfs writes, D-Bus method emissions, files touched). "
    "Extract any newly found bug/refactor tasks into state.json via lapsphere_state.py add-task."
)


def _module_slug(rs_file: Path, crate: str) -> tuple[str, str, str]:
    """Return (id, doc_rel_path, module_label) for a .rs file.

    ID slug is collision-proof: walks the file's path under the crate
    and joins non-trivial segments. The bare `mod.rs` stem is
    represented by its parent directory (`gui::widgets` rather than
    `gui::mod`).

    Examples (crate, path) -> (id, doc_rel_path, label):
      ('daemon', 'daemon/src/hardware_control.rs') ->
          ('doc-daemon-hardware_control',
           'docs/components/daemon/hardware_control.md',
           'daemon::hardware_control')
      ('gui', 'gui/src/widgets/fan_curve_editor.rs') ->
          ('doc-gui-widgets-fan_curve_editor',
           'docs/components/gui/widgets/fan_curve_editor.md',
           'gui::widgets::fan_curve_editor')
      ('gui', 'gui/src/widgets/mod.rs') ->
          ('doc-gui-widgets',
           'docs/components/gui/widgets.md',
           'gui::widgets')
    """
    parts = rs_file.parts
    crate_idx = parts.index(crate)
    # Path under crate/src/, e.g. ('widgets', 'fan_curve_editor.rs')
    under_src = parts[crate_idx + 2:]  # +2 skips "<crate>" and "src"
    stem = rs_file.stem
    if stem == "mod" and len(under_src) >= 2:
        # mod.rs re-exports — represent by the parent dir name
        parent = under_src[-2]
        slug_parts = [crate, parent]
        doc_rel = f"docs/components/{crate}/{parent}.md"
        label = f"{crate}::{parent}"
    elif stem == "mod":
        # bare src/mod.rs — represents the crate root
        slug_parts = [crate]
        doc_rel = f"docs/components/{crate}.md"
        label = crate
    else:
        slug_parts = [crate] + list(under_src[:-1]) + [stem]
        doc_rel = (
            f"docs/components/{crate}/"
            + "/".join(under_src[:-1] + (stem + ".md",))
        )
        if under_src[:-1]:
            label = f"{crate}::" + "::".join(under_src[:-1] + (stem,))
        else:
            label = f"{crate}::{stem}"
    slug_parts = [p for p in slug_parts if p]  # drop empties
    task_id = "doc-" + "-".join(slug_parts)
    return task_id, doc_rel, label


def build_component_audit_catalog(root: Path) -> list[dict[str, str | int]]:
    """Walk every crate's src/ and emit one audit entry per .rs file.

    Always emits the full reference catalog (every module) regardless of
    how many are selected for the active backlog. The 4-5 selection is
    done in `suggest_backlog`, which reads the catalog and picks the
    CRITICAL_MODULES entries.
    """
    members = _parse_workspace_members(root)
    catalog: list[dict[str, str | int]] = []
    seen_ids: set[str] = set()
    for crate in members:
        src_dir = root / crate / "src"
        if not src_dir.is_dir():
            continue
        for rs in sorted(src_dir.rglob("*.rs")):
            if _is_excluded(rs):
                continue
            task_id, doc_rel, label = _module_slug(rs, crate)
            # Defensive: if a future path produces a duplicate id, suffix it
            suffix = 1
            base_id = task_id
            while task_id in seen_ids:
                suffix += 1
                task_id = f"{base_id}-{suffix}"
            seen_ids.add(task_id)
            rel_path = str(rs.relative_to(root))
            # Priority: P1 if the module's stem (last path component) or
            # any parent dir matches CRITICAL_MODULES; else P2. This is
            # explicit-set matching, not substring — so we don't tag
            # unrelated files just because the path mentions "power".
            stem = rs.stem
            parent_stems = {p.name for p in rs.relative_to(src_dir).parents} | {stem}
            is_critical = bool(parent_stems & CRITICAL_MODULES)
            priority = 1 if is_critical else 2
            catalog.append({
                "id": task_id,
                "title": f"[Doc & Audit] Exhaustive analysis of {label}",
                "category": "docs",
                "priority": priority,
                "kind": "doc",
                "target_file": rel_path,
                "doc_output": doc_rel,
                "acceptance": ACCEPTANCE_CHECKLIST,
            })
    return catalog


def _seed_critical_module_tasks(
    catalog: list[dict[str, str | int]],
    tasks: list[dict[str, object]],
    used_ids: set[str],
) -> int:
    """Mix in 4-5 critical-module doc tasks from the component catalog.

    Sorts the catalog so P1 critical modules come first, then
    round-robins by crate so we don't get 3 daemon tasks in a row.
    Caps at 5 to leave room for the existing heuristics' output. Adds
    directly to `tasks` and registers ids in `used_ids` so later
    heuristics don't double-add.
    """
    # Stable sort: critical first, then by crate for variety
    crit = [c for c in catalog if c.get("priority") == 1]
    by_crate: dict[str, list[dict[str, str | int]]] = {}
    for c in crit:
        # Crate is the first segment of the id: "doc-<crate>-..."
        tid = c["id"]
        assert isinstance(tid, str)
        crate = tid.split("-", 2)[1]
        by_crate.setdefault(crate, []).append(c)
    round_robin: list[dict[str, str | int]] = []
    crates_in_order = sorted(by_crate.keys())
    while any(by_crate.values()) and len(round_robin) < 5:
        for crate in crates_in_order:
            bucket = by_crate[crate]
            if bucket:
                round_robin.append(bucket.pop(0))
                if len(round_robin) >= 5:
                    break
    # Cap seeded critical-module doc tasks at P2 regardless of the
    # catalog label. The catalog itself still marks these modules P1
    # (informational), but in the active queue they interleave with
    # code-bug/safety tasks rather than dominating the first 5 ticks.
    SEEDED_PRIORITY_CAP = 2
    added = 0
    for c in round_robin:
        cid = c["id"]
        if not isinstance(cid, str) or cid in used_ids:
            continue
        seed_prio = c["priority"]
        # If the catalog label is P1, still emit as P2 (interleaving).
        # P3+ labels stay as-is (won't normally happen — catalog is P1/P2).
        if isinstance(seed_prio, int) and seed_prio < SEEDED_PRIORITY_CAP:
            emitted_prio = SEEDED_PRIORITY_CAP
        else:
            emitted_prio = seed_prio
        tasks.append({
            "id": cid,
            "title": c["title"],
            "priority": emitted_prio,
            "kind": c["kind"],
            "target_file": c.get("target_file"),
            "doc_output": c.get("doc_output"),
            "acceptance": c.get("acceptance"),
        })
        used_ids.add(cid)
        added += 1
    return added


def suggest_backlog(
    inv: dict[str, object],
    catalog: list[dict[str, str | int]] | None = None,
) -> list[dict[str, object]]:
    """Generate a 8-12 task backlog derived from the inventory.

    Deterministic, no LLM. The cron agent's job is to review and write
    this list (with edits) to a JSON file for bulk-load. This removes
    the per-task synthesis step that was blowing the agent's context.

    Heuristics (in priority order):
      - Lowest-coverage crate first (P1 docs tasks per under-covered pub item)
      - Highest-LOC crate's untested file gets a P2 test task
      - Sysfs write paths get a P1 safety task (handles hardware directly)
      - D-Bus interface file gets a P2 doc + P2 test task
      - Dedupe by id
    """
    tasks: list[dict[str, object]] = []
    used_ids: set[str] = set()

    # Mix in critical-module doc tasks first so the existing heuristics
    # (which dedupe by id) see them as already-present. Catalog is
    # optional so unit tests / older callers that pass only `inv` still
    # work; in that case the seeder is a no-op.
    if catalog:
        _seed_critical_module_tasks(catalog, tasks, used_ids)

    def add(prio: int, kind: str, slug: str, title: str) -> None:
        tid = f"task-{slug}"
        if tid in used_ids:
            return
        used_ids.add(tid)
        tasks.append({
            "id": tid,
            "title": title,
            "priority": prio,
            "kind": kind,
        })

    crates = inv.get("crates", {}) or {}
    # Sort by doc coverage ascending (worst first), break ties by LOC desc.
    by_coverage = sorted(
        crates.items(),
        key=lambda kv: (kv[1].get("doc_coverage", {}).get("ratio_pct") or 0, -(kv[1].get("rust_loc") or 0)),
    )
    for name, c in by_coverage:
        cov = c.get("doc_coverage", {})
        ratio = cov.get("ratio_pct")
        pub_items = cov.get("pub_items", 0)
        loc = c.get("rust_loc", 0)
        if ratio is None:
            continue
        if ratio < 20 and pub_items > 20:
            add(
                1, "doc",
                f"doc-{name}-crate",
                f"Add doc comments to all {pub_items} public items in the `{name}` crate "
                f"(currently {ratio:.0f}% coverage).",
            )
        elif ratio < 60 and pub_items > 10:
            add(
                2, "doc",
                f"doc-{name}-partial",
                f"Raise doc coverage in `{name}` from {ratio:.0f}% to >=80% on the most-used public items.",
            )
        # Highest-LOC crate gets a test scaffold
        if loc >= 5000:
            add(
                2, "test",
                f"test-{name}-scaffold",
                f"Add unit-test scaffolding for the `{name}` crate ({loc} LOC, 0 integration tests today).",
            )

    sysfs_paths = inv.get("sysfs_paths", []) or []
    write_paths = [
        p for p in sysfs_paths
        if any(token in p.get("path", "") for token in ("/pwm", "/power", "/boost", "/cpufreq", "/backlight", "/governor"))
    ]
    if write_paths:
        add(
            1, "safety",
            "safety-sysfs-writes",
            f"Add an allowlist + range check before the daemon writes to any of the "
            f"{len(write_paths)} /sys write paths (cpufreq, pwm, backlight, boost).",
        )
    if len(sysfs_paths) >= 50:
        add(
            3, "refactor",
            "refactor-sysfs-table",
            f"Move the {len(sysfs_paths)}-row sysfs path inventory from a hard-coded list into "
            f"a data-driven table loaded at startup.",
        )

    dbus_names = inv.get("dbus_names", []) or []
    if any("io.lapsphere" in n for n in dbus_names):
        # Skip the generic P2 doc task if the catalog already emitted a
        # specific critical-module doc task for the daemon's D-Bus
        # interface (the worker would otherwise redo the same work).
        specific_dbus_doc = "doc-daemon-dbus_interface" in used_ids
        if not specific_dbus_doc:
            add(
                2, "doc",
                "doc-dbus-interface",
                f"Document the D-Bus interface contract for {', '.join(n for n in dbus_names if 'io.lapsphere' in n)} "
                f"in daemon/src/dbus_interface.rs.",
            )
        add(
            2, "test",
            "test-dbus-interface",
            f"Add a D-Bus mock-client test that exercises each method on the "
            f"{', '.join(n for n in dbus_names if 'io.lapsphere' in n)} interface.",
        )

    units = inv.get("systemd_units", []) or []
    if units:
        add(
            3, "test",
            "test-systemd-units",
            f"Add a `systemd-analyze verify` check to CI for the {len(units)} service units.",
        )

    return tasks


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="lapsphere_inventory.py")
    p.add_argument("--root", default=".", help="repo root (default: cwd)")
    p.add_argument("--output", default=".hermes/inventory.json", help="output path (relative to root)")
    p.add_argument("--print", action="store_true", help="print JSON to stdout instead of writing a file")
    p.add_argument(
        "--suggest-backlog",
        action="store_true",
        help="Write a 8-12 task backlog suggestion to .hermes/backlog.suggested.json "
        "(consumed by lapsphere_state.py add-tasks --from).",
    )
    p.add_argument(
        "--suggest-component-audit",
        action="store_true",
        help="Write the full per-module audit reference catalog to "
        ".hermes/component-audit-backlog.json (one entry per .rs file, "
        "with target_file/doc_output/acceptance). The same catalog is "
        "automatically mixed into --suggest-backlog output as 4-5 "
        "critical-module doc tasks.",
    )
    args = p.parse_args(argv)

    root = Path(args.root).resolve()
    if not (root / "Cargo.toml").is_file():
        print(f"error: no Cargo.toml at {root}/Cargo.toml — is this a LapSphere checkout?", file=sys.stderr)
        return 1
    inv = build_inventory(root)
    payload = json.dumps(inv, indent=2, sort_keys=False) + "\n"

    if args.suggest_backlog or args.suggest_component_audit:
        catalog: list[dict[str, str | int]] = build_component_audit_catalog(root)
    else:
        catalog = []

    if args.suggest_component_audit:
        out = root / ".hermes" / "component-audit-backlog.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "root": str(root),
                    "generated_at": None,  # mtime is the source of truth; no timestamps in payload
                    "module_count": len(catalog),
                    "critical_modules": sorted(CRITICAL_MODULES),
                    "tasks": catalog,
                },
                indent=2,
            ) + "\n",
            encoding="utf-8",
        )
        n_crit = sum(1 for c in catalog if c.get("priority") == 1)
        print(
            f"Component-audit catalog written to {out.relative_to(root)} "
            f"({len(catalog)} modules, {n_crit} critical)"
        )
        return 0

    if args.suggest_backlog:
        backlog = suggest_backlog(inv, catalog=catalog)
        out = root / ".hermes" / "backlog.suggested.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps({"tasks": backlog}, indent=2) + "\n", encoding="utf-8")
        print(f"Backlog suggestion written to {out.relative_to(root)} ({len(backlog)} tasks)")
        return 0

    if args.print:
        sys.stdout.write(payload)
        return 0

    out = Path(args.output)
    if not out.is_absolute():
        out = root / out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(payload, encoding="utf-8")
    size = out.stat().st_size
    n_crates = len(inv.get("crates", {}))
    n_sysfs = len(inv.get("sysfs_paths", []))
    n_dbus = len(inv.get("dbus_names", []))
    n_units = len(inv.get("systemd_units", []))
    print(f"Inventory written to {out.relative_to(root)} ({size} bytes); "
          f"crates={n_crates} sysfs_paths={n_sysfs} dbus_names={n_dbus} systemd_units={n_units}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
