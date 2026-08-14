# Install Chaos CLI from GitHub Releases and put it on user PATH.
#
# Preferred on Windows when iex is unavailable: scripts\install.bat
#   install.bat
#   install.bat --version 0.2.113
#
# One-liner (PowerShell 5.1 / 7+, latest):
#   irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1 | iex
#
# Pin a version (also skips the GitHub "latest" API call):
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.ps1))) -Version 0.2.113
#
# cmd.exe one-liner (NOT for PowerShell — Windows PowerShell 5.1 rejects &&):
#   curl -L -o "%TEMP%\install-chaos.bat" https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.bat && "%TEMP%\install-chaos.bat"
#
# PowerShell equivalent of the bat one-liner:
#   $bat = "$env:TEMP\install-chaos.bat"; curl.exe -L -o $bat https://raw.githubusercontent.com/chao2hang/chaos-code/main/scripts/install.bat; & $bat
#
# Or clone/download this file:
#   .\install.ps1 -Version 0.2.113
#   .\install.ps1 -Dir "$env:USERPROFILE\.chaos\bin" -Force
#
# Parameters:
#   -Version   Semver without leading v (default: latest GitHub release)
#   -Dir       Install directory (default: %USERPROFILE%\.chaos\bin or $env:CHAOS_HOME\bin)
#   -NoPath    Do not modify user PATH
#   -Force     Re-download even if the target version is already installed
#   -Repo      GitHub owner/repo (default: chao2hang/chaos-code)
#   -Mirror    GitHub mirror prefix (e.g. https://ghfast.top); also CHAOS_GITHUB_MIRROR
#   -Cn        Prefer public mirrors first (also CHAOS_CN=1 / CHAOS_MIRROR_FIRST=1)
#
# Existing installs are upgraded in place when the resolved version differs.
#
# Download acceleration (China / restricted networks):
#   $env:CHAOS_GITHUB_MIRROR = "https://ghfast.top"
#   $env:CHAOS_CN = "1"
# Binary is still verified against SHA256SUMS after download.

[CmdletBinding()]
param(
    [string]$Version = $env:CHAOS_VERSION,
    [string]$Dir = "",
    [switch]$NoPath,
    [switch]$Force,
    [string]$Repo = $(if ($env:CHAOS_REPO) { $env:CHAOS_REPO } else { "chao2hang/chaos-code" }),
    [string]$Mirror = $(if ($env:CHAOS_GITHUB_MIRROR) { $env:CHAOS_GITHUB_MIRROR } else { "" }),
    [switch]$Cn
)

$ErrorActionPreference = "Stop"
$BinName = "chaos.exe"

# Windows PowerShell 5.1 defaults to TLS 1.0; GitHub requires TLS 1.2+.
# No-op / harmless on PowerShell 7+ and when the enum value is already present.
try {
    $tls = [Net.ServicePointManager]::SecurityProtocol
    $want = [Net.SecurityProtocolType]::Tls12
    if (($tls -band $want) -ne $want) {
        [Net.ServicePointManager]::SecurityProtocol = $tls -bor $want
    }
} catch { }

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
    # Prefer RuntimeInformation when present (.NET 4.7.1+ / PowerShell 7+).
    # Fall back to PROCESSOR_* env vars on older Windows PowerShell hosts.
    try {
        $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
        if ($arch -eq "arm64") { return "chaos-win32-arm64.exe" }
        if ($arch -eq "x64" -or $arch -eq "x86") { return "chaos-win32-x64.exe" }
    } catch {
        # Type missing or otherwise unusable — fall through to env vars.
    }

    if ($env:PROCESSOR_ARCHITECTURE -match "ARM64" -or $env:PROCESSOR_ARCHITEW6432 -match "ARM64") {
        return "chaos-win32-arm64.exe"
    }
    return "chaos-win32-x64.exe"
}

function Strip-LeadingV {
    param([AllowNull()][string]$Tag)
    if ([string]::IsNullOrWhiteSpace($Tag)) { return $Tag }
    $t = $Tag.Trim()
    if ($t.Length -gt 0 -and ($t[0] -eq 'v' -or $t[0] -eq 'V')) {
        return $t.Substring(1)
    }
    return $t
}

