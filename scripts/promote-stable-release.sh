#!/usr/bin/env bash
set -Eeuo pipefail

: "${GH_TOKEN:?missing workflow token}"
: "${GITHUB_REPOSITORY:?missing repository}"
: "${RELEASE_VERSION:?missing release version}"

[[ "$RELEASE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo "Invalid RELEASE_VERSION: $RELEASE_VERSION" >&2
  exit 1
}

tag="v$RELEASE_VERSION"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

meta="$tmp/release.json"
gh release view "$tag" --json tagName,isDraft,isPrerelease,targetCommitish,url,assets > "$meta"

[[ "$(jq -r '.isDraft' "$meta")" == "false" ]] || {
  echo "$tag is still a draft" >&2
  exit 1
}

gh release download "$tag" -p build-info.txt -p SHA256SUMS -D "$tmp"

source_sha="$(sed -n 's/^source_commit=//p' "$tmp/build-info.txt" | head -n1)"
run_id="$(sed -n 's/^workflow_run=//p' "$tmp/build-info.txt" | head -n1)"
target="$(jq -r '.targetCommitish' "$meta")"

[[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || { echo "Invalid source_commit in build-info.txt" >&2; exit 1; }
[[ "$run_id" =~ ^[0-9]+$ ]] || { echo "Invalid workflow_run in build-info.txt" >&2; exit 1; }
[[ "$target" == "$source_sha" ]] || {
  echo "Release target $target does not match built source $source_sha" >&2
  exit 1
}

run_json="$tmp/run.json"
gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run_id" > "$run_json"
[[ "$(jq -r '.conclusion' "$run_json")" == "success" ]] || {
  echo "Build workflow $run_id was not successful" >&2
  exit 1
}
[[ "$(jq -r '.head_sha' "$run_json")" == "$source_sha" ]] || {
  echo "Build workflow source does not match release source" >&2
  exit 1
}

assets="$(jq -r '.assets[].name' "$meta")"
for required in   "AI-Light-$RELEASE_VERSION-windows-x64-setup.exe"   "ai-light-agent-$RELEASE_VERSION-linux-x86_64.tar.gz"   "ai-light-$RELEASE_VERSION-source.tar.gz"   "SHA256SUMS"   "build-info.txt"   "Cargo.lock"   "LICENSE"   "THIRD-PARTY-NOTICES.md"
do
  grep -Fxq "$required" <<<"$assets" || {
    echo "Missing required release asset: $required" >&2
    exit 1
  }
done

grep -Fq "AI-Light-$RELEASE_VERSION-windows-x64-setup.exe" "$tmp/SHA256SUMS"
grep -Fq "ai-light-agent-$RELEASE_VERSION-linux-x86_64.tar.gz" "$tmp/SHA256SUMS"
grep -Fq "ai-light-$RELEASE_VERSION-source.tar.gz" "$tmp/SHA256SUMS"

cat > "$tmp/notes.md" <<EOF
# AI Light $RELEASE_VERSION

Stable release of the Rust/Tauri Windows status-light controller and Rust Ubuntu Codex relay.

## Downloads

- Windows x64: `AI-Light-$RELEASE_VERSION-windows-x64-setup.exe`
- Ubuntu 24.04 x86_64 agent: `ai-light-agent-$RELEASE_VERSION-linux-x86_64.tar.gz`
- Source archive: `ai-light-$RELEASE_VERSION-source.tar.gz`
- Integrity: verify files with `SHA256SUMS`

## Highlights

- Yellow breathing while work is active, steady yellow while waiting, green completion state, and red error breathing.
- Configurable buzzer reminders with low-battery sound protection.
- BLE battery percentage and HID battery voltage display; no charging-state inference.
- Evidence-based Codex session lifecycle tracking.
- Unverified legacy records no longer masquerade as active work or block a real task's completion.
- Real/unknown subagents are retained; only the explicitly identified `thread_title` helper is treated as synthetic.
- Atomic Ubuntu agent upgrade preserves running Codex tasks, hooks, client settings, and state.

## Verification

Source commit: `$source_sha`

GitHub Actions run: https://github.com/$GITHUB_REPOSITORY/actions/runs/$run_id

The release pipeline completed Linux tests, native Windows tests, Ubuntu packaging, and the Windows NSIS installer build before publishing these assets.

**Unsigned stable release.** The Windows installer is not commercially code-signed. CI does not replace physical BLE/hardware validation. No vendor executable or firmware is included. Verify `SHA256SUMS` and keep normal Windows security protections enabled.
EOF

if [[ "$(jq -r '.isPrerelease' "$meta")" == "true" ]]; then
  gh release edit "$tag" --prerelease=false --latest --notes-file "$tmp/notes.md"
else
  gh release edit "$tag" --latest --notes-file "$tmp/notes.md"
fi

after="$tmp/after.json"
gh release view "$tag" --json isDraft,isPrerelease,isLatest,url,targetCommitish > "$after"
[[ "$(jq -r '.isDraft' "$after")" == "false" ]]
[[ "$(jq -r '.isPrerelease' "$after")" == "false" ]]
[[ "$(jq -r '.isLatest' "$after")" == "true" ]]
[[ "$(jq -r '.targetCommitish' "$after")" == "$source_sha" ]]

echo "Promoted $tag to stable latest release without replacing build assets."
jq -r '.url' "$after"
