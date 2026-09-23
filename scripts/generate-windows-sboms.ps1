param(
  [Parameter(Mandatory = $true)]
  [ValidatePattern('^v\d+\.\d+\.\d+$')]
  [string]$ReleaseTag,

  [Parameter(Mandatory = $true)]
  [string]$OutputDir
)

$ErrorActionPreference = 'Stop'
$generatedName = "Mellowdeck-$ReleaseTag.cdx.json"
cargo cyclonedx --format json --target x86_64-pc-windows-msvc --override-filename "Mellowdeck-$ReleaseTag.cdx"
if ($LASTEXITCODE -ne 0) { throw 'Could not build the SBOMs.' }

New-Item -ItemType Directory -Force $OutputDir | Out-Null
foreach ($entry in @(
  @{ Package = 'mellowdeck-cli'; Suffix = 'cli' },
  @{ Package = 'mellowdeck-player-host'; Suffix = 'player-host' }
)) {
  $package = $entry.Package
  $source = Join-Path "crates/$package" $generatedName
  if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "Missing SBOM for $package at $source."
  }
  $sbom = Get-Content -LiteralPath $source -Raw | ConvertFrom-Json
  if ($sbom.metadata.component.name -ne $package) {
    throw "SBOM at $source describes '$($sbom.metadata.component.name)' instead of $package."
  }
  $destination = Join-Path $OutputDir "Mellowdeck-$ReleaseTag-$($entry.Suffix).cdx.json"
  Move-Item -LiteralPath $source -Destination $destination -Force
}