function Get-HttpStatusCode {
    param($ErrorRecord)
    try {
        if ($null -eq $ErrorRecord -or $null -eq $ErrorRecord.Exception) { return $null }
        $resp = $ErrorRecord.Exception.Response
        if ($null -eq $resp) { return $null }
        # WinPS: HttpWebResponse.StatusCode (enum); PS7: HttpResponseMessage.StatusCode
        $code = $resp.StatusCode
        if ($null -eq $code) { return $null }
        return [int]$code
    } catch {
        return $null
    }
}

function Get-TagFromLocationHeader {
    param([AllowNull()][string]$Location)
    if ([string]::IsNullOrWhiteSpace($Location)) { return $null }
    if ($Location -match 'tag/([^/\\?#"\s]+)') {
        return (Strip-LeadingV -Tag $Matches[1])
    }
    return $null
}

function Get-LatestVersionViaRedirect {
    # Follow-free HEAD/GET of /releases/latest → Location: .../tag/vX.Y.Z
    # Avoids GitHub API quota. Prefer HttpWebRequest (reliable on WinPS 5.1).
    $latestUrl = "https://github.com/$Repo/releases/latest"
    try {
        $req = [System.Net.HttpWebRequest]::Create($latestUrl)
        $req.Method = "GET"
        $req.AllowAutoRedirect = $false
        $req.UserAgent = "chaos-code-installer"
        $req.Timeout = 30000
        try {
            $resp = $req.GetResponse()
        } catch [System.Net.WebException] {
            $resp = $_.Exception.Response
        }
        if ($null -ne $resp) {
            try {
                $location = $resp.Headers["Location"]
                $tag = Get-TagFromLocationHeader -Location $location
                if ($tag) { return $tag }
            } finally {
                if ($resp -is [System.IDisposable]) { $resp.Dispose() }
            }
        }
    } catch {
        # fall through
    }

    # curl.exe -I (Windows 10+ often has it)
    try {
        $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
        if ($curl) {
            $headers = & curl.exe -fsSLI -A "chaos-code-installer" $latestUrl 2>$null
            if ($headers) {
                foreach ($line in ($headers -split "`r?`n")) {
                    if ($line -match '^[Ll]ocation:\s*(.+)$') {
                        $tag = Get-TagFromLocationHeader -Location $Matches[1].Trim()
                        if ($tag) { return $tag }
                    }
                }
            }
        }
    } catch {
        # fall through
    }

    return $null
}

