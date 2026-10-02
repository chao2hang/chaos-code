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

Write-Output '== platform report =='
$os = Get-CimInstance Win32_OperatingSystem
Write-Output ("os           : Windows {0} ({1}) build {2}" -f $os.Caption, $os.Architecture, $os.BuildNumber)
Write-Output ("hostname     : $env:COMPUTERNAME")
$rustcVersion = (& rustc -V 2>&1) -join ' '
$cargoVersion = (& cargo -V 2>&1) -join ' '
Write-Output ("rustc        : $rustcVersion")
Write-Output ("cargo        : $cargoVersion")
Write-Output ("logical cpus : $env:NUMBER_OF_PROCESSORS")
Write-Output ("RUST_MIN_STACK: $env:RUST_MIN_STACK")
$commit = (& git rev-parse HEAD 2>$null)
if ($commit) { Write-Output "git commit   : $commit" } else { Write-Output 'git commit   : unknown' }
Write-Output "crates       : $($Crates -join ', ')"
Write-Output ''

$select = @()
foreach ($crate in $Crates) { $select += @('-p', $crate) }

Write-Output '== cargo test =='
& cargo test --locked --no-fail-fast @select
exit $LASTEXITCODE
