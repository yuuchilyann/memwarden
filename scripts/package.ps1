<#
.SYNOPSIS
Builds a release and packages it as dist\memwarden-<version>-windows-x64.zip.

.DESCRIPTION
The zip holds memwarden.exe, memwarden-host.exe (they must stay in the same
folder), the unpacked browser extension without its tests, the README and the
licenses, inside one top-level folder. A .sha256 file is written next to it.
Used by the release workflow and locally to check the package.

.PARAMETER Tag
Release tag such as v0.1.0; packaging fails unless it matches Cargo.toml.

.PARAMETER NoBuild
Package the existing target\release build instead of building first.

.EXAMPLE
.\scripts\package.ps1
#>
param(
    [string]$Tag,
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    if ($Tag -and $Tag -ne "v$version") {
        throw "Tag $Tag does not match the Cargo.toml version $version"
    }
    if (-not $NoBuild) {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed' }
    }

    $name = "memwarden-$version-windows-x64"
    $dist = Join-Path $root 'dist'
    $stage = Join-Path $dist $name
    $zip = Join-Path $dist "$name.zip"
    foreach ($old in @($stage, $zip, "$zip.sha256")) {
        if (Test-Path $old) { Remove-Item -Recurse -Force $old }
    }
    New-Item -ItemType Directory -Force $stage | Out-Null

    Copy-Item target\release\memwarden.exe, target\release\memwarden-host.exe, README.md, LICENSE-MIT, LICENSE-APACHE $stage
    Copy-Item -Recurse extension (Join-Path $stage 'extension')
    Remove-Item -Recurse -Force (Join-Path $stage 'extension\tests')

    Compress-Archive -Path $stage -DestinationPath $zip
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -Path "$zip.sha256" -Value "$hash  $name.zip" -Encoding ascii
    Remove-Item -Recurse -Force $stage
    $zip
}
finally {
    Pop-Location
}