function Get-LatestVersion {
    $headers = @{
        "User-Agent" = "chaos-code-installer"
        "Accept"     = "application/vnd.github+json"
    }

    # 1) GitHub API /releases/latest — try origin + mirrors (same list as downloads)
    $apiOrigin = "https://api.github.com/repos/$Repo/releases/latest"
    $apiCandidates = Get-GitHubUrlCandidates -OriginUrl $apiOrigin
    $tmpJson = Join-Path $env:TEMP ("chaos-rel-" + [guid]::NewGuid().ToString("n") + ".json")
    try {
        foreach ($api in $apiCandidates) {
            try {
                Write-Host "  resolve: $api"
                Invoke-WebRequest -Uri $api -OutFile $tmpJson -Headers $headers -UseBasicParsing
                if (-not (Test-Path -LiteralPath $tmpJson)) { continue }
                $raw = [System.IO.File]::ReadAllText($tmpJson, [System.Text.Encoding]::UTF8)
                $tagName = $null
                if ($raw -match '"tag_name"\s*:\s*"([^"]+)"') {
                    $tagName = $Matches[1]
                }
                if (-not [string]::IsNullOrWhiteSpace($tagName)) {
                    return (Strip-LeadingV -Tag $tagName)
                }
            } catch {
                $status = Get-HttpStatusCode $_
                if ($status -eq 403 -and $api -eq $apiOrigin) {
                    Write-Warning "GitHub API rate limited (HTTP 403); trying mirrors / redirect."
                }
            }
        }
        Write-Warning "GitHub API latest failed for all candidates; trying redirect fallback."
    } finally {
        if (Test-Path -LiteralPath $tmpJson) {
            Remove-Item -Force -LiteralPath $tmpJson -ErrorAction SilentlyContinue
        }
    }

    # 2) releases/latest redirect → .../tag/vX.Y.Z (no API quota)
    $viaRedirect = Get-LatestVersionViaRedirect
    if (-not [string]::IsNullOrWhiteSpace($viaRedirect)) {
        return $viaRedirect
    }

    throw ("could not resolve latest release for $Repo. Pass -Version X.Y.Z explicitly, " +
           "or set `$env:CHAOS_CN='1' / `$env:CHAOS_GITHUB_MIRROR='https://ghfast.top'.")
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

# Public ghproxy-style mirrors (prefix + full origin URL).
$script:DefaultGitHubMirrors = @(
    "https://ghfast.top",
    "https://ghproxy.net",
    "https://mirror.ghproxy.com"
)

function Apply-UrlMirror {
    param([string]$MirrorPrefix, [string]$Url)
    $m = $MirrorPrefix.TrimEnd("/")
    return "$m/$Url"
}

function Get-GitHubUrlCandidates {
    param([string]$OriginUrl)

    $preferMirrors = $false
    if ($Cn) { $preferMirrors = $true }
    if ($env:CHAOS_CN -eq "1") { $preferMirrors = $true }
    if ($env:CHAOS_MIRROR_FIRST -eq "1") { $preferMirrors = $true }

    $list = New-Object System.Collections.Generic.List[string]
    $seen = @{}

    $ordered = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrWhiteSpace($Mirror)) {
        [void]$ordered.Add((Apply-UrlMirror -MirrorPrefix $Mirror -Url $OriginUrl))
    }
    if ($preferMirrors) {
        foreach ($m in $script:DefaultGitHubMirrors) {
            [void]$ordered.Add((Apply-UrlMirror -MirrorPrefix $m -Url $OriginUrl))
        }
        [void]$ordered.Add($OriginUrl)
    } else {
        [void]$ordered.Add($OriginUrl)
        foreach ($m in $script:DefaultGitHubMirrors) {
            [void]$ordered.Add((Apply-UrlMirror -MirrorPrefix $m -Url $OriginUrl))
        }
    }

    foreach ($c in $ordered) {
        if ([string]::IsNullOrWhiteSpace($c)) { continue }
        if ($seen.ContainsKey($c)) { continue }
        $seen[$c] = $true
        [void]$list.Add($c)
    }

    return ,$list.ToArray()
}

function Download-GitHubFile {
    param(
        [string]$OriginUrl,
        [string]$OutFile,
        [hashtable]$Headers,
        [int]$MinBytes = 16
    )

    $candidates = Get-GitHubUrlCandidates -OriginUrl $OriginUrl
    $lastErr = $null
    foreach ($cand in $candidates) {
        try {
            if (Test-Path -LiteralPath $OutFile) {
                Remove-Item -Force -LiteralPath $OutFile -ErrorAction SilentlyContinue
            }
            Write-Host "  try: $cand"
            Invoke-WebRequest -Uri $cand -OutFile $OutFile -Headers $Headers -UseBasicParsing
            if (-not (Test-Path -LiteralPath $OutFile)) { continue }
            $len = (Get-Item -LiteralPath $OutFile).Length
            if ($len -lt $MinBytes) {
                $lastErr = "too small ($len bytes) from $cand"
                Remove-Item -Force -LiteralPath $OutFile -ErrorAction SilentlyContinue
                continue
            }
            # Reject HTML error pages when we expected a binary/text asset
            if ($len -lt 1MB) {
                $head = Get-Content -LiteralPath $OutFile -TotalCount 1 -ErrorAction SilentlyContinue
                if ($head -match "<!DOCTYPE|<html") {
                    $lastErr = "HTML response from $cand"
                    Remove-Item -Force -LiteralPath $OutFile -ErrorAction SilentlyContinue
                    continue
                }
            }
            return $cand
        } catch {
            $lastErr = $_.Exception.Message
            if (Test-Path -LiteralPath $OutFile) {
                Remove-Item -Force -LiteralPath $OutFile -ErrorAction SilentlyContinue
            }
        }
    }
    throw ("download failed for $OriginUrl. last error: $lastErr. " +
           "Tip: set `$env:CHAOS_GITHUB_MIRROR='https://ghfast.top' or `$env:CHAOS_CN='1'")
}

