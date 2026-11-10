<#
.SYNOPSIS
    Installs Kiln for the current user (no admin needed).

.DESCRIPTION
    Copies the release build to %LOCALAPPDATA%\Programs\Kiln\Kiln.exe and adds a Start menu
    shortcut (and optionally a desktop one). Settings and the game library live separately in
    %LOCALAPPDATA%\Kiln and are kept on reinstall/uninstall.

    Build first:  cargo build --release -p kiln-shell

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Desktop
.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\install.ps1 -Uninstall
#>
param(
    [switch]$Desktop,
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'

$repo     = Split-Path -Parent $PSScriptRoot
$source   = Join-Path $repo 'target\release\kiln-shell.exe'
$dir      = Join-Path $env:LOCALAPPDATA 'Programs\Kiln'
$exe      = Join-Path $dir 'Kiln.exe'
$startLnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Kiln.lnk'
$deskLnk  = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Kiln.lnk'

if (Get-Process -Name 'Kiln' -ErrorAction SilentlyContinue) {
    throw 'Kiln is running. Exit it (power menu > Exit to desktop) and run this again.'
}

if ($Uninstall) {
    foreach ($p in @($startLnk, $deskLnk, $exe)) {
        if (Test-Path $p) { Remove-Item $p -Force; Write-Host "Removed $p" }
    }
    if ((Test-Path $dir) -and -not (Get-ChildItem $dir)) { Remove-Item $dir }
    Write-Host "Kiln uninstalled. Your settings and library are still in $env:LOCALAPPDATA\Kiln."
    return
}

if (-not (Test-Path $source)) {
    throw "No release build at $source. Run: cargo build --release -p kiln-shell"
}

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item $source $exe -Force
Write-Host "Installed $exe"

$shell = New-Object -ComObject WScript.Shell
$targets = @($startLnk)
if ($Desktop) { $targets += $deskLnk }
foreach ($lnk in $targets) {
    $s = $shell.CreateShortcut($lnk)
    $s.TargetPath = $exe
    $s.WorkingDirectory = $dir
    $s.IconLocation = "$exe,0"
    $s.Description = 'Kiln: couch-friendly game launcher'
    $s.Save()
    Write-Host "Shortcut $lnk"
}
Write-Host 'Done. Open Kiln from the Start menu.'
