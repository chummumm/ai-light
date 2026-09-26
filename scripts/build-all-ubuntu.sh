#!/usr/bin/env bash
# Native agent + optional Windows MSVC cross-build. No Python/Wine required.
set -Eeuo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
install=0; windows=1; agent=1; exe_only=0; jobs=2
while (($#)); do
  case "$1" in
    --install-deps) install=1;;
    --windows-only) agent=0;;
    --agent-only) windows=0;;
    --exe-only) exe_only=1;;
    --jobs) shift; jobs="${1:?missing --jobs value}";;
    -h|--help) echo 'Usage: bash scripts/build-all-ubuntu.sh [--install-deps] [--windows-only|--agent-only] [--exe-only] [--jobs N]'; exit 0;;
    *) echo "Unknown argument: $1" >&2; exit 2;;
  esac
  shift
done
[[ "$jobs" =~ ^[1-9][0-9]*$ ]] || { echo 'jobs must be a positive integer' >&2; exit 2; }
[[ "$(uname -s)" == Linux ]] || { echo 'Use this script on Linux.' >&2; exit 1; }
stamp="$(date -u +%Y%m%dT%H%M%SZ)-$$"
mkdir -p build-logs
exec > >(tee "build-logs/ubuntu-$stamp.log") 2>&1
trap 'code=$?; echo "Build failed (exit $code), line $LINENO; no release was published." >&2; exit "$code"' ERR
if ((install)); then
  apt=(apt-get); ((EUID==0)) || apt=(sudo apt-get)
  "${apt[@]}" update
  packages=(build-essential pkg-config libssl-dev ca-certificates curl file unzip xz-utils cmake ninja-build)
  ((windows==0)) || packages+=(clang clang-tools lld llvm nsis libayatana-appindicator3-dev)
  "${apt[@]}" install -y "${packages[@]}"
fi
[[ ! -f "$HOME/.cargo/env" ]] || source "$HOME/.cargo/env"
command -v cargo >/dev/null || { echo 'Install Rust via https://rustup.rs/ and restart this shell.' >&2; exit 1; }
export CARGO_BUILD_JOBS="$jobs"
rustc --version; cargo --version
[[ -f Cargo.lock ]] || cargo generate-lockfile
cargo test -p light-core -p light-agent --locked
if ((agent)); then cargo build -p light-agent --release --locked; fi
if ((windows)); then
  for dir in $(find /usr/lib -maxdepth 1 -type d -name 'llvm-*' 2>/dev/null | sort -Vr); do
    if [[ -x "$dir/bin/clang-cl" && -x "$dir/bin/lld-link" && -x "$dir/bin/llvm-rc" && -x "$dir/bin/llvm-lib" ]]; then export PATH="$dir/bin:$PATH"; break; fi
  done
  for tool in clang-cl lld-link llvm-rc llvm-lib llvm-ar; do command -v "$tool" >/dev/null || { echo "Missing $tool; run --install-deps." >&2; exit 1; }; done
  if ((!exe_only)); then pkg-config --exists ayatana-appindicator3-0.1 || { echo 'Install libayatana-appindicator3-dev.' >&2; exit 1; }; fi
  rustup target add x86_64-pc-windows-msvc
  cargo tauri --version >/dev/null 2>&1 || cargo install tauri-cli --version 2.11.5 --locked
  cargo xwin --version >/dev/null 2>&1 || cargo install cargo-xwin --version 0.23.1 --locked
  echo 'Cross compilation uses Microsoft SDK/CRT. Review cargo-xwin licensing before downloading: https://github.com/rust-cross/cargo-xwin#readme'
  export XWIN_CACHE_DIR="${XWIN_CACHE_DIR:-$HOME/.cache/ai-light-build/xwin}"
  if ((exe_only)); then
    cargo xwin build -p ai-light --release --target x86_64-pc-windows-msvc --locked
  else
    cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis -- --locked
  fi
fi
out="dist/ubuntu-cross/$stamp"; mkdir -p "$out"
cp Cargo.lock "$out/"
if ((agent)); then cp target/release/light-agent "$out/"; fi
if ((windows)); then
  cp target/x86_64-pc-windows-msvc/release/ai-light.exe "$out/"
  if ((!exe_only)); then
    mapfile -t installers < <(find target/x86_64-pc-windows-msvc/release/bundle/nsis -maxdepth 1 -type f -name '*.exe')
    ((${#installers[@]})) || { echo 'No NSIS installer generated' >&2; exit 1; }
    cp "${installers[@]}" "$out/"
  fi
fi
(cd "$out"; sha256sum -- * > SHA256SUMS)
printf 'Build completed: %s/%s\n' "$PWD" "$out"
