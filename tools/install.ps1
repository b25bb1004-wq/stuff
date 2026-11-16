<#
.SYNOPSIS
Installs Monolith's command-line tools into %USERPROFILE%\.monolith\bin (Windows 11, Windows PowerShell 5.1).

.DESCRIPTION
Default: builds the tools with cargo --release, copies them to %USERPROFILE%\.monolith\bin, writes
VERSION and MANIFEST there, and prints the folder to add to PATH. PATH is NOT changed by default.
Re-running upgrades in place. A tool that is running (file locked) is reported at the end.

-AddToPath  adds the bin folder to the USER Path (never the machine Path). Refuses if it is already there.
-Uninstall  removes the bin folder, VERSION, MANIFEST and the User Path entry for it.
-WhatIf     dry run: prints what would be built, copied and changed, and changes nothing.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File tools\install.ps1
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$AddToPath,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent $PSScriptRoot
$MonoDir = Join-Path $env:USERPROFILE '.monolith'
$BinDir = Join-Path $MonoDir 'bin'

# pkg:bin pairs (the allowlist). Not installed, on purpose:
#   deploy: argo-registry, aurum-node, iridium-server, monolith-web (servers needing setup), healthcheck (container probe)
#   zeus `zeus` and titan `kernel`/`userland`: no_std images, empty stubs on a host
#   silicon crate `silicon`: same bin name as chisel's `silicon`, which is the one installed
$Tools = @(
    'sphinx:sphinx', 'midas:midas', 'kairos:kairos', 'hades:hades', 'chisel:silicon', 'elysium:elysium',
    'cobalt:cobalt', 'cobalt:logos', 'xenon:xenon', 'mercury:mercury', 'iridium:iridium', 'iridium:iridium-cli',
    'chronos:chronos', 'argo:argo', 'aurum:aurum', 'krypton:krypton', 'calliope:calliope', 'indium:indium',
    'iris:iris', 'odyssey:odyssey', 'helios:helios', 'pandora:pandora', 'lithium:lithium', 'zeus:zeusctl',
    'bench:bench', 'athena:athena', 'neon:titan'
)
$ToolNames = @($Tools | ForEach-Object { $_.Split(':')[1] })

function Normalize-PathEntry([string]$p) {
    return $p.Trim().TrimEnd('\').ToLowerInvariant()
}

function Get-UserPathEntries {
    $raw = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrEmpty($raw)) { return @() }
    return @($raw.Split(';') | Where-Object { $_.Trim() -ne '' })
}

function Set-UserPathEntries([string[]]$entries) {
    [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
}

function Get-Summary([string]$exe) {
    # first "name - text" line of --help in the first 6 non-empty lines, else the first non-empty line
    $ErrorActionPreference = 'Continue'
    $out = @(& $exe --help 2>$null | Where-Object { $_.Trim() -ne '' })
    $ErrorActionPreference = 'Stop'
    if ($out.Count -eq 0) { return 'no --help output' }
    $head = @($out | Select-Object -First 6)
    $pick = $head | Where-Object { $_ -match '^\S.* - ' } | Select-Object -First 1
    if (-not $pick) { $pick = $out[0] }
    return ($pick -replace '\s+$', '')
}

function Write-Utf8([string]$path, [string]$text) {
    [IO.File]::WriteAllText($path, $text, (New-Object Text.UTF8Encoding $false))
}

if ($Uninstall) {
    $problems = @()
    if ($PSCmdlet.ShouldProcess($BinDir, 'remove bin folder')) {
        if (Test-Path -LiteralPath $BinDir) {
            try { Remove-Item -LiteralPath $BinDir -Recurse -Force }
            catch { $problems += "could not remove ${BinDir}: $($_.Exception.Message) (a tool still running?)" }
        }
        foreach ($f in 'VERSION', 'MANIFEST') {
            Remove-Item -LiteralPath (Join-Path $MonoDir $f) -Force -ErrorAction SilentlyContinue
        }
        if ((Test-Path -LiteralPath $MonoDir) -and -not (Get-ChildItem -LiteralPath $MonoDir -Force)) {
            Remove-Item -LiteralPath $MonoDir -Force -ErrorAction SilentlyContinue
        }
    }
    $key = Normalize-PathEntry $BinDir
    $entries = Get-UserPathEntries
    $kept = @($entries | Where-Object { (Normalize-PathEntry $_) -ne $key })
    if ($kept.Count -ne $entries.Count) {
        if ($PSCmdlet.ShouldProcess('User Path', 'remove Monolith entry')) {
            Set-UserPathEntries $kept
            Write-Output 'removed the Monolith entry from the User Path'
        }
    }
    if ($problems.Count -gt 0) { $problems | ForEach-Object { Write-Error $_ }; exit 1 }
    Write-Output 'Monolith tools uninstalled. Open a new terminal for PATH to update.'
    exit 0
}

# ---- install ----
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo not found; install Rust from https://rustup.rs first'
}

