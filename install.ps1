# Install a built Undavi for the current user, with a Start menu entry and one under
# "Installed apps" in the Windows settings. Used by the release zip and by updates of it
# (there's also an installer, see packaging\windows); also works on its own:
#
#   cargo build --release
#   powershell -ExecutionPolicy Bypass -File install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Uninstall
#
# In File Explorer: right-click it, then "Run with PowerShell". It never needs to be run
# as administrator.
#
# -Prefix changes where to (a folder you can write to; use the same one to uninstall).
# -FilesOnly leaves out both entries (for packaging).
param(
    [string]$Prefix,
    [switch]$Uninstall,
    [switch]$FilesOnly
)

Set-StrictMode -Version 3
$ErrorActionPreference = 'Stop'

$root = $PSScriptRoot
$isDefault = -not $Prefix
if ($isDefault) {
    $Prefix = Join-Path ([Environment]::GetFolderPath('LocalApplicationData', 'Create')) 'Programs\Undavi'
}
$Prefix = [IO.Path]::GetFullPath($Prefix).TrimEnd('\')
$bin = Join-Path $Prefix 'bin'
$program = Join-Path $bin 'undavi.exe'
$licenses = Join-Path $Prefix 'licenses'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs', 'Create')) 'Undavi.lnk'
$entry = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Undavi'

# Where the entry under "Installed apps" says Undavi is.
function Get-InstalledPrefix {
    $value = Get-ItemProperty -LiteralPath $entry -ErrorAction SilentlyContinue
    if ($value -and $value.PSObject.Properties['InstallLocation']) { $value.InstallLocation }
}

# Versions that were replaced while they were running (see below).
function Remove-Replaced {
    Get-ChildItem -LiteralPath $bin -Filter 'undavi.exe.old*' -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

function Install-Undavi {
    $built = Join-Path $root 'target\release\undavi.exe'
    if (-not (Test-Path -LiteralPath $built)) {
        throw "There's no undavi.exe next to this script. Unpack the whole zip first, or run: cargo build --release"
    }

    foreach ($folder in $bin, $licenses) {
        $null = New-Item -ItemType Directory -Force -Path $folder
    }
    Remove-Replaced
    # A running program can't be overwritten, but it can be moved out of the way.
    $old = $null
    if (Test-Path -LiteralPath $program) {
        $old = "$program.old-$([Guid]::NewGuid().ToString('N'))"
        Move-Item -LiteralPath $program -Destination $old
    }
    try {
        Copy-Item -LiteralPath $built -Destination $program
    } catch {
        if ($old) { Move-Item -LiteralPath $old -Destination $program -Force }
        throw
    }
    Remove-Replaced

    # Undavi's license, the bundled font's and icons' licenses, and the notices of the
    # Rust crates compiled into the program, which their licenses ask to travel with it.
    foreach ($file in 'LICENSE', 'NOTICE', 'assets\fonts\OFL-Inter.txt', 'packaging\THIRD-PARTY-LICENSES.txt') {
        Copy-Item -LiteralPath (Join-Path $root $file) -Destination $licenses -Force
    }
    # Under another name: it's also called LICENSE.
    Copy-Item -LiteralPath (Join-Path $root 'assets\icons\ui\LICENSE') `
        -Destination (Join-Path $licenses 'Apache-Material-Symbols.txt') -Force

    # To uninstall with, and how updates tell that this is an install.
    $script = Join-Path $Prefix 'install.ps1'
    if ($PSCommandPath -ne $script) {
        Copy-Item -LiteralPath $PSCommandPath -Destination $script -Force
    }
    if ($FilesOnly) { return }

    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    $link.TargetPath = $program
    $link.WorkingDirectory = $bin
    $link.Description = 'Music visualizer with synced lyrics'
    $link.Save()

    $powershell = Join-Path $PSHOME 'powershell.exe'
    $uninstaller = "`"$powershell`" -NoProfile -ExecutionPolicy Bypass -File `"$script`" -Uninstall -Prefix `"$Prefix`""
    $size = (Get-ChildItem -LiteralPath $Prefix -Recurse -File | Measure-Object -Property Length -Sum).Sum
    $null = New-Item -Path $entry -Force
    foreach ($value in @{
        DisplayName = 'Undavi'
        DisplayVersion = (Get-Item -LiteralPath $program).VersionInfo.ProductVersion
        DisplayIcon = $program
        Publisher = 'Karson Califf'
        URLInfoAbout = 'https://github.com/karsoncaliff/undavi'
        InstallLocation = $Prefix
        UninstallString = $uninstaller
    }.GetEnumerator()) {
        $null = New-ItemProperty -LiteralPath $entry -Name $value.Key -Value "$($value.Value)" -PropertyType String
    }
    foreach ($value in @{ EstimatedSize = [int]($size / 1KB); NoModify = 1; NoRepair = 1 }.GetEnumerator()) {
        $null = New-ItemProperty -LiteralPath $entry -Name $value.Key -Value $value.Value -PropertyType DWord
    }
    Write-Host "Installed Undavi to $Prefix. It's in the Start menu."
}

function Uninstall-Undavi {
    # Only the Undavi in this folder.
    $running = @(Get-Process -Name 'undavi' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $program })
    if ($running) {
        throw 'Undavi is running. Close it first.'
    }
    Remove-Replaced
    foreach ($file in @($program, (Join-Path $Prefix 'install.ps1')) +
        @('LICENSE', 'NOTICE', 'OFL-Inter.txt', 'THIRD-PARTY-LICENSES.txt', 'Apache-Material-Symbols.txt' |
            ForEach-Object { Join-Path $licenses $_ })) {
        if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force }
    }
    # The folders, unless there's something else in them.
    foreach ($folder in $bin, $licenses, $Prefix) {
        if ((Test-Path -LiteralPath $folder) -and -not (Get-ChildItem -LiteralPath $folder -Force)) {
            Remove-Item -LiteralPath $folder -Force
        }
    }
    # The entries, unless they're about an Undavi installed somewhere else.
    if ((Get-InstalledPrefix) -eq $Prefix) {
        Remove-Item -LiteralPath $entry -Recurse -Force
        if (Test-Path -LiteralPath $shortcut) { Remove-Item -LiteralPath $shortcut -Force }
    }
    Write-Host "Removed Undavi from $Prefix. Settings stay in $env:APPDATA\undavi."
}

$failed = $false
try {
    if (Test-Path -LiteralPath (Join-Path $Prefix 'unins000.exe')) {
        throw 'This Undavi was installed with setup.exe. Use a newer one to update it, or Installed apps in the Windows settings to remove it.'
    }
    if ($Uninstall) {
        # Where it was installed, unless told otherwise.
        $installed = Get-InstalledPrefix
        if ($isDefault -and $installed -and $installed -ne $Prefix) {
            throw "Undavi is installed in another folder. Use: -Uninstall -Prefix `"$installed`""
        }
        Uninstall-Undavi
    } else {
        Install-Undavi
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    $failed = $true
}

# Started from File Explorer or the Windows settings: keep the window open to read.
try {
    $parent = (Get-CimInstance Win32_Process -Filter "ProcessId = $PID").ParentProcessId
    $name = (Get-Process -Id $parent -ErrorAction Stop).ProcessName
    if ($name -in 'explorer', 'SystemSettings') {
        $null = Read-Host 'Press Enter to close'
    }
} catch {
    # Started some other way (or without anyone to read it).
}
if ($failed) { exit 1 }
