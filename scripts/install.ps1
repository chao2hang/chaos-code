# Install Chaos CLI from GitHub Releases and put it on user PATH.
#
# Preferred on Windows when iex is unavailable: scripts\install.bat
#   install.bat
#   install.bat --version 0.2.113
#
# One-liner (PowerShell, latest):
#   irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1 | iex
#
# Pin a version:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1))) -Version 0.2.113
#
# Or clone/download this file:
#   .\install.ps1 -Version 0.2.113
#   .\install.ps1 -Dir "$env:USERPROFILE\.chaos\bin" -Force
#
# Parameters:
#   -Version   Semver without leading v (default: latest GitHub release)
#   -Dir       Install directory (default: %USERPROFILE%\.chaos\bin or $env:CHAOS_HOME\bin)
#   -NoPath    Do not modify user PATH
#   -Force     Overwrite existing chaos.exe
#   -Repo      GitHub owner/repo (default: chao2hang/chaos-code)

[CmdletBinding()]
param(
    [string]$Version = $env:CHAOS_VERSION,
    [string]$Dir = "",
    [switch]$NoPath,
    [switch]$Force,
    [string]$Repo = $(if ($env:CHAOS_REPO) { $env:CHAOS_REPO } else { "chao2hang/chaos-code" })
)

$ErrorActionPreference = "Stop"
$BinName = "chaos.exe"

function Resolve-ChaosHome {
    if ($env:CHAOS_HOME) { return $env:CHAOS_HOME }
    if ($env:GROK_HOME) { return $env:GROK_HOME }
    # Do NOT assign to $HOME / $home — PowerShell automatic variable is read-only
    # (iex one-liner fails with: Cannot overwrite variable HOME because it is read-only).
    $userHome = $env:USERPROFILE
    if (-not $userHome) {
        try { $userHome = [Environment]::GetFolderPath('UserProfile') } catch { }
    }
    if (-not $userHome -and $env:HOME) { $userHome = $env:HOME }
    if (-not $userHome) { throw "cannot resolve user profile (USERPROFILE/HOME empty)" }
    $chaos = Join-Path $userHome ".chaos"
    $grok = Join-Path $userHome ".grok"
    if (Test-Path -LiteralPath $chaos) { return $chaos }
    if (Test-Path -LiteralPath $grok) { return $grok }
    return $chaos
}

function Get-AssetName {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    switch ($arch) {
        "x64" { return "chaos-win32-x64.exe" }
        "arm64" { return "chaos-win32-arm64.exe" }
        default {
            # Fallback for older PowerShell
            if ($env:PROCESSOR_ARCHITECTURE -match "ARM64") { return "chaos-win32-arm64.exe" }
            return "chaos-win32-x64.exe"
        }
    }
}

function Get-LatestVersion {
    $api = "https://api.github.com/repos/$Repo/releases/latest"
    $headers = @{
        "User-Agent" = "chaos-code-installer"
        "Accept"     = "application/vnd.github+json"
    }
    try {
        $rel = Invoke-RestMethod -Uri $api -Headers $headers
    } catch {
        $status = $null
        try { $status = [int]$_.Exception.Response.StatusCode } catch { }
        if ($status -eq 403) {
            throw "GitHub API rate limited (HTTP 403) resolving latest release for $Repo. Retry later or pass -Version X.Y.Z."
        }
        throw "failed to query latest release for $Repo ($($_.Exception.Message)). Check network/proxy, or pass -Version X.Y.Z."
    }
    $tag = [string]$rel.tag_name
    if (-not $tag) { throw "could not resolve latest release for $Repo (empty tag_name). Pass -Version X.Y.Z." }
    return $tag.TrimStart("v")
}

