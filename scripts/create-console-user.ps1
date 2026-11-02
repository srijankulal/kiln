# Kiln Console Mode: Optional Dedicated User Profile Setup Script
# Run this script in an Elevated PowerShell prompt on your Windows 11 PC.
# This script creates a standard, non-administrative local Windows user named 'KilnConsole'
# and configures it to launch Kiln automatically upon login.
#
# To undo this at any time, run:
# net user KilnConsole /delete

param(
    [string]$Username = "KilnConsole",
    [string]$InstallPath = "$env:LOCALAPPDATA\Kiln\bin\kiln.exe"
)

Write-Host "=== Kiln Dedicated Console User Setup ===" -ForegroundColor Cyan

# 1. Check for administrative privileges
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "Please run this script as Administrator."
    exit 1
}

# 2. Check if user already exists
$existingUser = Get-LocalUser -Name $Username -ErrorAction SilentlyContinue
if ($existingUser) {
    Write-Host "User '$Username' already exists." -ForegroundColor Yellow
} else {
    Write-Host "Creating standard local user '$Username'..." -ForegroundColor Green
    $password = Read-Host -Prompt "Enter password for $Username (or press Enter for none)" -AsSecureString
    if ($password.Length -eq 0) {
        # Blank password for instant console login if preferred
        New-LocalUser -Name $Username -Description "Dedicated Kiln Console Mode User" -PasswordNeverExpires | Out-Null
    } else {
        New-LocalUser -Name $Username -Password $password -Description "Dedicated Kiln Console Mode User" -PasswordNeverExpires | Out-Null
    }
    Write-Host "Standard user '$Username' created successfully." -ForegroundColor Green
}

Write-Host ""
Write-Host "Configuration options:"
Write-Host "1. (Recommended) Auto-start Kiln via Startup folder on first login to $Username."
Write-Host "2. Kiln will never replace the Windows shell for your primary account."
Write-Host ""
Write-Host "Setup complete. Switch to user '$Username' when connecting to your TV." -ForegroundColor Cyan
