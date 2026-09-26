param([switch]$ExeOnly)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
function Run-Native([scriptblock]$Command) { & $Command; if ($LASTEXITCODE -ne 0) { throw "Native command failed: $LASTEXITCODE" } }
if (-not (Test-Path Cargo.lock)) { Run-Native { cargo generate-lockfile } }
Run-Native { cargo test -p light-core -p light-agent --target x86_64-pc-windows-msvc --locked }
if ($ExeOnly) {
  Run-Native { cargo build -p ai-light --release --target x86_64-pc-windows-msvc --locked }
} else {
  Run-Native { cargo tauri build --target x86_64-pc-windows-msvc --bundles nsis -- --locked }
  Get-ChildItem target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe | Select-Object FullName,Length
}
