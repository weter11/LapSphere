# Architecture Overview

> STATUS: NOT YET POPULATED
> This file is part of the architecture-archaeology scaffold (see `.github/agents/my-agent.agent.md`, category 4). No verified system model exists here yet.
> Agents: this status is not an error. Proceed with extra caution and say so explicitly in your output. Do not fabricate content to fill this in — only an Architecture Archaeology task, working from direct source inspection, should populate this file.

## What belongs here once populated
- High-level system purpose and shape
- Major components and how they relate (daemon, GUI, drivers, common, nvidia)
- Process/IPC boundaries (e.g. the DBus interface between daemon and GUI)
- Entry points

## Maintenance
Update only when an architectural fact changes (new component, changed boundary) — not on routine bug fixes. See `my-agent.agent.md` → Documentation Update Discipline.
