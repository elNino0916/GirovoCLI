# A release of Girovo CLI, by hand. There is no CI: this runs on the
# maintainer's Windows PC, where the MT-Media certificate is installed.
#
#   pwsh -File scripts\release.ps1             build, test, sign, put the files in dist\
#   pwsh -File scripts\release.ps1 -Publish    the same, then the GitHub release v<version>
#
# The version is Cargo.toml's. Files in dist\:
#   girovo-cli-<version>-windows-x64.exe   signed and timestamped
#   SHA256SUMS.txt
#
# -Publish needs the GitHub CLI (gh) signed in, a clean checkout with the
# commit pushed, and fails when the tag already exists. -Notes <file> sets the
# release notes (else a short default).

param(
  [switch]$Publish,
  [string]$Notes
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw 'No version in Cargo.toml.' }
$tag = "v$version"
Write-Host "Girovo CLI $version"

if ($Publish) {
  if (git status --porcelain) { throw 'The checkout has changes. Commit them first.' }
  git fetch --quiet origin
  $head = git rev-parse HEAD
  $remote = git rev-parse '@{u}' 2>$null
  if ($head -ne $remote) { throw 'This commit is not pushed yet. Push it first.' }
  gh release view $tag --repo elNino0916/GirovoCLI *> $null
  if ($LASTEXITCODE -eq 0) { throw "Release $tag exists already. Raise the version in Cargo.toml." }
}

& "$PSScriptRoot\sign.ps1" -Check

cargo test --release
if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }

$dist = Join-Path $Root 'dist'
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory $dist | Out-Null
$exe = Join-Path $dist "girovo-cli-$version-windows-x64.exe"
Copy-Item target\release\girovo-cli.exe $exe

& "$PSScriptRoot\sign.ps1" $exe

$sums = Get-ChildItem $dist -File | Where-Object Name -ne 'SHA256SUMS.txt' | ForEach-Object {
  "$((Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLowerInvariant())  $($_.Name)"
}
[IO.File]::WriteAllLines((Join-Path $dist 'SHA256SUMS.txt'), [string[]]$sums)
Write-Host "`nIn dist\:"
Get-ChildItem $dist | ForEach-Object { Write-Host "  $($_.Name)" }

if (-not $Publish) {
  Write-Host "`nNot published. Run again with -Publish to make the GitHub release $tag."
  exit 0
}

$notesFile = $Notes
if (-not $notesFile) {
  $notesFile = Join-Path $dist 'notes.md'
  @(
    "Girovo CLI $version for Windows (x64), signed by MT-Media.",
    '',
    'Needs Girovo with Girovo API switched on (Einstellungen → Girovo API). More on https://developer.girovo.de'
  ) | Set-Content -Encoding utf8 $notesFile
}
gh release create $tag --repo elNino0916/GirovoCLI --title "Girovo CLI $version" --notes-file $notesFile --target (git rev-parse HEAD) $exe (Join-Path $dist 'SHA256SUMS.txt')
if ($LASTEXITCODE -ne 0) { throw 'gh release create failed.' }
Write-Host "Published $tag."
