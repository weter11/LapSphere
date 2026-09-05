#!/usr/bin/env bash
# Thin wrapper so cron --script (which expects .sh/.bash via bash) can run
# the inventory. Pure delegation; no logic here. The .py is the real worker.
#
# When invoked by the cron scheduler, the CWD is the script's install
# location (typically ~/.hermes/scripts/), not the job's --workdir. We
# accept the workdir via $1 (default: ascend until we find Cargo.toml).
# Extra CLI flags are intentionally NOT accepted: this wrapper always
# emits the full inventory artifact set (see below). Cron jobs that
# need different behavior should call the .py directly.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

# Discover repo root by walking up from CWD until we find Cargo.toml.
# Falls back to $1 if provided, falls back to /home/wer/devis/lapsphere/repo
# (this script's primary deployment target on weter11's box).
discover_root() {
  if [ -n "${1:-}" ] && [ -f "$1/Cargo.toml" ]; then
    (cd "$1" && pwd)
    return
  fi
  local d="$PWD"
  while [ "$d" != "/" ]; do
    if [ -f "$d/Cargo.toml" ]; then
      echo "$d"
      return
    fi
    d="$(dirname "$d")"
  done
  echo "/home/wer/devis/lapsphere/repo"
}

# $1 may be a repo path OR a flag like --suggest-backlog. If it looks
# like a path with Cargo.toml, use it as the repo; otherwise treat as
# a flag.
first="${1:-}"
if [ -n "$first" ] && [ -d "$first" ] && [ -f "$first/Cargo.toml" ]; then
  REPO_ROOT="$(discover_root "$first")"
  shift
else
  REPO_ROOT="$(discover_root "")"
fi

# Always write the inventory artifacts the day-start prompt depends on:
#   * .hermes/inventory.json               (full repo scan)
#   * .hermes/component-audit-backlog.json (per-module reference catalog;
#     NOT loaded into state.json — workers read it on-demand)
#   * .hermes/backlog.suggested.json       (8-13 mixed tasks for
#     add-tasks --from; 4-5 are critical-module doc tasks interleaved
#     with code/safety/test tasks)
#
# The catalog and the suggested backlog are both deterministic from the
# inventory and idempotent, so there's no harm in always emitting them.
# Day-start reads .hermes/backlog.suggested.json; hourly-tick and
# day-end ignore all three. We run --suggest-component-audit first
# (writes the catalog sidecar) and then --suggest-backlog (consumes the
# same in-memory catalog via the second python invocation's filesystem
# walk — cheap, <100ms on this repo).
PY="$SCRIPT_DIR/lapsphere_inventory.py"
python3 "$PY" --root "$REPO_ROOT" --suggest-component-audit
python3 "$PY" --root "$REPO_ROOT" --suggest-backlog