# --- main ---
if ([string]::IsNullOrWhiteSpace($Version)) {
    Write-Host "resolving latest release..."
    $Version = Get-LatestVersion
}
if ([string]::IsNullOrWhiteSpace($Version)) {
    throw "could not resolve a version. Pass -Version X.Y.Z explicitly, or check network/proxy/GitHub rate limits."
}
$Version = Strip-LeadingV -Tag $Version.Trim()

if (-not $Dir) {
    $Dir = Join-Path (Resolve-ChaosHome) "bin"
}

$asset = Get-AssetName
$originUrl = "https://github.com/$Repo/releases/download/v$Version/$asset"
$sumsOrigin = "https://github.com/$Repo/releases/download/v$Version/SHA256SUMS"
$sigOrigin = "https://github.com/$Repo/releases/download/v$Version/$asset.sig"
$dest = Join-Path $Dir $BinName

Write-Host "Chaos installer"
Write-Host "  repo:    $Repo"
Write-Host "  version: $Version"
Write-Host "  asset:   $asset"
Write-Host "  dest:    $dest"
Write-Host "  origin:  $originUrl"
if (-not [string]::IsNullOrWhiteSpace($Mirror)) {
    Write-Host "  mirror:  $Mirror (CHAOS_GITHUB_MIRROR / -Mirror)"
} elseif ($Cn -or $env:CHAOS_CN -eq "1" -or $env:CHAOS_MIRROR_FIRST -eq "1") {
    Write-Host "  mirror:  public list first (CHAOS_CN / -Cn)"
} else {
    Write-Host "  mirror:  origin first, then public fallbacks"
}

# Default is upgrade-in-place. -Force re-downloads even when already on target.
if ((Test-Path -LiteralPath $dest) -and -not $Force) {
    try {
        $cur = & $dest --version 2>$null
        if ($cur -and ($cur -match [regex]::Escape($Version))) {
            Write-Host "already installed: $cur"
            if (-not $NoPath) { Ensure-UserPath -InstallDir $Dir }
            Write-Host "done. open a NEW terminal if chaos is not found, or for this session:"
            Write-Host "  `$env:Path = `"$Dir;`$env:Path`""
            return
        }
        if ($cur) {
            Write-Host "upgrading existing install: $cur -> $Version"
        } else {
            Write-Host "replacing existing binary at $dest"
        }
    } catch {
        Write-Host "replacing existing binary at $dest"
    }
}

New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$tmp = Join-Path $env:TEMP ("chaos-install-" + [guid]::NewGuid().ToString("n") + ".exe")

