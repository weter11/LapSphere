# Modules

> STATUS: NOT YET POPULATED
> See `docs/architecture/overview.md` for the scaffold convention. Agents: this is not an error — proceed with extra caution and say so.

## What belongs here once populated
For each module/crate (daemon, gui, common, nvidia, drivers_src):
- Responsibility (what it owns, what it explicitly does not own)
- Dependencies in and out
- Notable internal boundaries (e.g. `hardware_detection.rs` vs `hardware_control.rs`)

## Maintenance
Update when a module's responsibility or dependency shape actually changes.
