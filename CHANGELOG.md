# Changelog

## 0.3.3 — Reliable Codex turn completion

- Add a user-level Codex `notify` completion fallback for `agent-turn-complete`, covering environments where `Stop` is not emitted (notably affected `codex exec` versions).
- Accept completion only for a session/turn already registered by hooks, filtering unrelated background notifications such as Codex title generation.
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
