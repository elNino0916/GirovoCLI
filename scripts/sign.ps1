# Signs girovo-cli.exe with the MT-Media code-signing certificate, the same
# one Girovo's own programs carry (Girovo's docs/mt-media-code-signing.md).
# Girovo always trusts MT-Media and shows the app as "Girovo CLI" by MT-Media.
#
# The certificate and its private key come from the Windows certificate
# store (CurrentUser\My, found by thumbprint), never from a file, so no
# password is read here. Each file gets a SHA-256 signature with an RFC 3161
# timestamp and is checked afterwards: the signer must be exactly this
# certificate and the timestamp must be there.
#
#   pwsh -File scripts\sign.ps1 target\release\girovo-cli.exe [more files]
#   pwsh -File scripts\sign.ps1 -Check        only checks SignTool and the certificate
#
# GIROVO_SIGN_THUMBPRINT, GIROVO_SIGN_TIMESTAMP and SIGNTOOL override the defaults.

param(
  [switch]$Check,
  [Parameter(ValueFromRemainingArguments = $true)][string[]]$Files
)

$ErrorActionPreference = 'Stop'

$Thumbprint = if ($env:GIROVO_SIGN_THUMBPRINT) { $env:GIROVO_SIGN_THUMBPRINT } else { '07995B7408F1482085F8DC239DEC4294F0CAF149' }
$Thumbprint = ($Thumbprint -replace '\s', '').ToUpperInvariant()
$TimestampServers = if ($env:GIROVO_SIGN_TIMESTAMP) { @($env:GIROVO_SIGN_TIMESTAMP) } else { @('http://timestamp.digicert.com', 'http://timestamp.sectigo.com') }
$Description = 'Girovo CLI'
$InfoUrl = 'https://developer.girovo.de'

function Find-SignTool {
  if ($env:SIGNTOOL) { if (Test-Path $env:SIGNTOOL) { return $env:SIGNTOOL } else { return $null } }
  $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  if (-not (Test-Path $kits)) { return $null }
  $found = Get-ChildItem $kits -Directory | Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
    Sort-Object { [version]$_.Name } -Descending |
    ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
    Where-Object { Test-Path $_ } | Select-Object -First 1
  return $found
}

function Test-Ready {
  $tool = Find-SignTool
  if (-not $tool) { throw 'SignTool was not found. Install the Windows SDK, or set SIGNTOOL to signtool.exe.' }
  $cert = Get-Item -LiteralPath "Cert:\CurrentUser\My\$Thumbprint" -ErrorAction SilentlyContinue
  if (-not $cert) { throw "The MT-Media certificate ($Thumbprint) is not in Cert:\CurrentUser\My. Import MT-Media-CodeSigning.pfx there first." }
  if (-not $cert.HasPrivateKey) { throw 'The MT-Media certificate is there, but without its private key. Import it from the .pfx again.' }
  if ($cert.NotAfter -lt (Get-Date)) { throw "The MT-Media certificate expired on $($cert.NotAfter.ToString('yyyy-MM-dd'))." }
  return @{ Tool = $tool; Cert = $cert }
}

$ready = Test-Ready
if ($Check) {
  Write-Host "SignTool: $($ready.Tool)"
  Write-Host "Certificate: $($ready.Cert.Subject), valid until $($ready.Cert.NotAfter.ToString('yyyy-MM-dd'))"
  exit 0
}
if (-not $Files -or $Files.Count -eq 0) { throw 'Name the files to sign, for example target\release\girovo-cli.exe.' }

foreach ($file in $Files) {
  $path = (Resolve-Path $file).Path
  $signed = $false
  foreach ($server in $TimestampServers) {
    for ($try = 1; $try -le 3 -and -not $signed; $try++) {
      & $ready.Tool sign /fd SHA256 /sha1 $Thumbprint /s My /tr $server /td SHA256 /d $Description /du $InfoUrl $path | Out-Host
      if ($LASTEXITCODE -eq 0) { $signed = $true } else { Start-Sleep -Seconds (2 * $try) }
    }
    if ($signed) { break }
  }
  if (-not $signed) { throw "Signing failed: $path (no timestamp server answered, or SignTool refused)." }

  # Checked twice: SignTool's own check, and who Windows says signed it.
  & $ready.Tool verify /pa /v $path | Out-String | Tee-Object -Variable verify | Out-Null
  if ($LASTEXITCODE -ne 0) { Write-Host $verify; throw "The signature doesn't check out: $path" }
  if ($verify -notmatch 'The signature is timestamped') { throw "The signature has no timestamp: $path" }
  $sig = Get-AuthenticodeSignature -LiteralPath $path
  if ($sig.SignerCertificate.Thumbprint -ne $Thumbprint) { throw "Signed by someone else ($($sig.SignerCertificate.Thumbprint)): $path" }
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
  Write-Host "Signed: $path"
  Write-Host "  by $($sig.SignerCertificate.Subject), status $($sig.Status), timestamp $($sig.TimeStamperCertificate.Subject)"
  Write-Host "  SHA-256 $hash"
}