$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }

$cargoBins = @('build', '--release', '--manifest-path', (Join-Path $Root 'Cargo.toml'))
foreach ($t in $Tools) {
    $bin = $t.Split(':')[1]
    if ($bin -ne 'silicon') { $cargoBins += @('--bin', $bin) }   # silicon is built on its own below
}
$count = $ToolNames.Count

if ($PSCmdlet.ShouldProcess("$count tools", 'cargo build --release')) {
    Write-Output "building $count tools in release mode (first run is slow)..."
    & cargo @cargoBins
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
    & cargo build --release --manifest-path (Join-Path $Root 'Cargo.toml') -p chisel --bin silicon
    if ($LASTEXITCODE -ne 0) { throw "cargo build of silicon failed (exit $LASTEXITCODE)" }
}

$busy = @()
if ($PSCmdlet.ShouldProcess($BinDir, "copy $count tools")) {
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
    foreach ($bin in $ToolNames) {
        $src = Join-Path $TargetDir "release\$bin.exe"
        $dst = Join-Path $BinDir "$bin.exe"
        $tmp = "$dst.new"
        try {
            Copy-Item -LiteralPath $src -Destination $tmp -Force
            Move-Item -LiteralPath $tmp -Destination $dst -Force
        }
        catch {
            Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
            $busy += "$bin ($($_.Exception.Message))"
        }
    }

    $sha = 'unknown'
    try { $sha = (& git -C $Root rev-parse --short HEAD 2>$null).Trim() } catch { }
    if (-not $sha) { $sha = 'unknown' }
    Write-Utf8 (Join-Path $MonoDir 'VERSION') ($sha + "`n")

    $lines = @("# Monolith $sha, installed $(Get-Date -Format 'yyyy-MM-dd'): tool<TAB>one-line --help summary")
    foreach ($bin in $ToolNames) {
        $exe = Join-Path $BinDir "$bin.exe"
        if (Test-Path -LiteralPath $exe) { $lines += "$bin`t$(Get-Summary $exe)" }
    }
    Write-Utf8 (Join-Path $MonoDir 'MANIFEST') (($lines -join "`n") + "`n")
    Write-Output "installed $(@(Get-ChildItem -LiteralPath $BinDir -Filter '*.exe').Count) tools into $BinDir (version $sha)"

    # a running tool keeps the old build in memory after the copy: say so, so the operator restarts it
    $still = @()
    try {
        $still = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
            $_.Path -and $_.Path.StartsWith($BinDir, [StringComparison]::OrdinalIgnoreCase)
        } | ForEach-Object { $_.ProcessName } | Sort-Object -Unique)
    } catch { }
    if ($still.Count -gt 0) {
        Write-Output ('running now, restart to use the new build: ' + ($still -join ', '))
    }
}

if ($AddToPath) {
    $key = Normalize-PathEntry $BinDir
    $entries = Get-UserPathEntries
    if ($entries | Where-Object { (Normalize-PathEntry $_) -eq $key }) {
        Write-Error "refused: $BinDir is already on the User Path; nothing changed"
        exit 1
    }
    if ($PSCmdlet.ShouldProcess('User Path', 'append the bin folder')) {
        Set-UserPathEntries (@($entries) + $BinDir)
        Write-Output "added $BinDir to the USER Path; open a new terminal to use the tools"
    }
}
else {
    Write-Output 'PATH is unchanged. To use the tools everywhere, add this folder to your USER Path:'
    Write-Output "  $BinDir"
    Write-Output '(or re-run with -AddToPath to have it added for you)'
}

if ($busy.Count -gt 0) {
    Write-Error ('NOT UPDATED (in use, close them and re-run): ' + ($busy -join '; '))
    exit 1
}
