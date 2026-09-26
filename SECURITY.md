# Security

AI Light is a local status utility, not an alarm, medical monitor or guaranteed task-verification device. Bluetooth write success and low-battery detection are best-effort software signals.

Use the HTTP receiver only on a trusted host/VM network. It has Bearer authentication, source-IP filtering, request-size limits and no browser-origin allowance, but plain HTTP is not encrypted. Do not expose port17322 to the Internet. Use a trusted encrypted tunnel for untrusted networks.

Configuration is stored per-user; exported client.json contains a secret. Anyone with access to the user's local files may be able to read it. Never post the token or unsanitized diagnostics in public issues. Diagnostics can contain device identifiers, usernames, paths and local IPs even when the token is omitted.

Do not publish exploit details or secrets in an initial issue. Report a brief non-sensitive summary through the repository's private vulnerability-reporting feature if available; otherwise open an issue requesting a private contact method before sharing details. No security response SLA is claimed.

Releases are unsigned unless explicitly stated otherwise. Verify repository/release origin and SHA256SUMS; checksums detect file changes but are not a substitute for trusted publisher signatures. Do not disable Windows Defender/SmartScreen or TLS verification to install/build.
