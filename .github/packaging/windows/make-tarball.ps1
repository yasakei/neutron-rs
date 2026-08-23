# Build a self-contained portable .tar.gz for the ntsc release binary on Windows.
#
# Usage (from the repository root, AFTER build-msi.ps1 has run):
#
#   pwsh .github/packaging/windows/make-tarball.ps1 -Version 26.0.0b
#
# build-msi.ps1 already stages a complete self-contained tree at dist\win:
# ntsc.exe, its DLL closure, the bundled linker (ld.lld.exe + mingw\ import
# libraries) and the GNU-flavoured runtime (libntsc_runtime.a). This script
# reuses that tree rather than recomputing the closure, so it must run after
# build-msi.ps1.
#
# The Windows layout is intentionally FLAT (everything beside ntsc.exe): the
# compiler resolves both its bundled linker and the runtime archive from its
# own directory, so bin/ + lib/ separation would break `ntsc build`. Extract
# the archive and run ntsc.exe from the extracted folder (or add it to PATH).
param(
  [Parameter(Mandatory = $true)]
  [string]$Version,

  [string]$StageDir = "dist\win",
  [string]$Arch = "x86_64"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path (Join-Path $StageDir "ntsc.exe"))) {
  throw "$StageDir\ntsc.exe not found - run build-msi.ps1 first (it stages the self-contained tree this archive reuses)"
}

$name = "ntsc-$Version-windows-$Arch"
$out = Join-Path "dist" $name
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Path $out | Out-Null

# Copy the whole staged tree flat (ntsc.exe, DLLs, ld.lld.exe, runtime, mingw\),
# but drop the MSI-only license.rtf.
Copy-Item -Recurse -Force (Join-Path $StageDir "*") $out
Remove-Item -Force (Join-Path $out "license.rtf") -ErrorAction SilentlyContinue

# Add the man page and license, matching the Unix tarballs.
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Copy-Item (Join-Path $ScriptDir "..\ntsc.1") (Join-Path $out "ntsc.1")
Copy-Item (Join-Path (Get-Location) "LICENSE") (Join-Path $out "LICENSE")

# bsdtar ships with Windows 10+ as tar.exe and writes gzip archives.
$archive = Join-Path "dist" "$name.tar.gz"
if (Test-Path $archive) { Remove-Item -Force $archive }
tar -C dist -czf $archive $name
if ($LASTEXITCODE -ne 0) { throw "tar failed" }

Remove-Item -Recurse -Force $out
Write-Host "Built dist\$name.tar.gz"
