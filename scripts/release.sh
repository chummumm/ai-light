#!/usr/bin/env bash
set -Eeuo pipefail
: "${GH_TOKEN:?missing workflow token}"
: "${SOURCE_SHA:?missing immutable source commit}"
: "${RELEASE_VERSION:?missing version}"
[[ "$RELEASE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid version' >&2; exit 1; }
stable="${RELEASE_STABLE:-false}"
[[ "$stable" == true || "$stable" == false ]] || { echo 'Invalid release channel' >&2; exit 1; }
channel=pre-release
[[ "$stable" != true ]] || channel=stable
tag="v$RELEASE_VERSION"
root="$PWD"; release="$root/release-assets"; mkdir -p "$release"
mapfile -t installers < <(find incoming -type f -name '*-setup.exe')
mapfile -t agents < <(find incoming -type f -name 'ai-light-agent-linux-x86_64.tar.gz')
((${#installers[@]}==1 && ${#agents[@]}==1)) || { echo 'Expected exactly one successful Windows installer and one agent archive.' >&2; exit 1; }
cp "${installers[0]}" "$release/AI-Light-$RELEASE_VERSION-windows-x64-setup.exe"
cp "${agents[0]}" "$release/ai-light-agent-$RELEASE_VERSION-linux-x86_64.tar.gz"
cp Cargo.lock LICENSE THIRD-PARTY-NOTICES.md "$release/"
git archive --format=tar.gz --prefix="ai-light-$RELEASE_VERSION/" "$SOURCE_SHA" > "$release/ai-light-$RELEASE_VERSION-source.tar.gz"
if [[ -d incoming/windows-x64/third-party ]]; then tar -czf "$release/third-party-licenses-windows.tar.gz" -C incoming/windows-x64 third-party; fi
if [[ -f incoming/ubuntu-agent/dependency-inventory.json ]]; then cp incoming/ubuntu-agent/dependency-inventory.json "$release/"; fi
printf 'source_commit=%s\nversion=%s\nrust=1.98.1\nworkflow_run=%s\n' "$SOURCE_SHA" "$RELEASE_VERSION" "${GITHUB_RUN_ID:-local}" > "$release/build-info.txt"
(cd "$release"; sha256sum -- * > SHA256SUMS)
cat > "$release/RELEASE-NOTES.md" <<EOF
# AI Light $RELEASE_VERSION ($channel)

Windows x64 tray application + Ubuntu 24.04 x64 agent, built by GitHub Actions.

- Windows: install \`AI-Light-$RELEASE_VERSION-windows-x64-setup.exe\`.
- Ubuntu: extract the agent archive and follow \`docs/GETTING-STARTED.md\`.
- Source commit: \`$SOURCE_SHA\`.
- Complete build: $GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID .

Includes configurable breathing lights, state-based sound reminders, low-battery
sound protection, measured battery percentage/voltage, tray and user login startup.

**Unsigned build.** Cloud compilation/tests are not physical BLE, sound,
battery or installer runtime certification. No vendor executable or firmware is included.
Check SHA256SUMS. Do not disable Windows security protections.
EOF
if [[ -f "docs/releases/v$RELEASE_VERSION.md" ]]; then
  printf '\n' >> "$release/RELEASE-NOTES.md"
  cat "docs/releases/v$RELEASE_VERSION.md" >> "$release/RELEASE-NOTES.md"
fi
# Never overwrite an already published release or unrelated tag.
if git show-ref --verify --quiet "refs/tags/$tag"; then
  [[ "$(git rev-parse "$tag^{commit}")" == "$SOURCE_SHA" ]] || { echo 'Existing tag points at another source commit' >&2; exit 1; }
fi
if gh release view "$tag" >/dev/null 2>&1; then
  echo "Release $tag already exists; increase the project version before publishing." >&2; exit 1
fi
gh release create "$tag" --target "$SOURCE_SHA" --title "AI Light $RELEASE_VERSION" --draft --prerelease --notes-file "$release/RELEASE-NOTES.md"
# Notes are the release body; do not upload an unchecksummed duplicate.
rm "$release/RELEASE-NOTES.md"
gh release upload "$tag" "$release"/*
if [[ "$stable" == true ]]; then
  gh release edit "$tag" --draft=false --prerelease=false --latest
else
  gh release edit "$tag" --draft=false --prerelease
fi
