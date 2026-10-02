# Platform regression entry point for Windows hosts. Runs the crate tests whose
# behaviour is selected by `cfg(windows)` / `cfg(unix)` / `cfg(target_os = ...)`,
# so a Windows machine can verify its own code paths with one command.
#
# Usage:
#   ./scripts/test-platform.ps1
#   ./scripts/test-platform.ps1 -Crates xai-tty-utils,xai-grok-update
#
# Capture evidence with:
#   ./scripts/test-platform.ps1 *>&1 | Tee-Object platform-test-windows.log

[CmdletBinding()]
param(
    # Default target-OS crate set: TTY/stderr handle handling, the sandbox's
    # Job Object paths, PTY teardown, the updater's per-OS installer hint,
    # process spawning, and the composition-root binary.
    [string[]]$Crates = @(
        'xai-tty-utils',
        'xai-grok-sandbox',
        'xai-grok-shell-terminal',
        'xai-grok-update',
        'xai-grok-tools',
        'xai-grok-pager-bin'
    )
)

$ErrorActionPreference = 'Stop'

# Large actor tests exceed the default 2 MiB test-thread stack; CI uses 16 MiB.
if (-not $env:RUST_MIN_STACK) { $env:RUST_MIN_STACK = '16777216' }

function Get-FirstOutput {
    param([scriptblock]$Body)
    try {
        $value = & $Body 2>$null
        if ($value) { return ($value -join ' ') }
    } catch { }
    return 'unknown'
}

Write-Output '== platform report =='
# The Win32_OperatingSystem class only exists on Windows. Fall back to the
# portable runtime info so the same script is runnable (and testable) on
# PowerShell Core anywhere, while still reporting the full Windows caption.
$osLine = Get-FirstOutput {
    $os = Get-CimInstance Win32_OperatingSystem
    'Windows {0} ({1}) build {2}' -f $os.Caption, $os.Architecture, $os.BuildNumber
}
if ($osLine -eq 'unknown') {
    $rid = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    $osLine = '{0} ({1})' -f $rid, $arch
}
Write-Output "os           : $osLine"

$hostName = if ($env:COMPUTERNAME) { $env:COMPUTERNAME } else { [System.Net.Dns]::GetHostName() }
Write-Output "hostname     : $hostName"
Write-Output ("rustc        : " + (Get-FirstOutput { rustc -Vv }))
Write-Output ("cargo        : " + (Get-FirstOutput { cargo -V }))
$cpus = if ($env:NUMBER_OF_PROCESSORS) { $env:NUMBER_OF_PROCESSORS } else { [Environment]::ProcessorCount }
Write-Output "logical cpus : $cpus"
Write-Output "RUST_MIN_STACK: $env:RUST_MIN_STACK"
Write-Output ("git commit   : " + (Get-FirstOutput { git rev-parse HEAD }))
# A dirty tree means the log does not describe the pushed commit, so say so.
$dirty = Get-FirstOutput { git status --porcelain }
if ($dirty -and $dirty -ne 'unknown') { Write-Output 'working tree : dirty (log does not describe a pushed commit)' }
Write-Output "crates       : $($Crates -join ', ')"
Write-Output ''

$select = @()
foreach ($crate in $Crates) { $select += @('-p', $crate) }

Write-Output '== cargo test =='
& cargo test --locked --no-fail-fast @select
exit $LASTEXITCODE