try {
    Write-Host "downloading..."
    $headers = @{ "User-Agent" = "chaos-code-installer" }
    $usedUrl = Download-GitHubFile -OriginUrl $originUrl -OutFile $tmp -Headers $headers -MinBytes 1MB
    Write-Host "  from: $usedUrl"

    # Integrity: verify against the release's published SHA256SUMS before the
    # binary is moved into place or executed. Set CHAOS_SKIP_CHECKSUM=1 only if
    # you have verified the download some other way.
    #
    # IMPORTANT (WinPS 5.1): GitHub serves SHA256SUMS as application/octet-stream,
    # so Invoke-WebRequest .Content is often a [byte[]] — never -split that. Always
    # download to a file and read as UTF-8 text.
    if ($env:CHAOS_SKIP_CHECKSUM -eq "1") {
        Write-Warning "checksum verification skipped (CHAOS_SKIP_CHECKSUM=1)"
    } else {
        $sumsFile = Join-Path $env:TEMP ("chaos-sums-" + [guid]::NewGuid().ToString("n") + ".txt")
        $expected = $null
        try {
            try {
                [void](Download-GitHubFile -OriginUrl $sumsOrigin -OutFile $sumsFile -Headers $headers -MinBytes 16)
            } catch {
                throw ("could not fetch SHA256SUMS for v$Version. This release may predate " +
                       "checksum publishing. To install anyway, set CHAOS_SKIP_CHECKSUM=1 " +
                       "(you are then trusting the download).")
            }

            # Read as text bytes → UTF-8. Avoid Get-Content default encoding quirks.
            $sumsText = [System.IO.File]::ReadAllText($sumsFile, [System.Text.Encoding]::UTF8)
            foreach ($line in ($sumsText -split "`r?`n")) {
                $m = [regex]::Match($line.Trim(), '^(?<hash>[A-Fa-f0-9]{64})\s+\*?(?<name>\S+)\s*$')
                if ($m.Success -and ($m.Groups['name'].Value -eq $asset)) {
                    $expected = $m.Groups['hash'].Value.ToLowerInvariant()
                    break
                }
            }
            if (-not $expected) {
                $preview = ($sumsText -split "`r?`n" | Select-Object -First 8) -join "; "
                throw ("SHA256SUMS has no entry for $asset. First lines: $preview")
            }

            $actual = (Get-FileHash -LiteralPath $tmp -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($actual -ne $expected) {
                throw ("checksum mismatch for ${asset}: expected $expected, got $actual. " +
                       "Refusing to install. This download may be corrupt or tampered with.")
            }
            Write-Host "checksum OK ($actual)"
        } finally {
            if (Test-Path -LiteralPath $sumsFile) {
                Remove-Item -Force -LiteralPath $sumsFile -ErrorAction SilentlyContinue
            }
        }
    }

    # Signature verification: verify the downloaded binary against its .sig
    # sidecar using Python's cryptography library (if available). This is
    # defense-in-depth on top of the SHA256 checksum — the checksum catches
    # corruption, the signature catches a compromised release.
    #
    # Skipped silently when Python/cryptography is not installed,
    # when CHAOS_SKIP_SIGNATURE=1 is set, or when no .sig file exists at the
    # release URL (older releases that predate signing).
    # Fails hard when Python+cryptography IS available, the .sig IS found,
    # but the signature does not verify.
    if ($env:CHAOS_SKIP_SIGNATURE -eq "1") {
        Write-Warning "signature verification skipped (CHAOS_SKIP_SIGNATURE=1)"
    } else {
        $python = Get-Command python -ErrorAction SilentlyContinue
        $pubKey = $env:CHAOS_SIGNING_PUBLIC_KEY
        # Probe cryptography availability first — a missing package must
        # skip, not be reported as a tampered binary.
        $cryptoAvail = $false
        if ($python) {
            & python -c "from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey" 2>$null
            $cryptoAvail = ($LASTEXITCODE -eq 0)
        }
        if ($python -and $cryptoAvail -and $pubKey) {
            $sigFile = Join-Path $env:TEMP ("chaos-sig-" + [guid]::NewGuid().ToString("n") + ".sig")
            try {
                [void](Download-GitHubFile -OriginUrl $sigOrigin -OutFile $sigFile -Headers $headers -MinBytes 16 -ErrorAction SilentlyContinue)
                if (Test-Path -LiteralPath $sigFile) {
                    # Verify with Python's cryptography library (ed25519, raw).
                    # The .sig file contains a bare base64-encoded 64-byte signature.
                    $pyScript = @"
import base64, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

pubkey = base64.b64decode('$pubKey')
pk = Ed25519PublicKey.from_public_bytes(pubkey)

with open(r'$tmp', 'rb') as f:
    data = f.read()
with open(r'$sigFile', 'r') as f:
    sig = base64.b64decode(f.read().strip())

pk.verify(sig, data)
print('signature OK')
"@
                    $result = & python -c $pyScript 2>&1
                    if ($LASTEXITCODE -eq 0) {
                        Write-Host "signature OK"
                    } else {
                        throw ("signature verification FAILED for $asset. " +
                               "The binary may have been tampered with. Refusing to install. " +
                               "To bypass (NOT recommended), set CHAOS_SKIP_SIGNATURE=1.")
                    }
                }
            } finally {
                if (Test-Path -LiteralPath $sigFile) { Remove-Item -Force -LiteralPath $sigFile -ErrorAction SilentlyContinue }
            }
        }
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
Write-Host "OK. Verify (open a NEW terminal, or refresh PATH in this session):"
Write-Host "  `$env:Path = `"$Dir;`$env:Path`""
Write-Host "  chaos --version"
Write-Host "Or: & `"$dest`" --version"