function Ensure-UserPath {
    param([string]$InstallDir)

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) { $userPath = "" }
    $parts = $userPath -split ";" | Where-Object { $_ -and $_.Trim() -ne "" }
    $normalized = $InstallDir.TrimEnd("\")
    $exists = $false
    foreach ($p in $parts) {
        if ($p.TrimEnd("\") -ieq $normalized) { $exists = $true; break }
    }
    if (-not $exists) {
        $newPath = if ($userPath.Trim() -eq "") { $normalized } else { "$userPath;$normalized" }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "added to user PATH: $normalized"
    } else {
        Write-Host "already on user PATH: $normalized"
    }

    # Current session
    if (-not (($env:Path -split ";") | Where-Object { $_.TrimEnd("\") -ieq $normalized })) {
        $env:Path = "$normalized;$env:Path"
    }
}

# --- main ---
if (-not $Version) {
    Write-Host "resolving latest release..."
    $Version = Get-LatestVersion
}
if (-not $Version) {
    throw "could not resolve a version. Pass -Version X.Y.Z explicitly, or check network/proxy/GitHub rate limits."
}
$Version = $Version.TrimStart("v")

if (-not $Dir) {
    $Dir = Join-Path (Resolve-ChaosHome) "bin"
}

$asset = Get-AssetName
$url = "https://github.com/$Repo/releases/download/v$Version/$asset"
$dest = Join-Path $Dir $BinName

Write-Host "Chaos installer"
Write-Host "  repo:    $Repo"
Write-Host "  version: $Version"
Write-Host "  asset:   $asset"
Write-Host "  dest:    $dest"
Write-Host "  url:     $url"

if ((Test-Path -LiteralPath $dest) -and -not $Force) {
    try {
        $cur = & $dest --version 2>$null
        if ($cur -match [regex]::Escape($Version)) {
            Write-Host "already installed: $cur"
            if (-not $NoPath) { Ensure-UserPath -InstallDir $Dir }
            Write-Host "done. open a new terminal if chaos is not found."
            return
        }
    } catch { }
    throw "$dest exists (use -Force to overwrite)"
}

New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$tmp = Join-Path $env:TEMP ("chaos-install-" + [guid]::NewGuid().ToString("n") + ".exe")

try {
    Write-Host "downloading..."
    $headers = @{ "User-Agent" = "chaos-code-installer" }
    Invoke-WebRequest -Uri $url -OutFile $tmp -Headers $headers -UseBasicParsing

    # Reject tiny HTML error pages
    $len = (Get-Item -LiteralPath $tmp).Length
    if ($len -lt 1MB) {
        $head = Get-Content -LiteralPath $tmp -TotalCount 1 -ErrorAction SilentlyContinue
        if ($head -match "<!DOCTYPE|<html") {
            throw "download looks like HTML, not a binary: $url"
        }
    }

    # Integrity: verify against the release's published SHA256SUMS before the
    # binary is moved into place or executed. Set CHAOS_SKIP_CHECKSUM=1 only if
    # you have verified the download some other way.
    if ($env:CHAOS_SKIP_CHECKSUM -eq "1") {
        Write-Warning "checksum verification skipped (CHAOS_SKIP_CHECKSUM=1)"
    } else {
        $sumsUrl = "https://github.com/$Repo/releases/download/v$Version/SHA256SUMS"
        $sums = $null
        try {
            $sums = (Invoke-WebRequest -Uri $sumsUrl -Headers $headers -UseBasicParsing).Content
        } catch { }

        if (-not $sums) {
            throw ("could not fetch SHA256SUMS for v$Version. This release may predate " +
                   "checksum publishing. To install anyway, set CHAOS_SKIP_CHECKSUM=1 " +
                   "(you are then trusting the download).")
        }

        $expected = $null
        foreach ($line in ($sums -split "`n")) {
            $parts = ($line.Trim() -split '\s+', 2)
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) {
                $expected = $parts[0].ToLower()
                break
            }
        }
        if (-not $expected) { throw "SHA256SUMS has no entry for $asset" }

        $actual = (Get-FileHash -LiteralPath $tmp -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected) {
            throw ("checksum mismatch for ${asset}: expected $expected, got $actual. " +
                   "Refusing to install. This download may be corrupt or tampered with.")
        }
        Write-Host "checksum OK ($actual)"
    }

    Move-Item -Force -LiteralPath $tmp -Destination $dest
} finally {
    if (Test-Path -LiteralPath $tmp) { Remove-Item -Force -LiteralPath $tmp -ErrorAction SilentlyContinue }
}

Write-Host "installed: $dest"
try {
    & $dest --version
} catch {
    Write-Host "(binary installed; --version unavailable in this environment)"
}

if (-not $NoPath) {
    Ensure-UserPath -InstallDir $Dir
}

Write-Host ""
Write-Host "OK. Run: chaos --version"
Write-Host "If command not found, open a NEW terminal (user PATH was updated)."
Write-Host "Or for this session only:"
Write-Host "  `$env:Path = `"$Dir;`$env:Path`""
