<#
.SYNOPSIS
  Copies a World of Warcraft installation's addon data into dev-sandbox/ for development.

.DESCRIPTION
  Debug builds of the app use dev-sandbox/ as the WoW root, so development never touches
  the real installation. This script only reads from -Source. It copies .build.info and,
  for each game client, .flavor.info and Interface\AddOns.

.EXAMPLE
  ./scripts/make-sandbox.ps1
  ./scripts/make-sandbox.ps1 -Clients _retail_,_forever_
  ./scripts/make-sandbox.ps1 -TocOnly
#>
[CmdletBinding()]
param(
  [string]$Source = "F:\World of Warcraft",
  [string]$Destination = (Join-Path $PSScriptRoot "..\dev-sandbox"),
  # Client folders to copy, e.g. _retail_. Defaults to every _*_ folder in -Source.
  [string[]]$Clients,
  # Copy only .toc files: fast and enough for scanning, but not for install tests.
  [switch]$TocOnly
)

$ErrorActionPreference = "Stop"

$Source = (Resolve-Path $Source).Path
$Destination = [IO.Path]::GetFullPath($Destination)

if ($Destination.StartsWith($Source.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or $Destination -ieq $Source) {
  throw "Destination '$Destination' must not be inside the source installation '$Source'."
}
if (-not (Test-Path (Join-Path $Source ".build.info"))) {
  throw "'$Source' does not look like a World of Warcraft installation (no .build.info)."
}

if (-not $Clients) {
  $Clients = Get-ChildItem $Source -Directory | Where-Object Name -Match '^_.+_$' | Select-Object -ExpandProperty Name
}

New-Item -ItemType Directory -Force $Destination | Out-Null
Copy-Item (Join-Path $Source ".build.info") $Destination -Force

foreach ($client in $Clients) {
  $clientSource = Join-Path $Source $client
  $clientDest = Join-Path $Destination $client
  if (-not (Test-Path (Join-Path $clientSource ".flavor.info"))) {
    Write-Warning "Skipping $client (no .flavor.info)."
    continue
  }

  New-Item -ItemType Directory -Force $clientDest | Out-Null
  Copy-Item (Join-Path $clientSource ".flavor.info") $clientDest -Force

  $addonsSource = Join-Path $clientSource "Interface\AddOns"
  $addonsDest = Join-Path $clientDest "Interface\AddOns"
  if (-not (Test-Path $addonsSource)) {
    New-Item -ItemType Directory -Force $addonsDest | Out-Null
    continue
  }

  # /MIR mirrors into the sandbox only; robocopy never changes the source.
  $robocopyArgs = @($addonsSource, $addonsDest)
  if ($TocOnly) { $robocopyArgs += "*.toc" }
  $robocopyArgs += @("/MIR", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS", "/NP")
  robocopy @robocopyArgs | Out-Null
  if ($LASTEXITCODE -ge 8) { throw "robocopy failed for $client (exit code $LASTEXITCODE)." }

  $count = (Get-ChildItem $addonsDest -Directory).Count
  Write-Host "$client : $count addon folders"
}

Write-Host "Sandbox ready at $Destination"
