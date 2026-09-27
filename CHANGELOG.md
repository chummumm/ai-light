# Changelog

## 0.3.5 — Honest unverified legacy records

- Quarantine only pre-observer, unbound UserPromptSubmit/Working records after an error-free search of registered roots finds no journal. Preserve original state and timestamps; exclusion is not task success. Fresh accepted hooks or matching journal evidence restore participation.
- Unknown legacy records no longer indefinitely override real completion. UI explicitly shows unverified count and limits the meaning of green to participating tasks.
- Never filter all subagents as internal: only an explicitly identified thread_title helper is removed. Already registered real/unknown subagents remain.
- Add filesystem, state-race, projection and UI regression tests. No process interruption, state clearing, new hook registration or dependency upgrades.

## 0.3.4 — Evidence-based session lifecycle

- Reconcile already-known sessions with read-only, session/turn-correlated Codex lifecycle records; recover missed completion using original timestamps rather than replaying old green-light timers.
- Track exact Codex process identity (PID, start tick and boot ID), with conservative handling of unreadable or missing evidence. Never expire real tasks merely because they have been silent.
- Prevent late tool results, duplicate prompts and duplicate completion notifications from reopening a finished turn. Preserve legitimate continuation after a soft Stop hook.
- Remove verified auxiliary/subagent records from aggregation, expire active manual tests after 60 seconds, and collect old off/done records even when no hook arrives.
- Add an atomic `light-agent upgrade` path that preserves Codex tasks, existing hooks, connection configuration and state. Only the AI Light relay is restarted.
- Separate stored record counts from working/waiting/error/done counts in the Windows UI and add expandable per-session diagnostics.
- Add regression tests for partial journal writes, wrong IDs, concurrent state changes, PID reuse, notifications, eight-record migration and long-running silent tasks.
- See [session lifecycle and no-interruption upgrade](docs/SESSION-LIFECYCLE.md) for supported formats, privacy and remaining unknown-evidence cases.

## 0.3.3 — Reliable Codex turn completion

- Add a user-level Codex `notify` completion fallback for `agent-turn-complete`, covering environments where `Stop` is not emitted (notably affected `codex exec` versions).
- Accept completion only for a session/turn already registered by hooks; detect the current official hidden `thread_title` prompt and keep its temporary Working state from overriding the visible task.
- Preserve existing user `notify` configuration instead of overwriting it; uninstall removes only the exact AI Light line it installed.
- Keep `Stop` as the normal interactive completion path; duplicate Stop/notify delivery does not extend the five-minute green-light timer.
- Add regression tests for completion correlation, duplicate delivery, question waiting, existing notify preservation and uninstall behavior.

## 0.3.2 — Initial public pre-release

- Rust/Tauri Windows tray desktop and Rust Ubuntu Codex relay.
- Yellow/red breathing, steady waiting/completion, per-turn five-minute timer.
- Configurable sound patterns, delays, intervals, limits and relative volume.
- Low-battery/unknown-battery sound protection with recovery hysteresis.
- Standard BLE battery percentage and validated HID voltage; no charging inference.
- Public device discovery instead of a personal hard-coded Bluetooth address.
- Native Windows/Ubuntu GitHub Actions builds, immutable source/lockfile association,
  release checksums, license collection and full setup/build/troubleshooting guides.
- Deterministic Rust-generated application/tray artwork; no font or Node runtime dependency.

Earlier private prototypes and test scripts are not part of the public release history.
