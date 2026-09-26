# Contributing

Issues and pull requests are welcome. Keep device-specific protocol changes separate from UI changes and state-machine changes.

Before submitting:

```bash
cargo test -p light-core -p light-agent --locked
node --test tests/*.test.mjs
node scripts/check-ui.mjs
```

Include your platform and package/firmware versions and explain whether results are hardware-observed, inferred, simulated, or documented. Add tests for new frame layouts, state transitions and timeout behavior. Preserve Windows/Ubuntu separation and do not put model calls inside notification hooks.

Never commit exported client.json, secrets.json, a user's Bluetooth address, private logs, vendor executables/firmware, certificates or signing keys. Keep protocol output allowlisted; no arbitrary register/firmware writes. Do not add remote shell capabilities to the desktop webview.

Original contributions are submitted under the repository MIT license. Third-party code/assets need compatible licensing and attribution. Automated license collection does not substitute for reviewing new dependencies.
