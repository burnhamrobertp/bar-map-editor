# Run the same checks the GitHub CI runs, locally.
# Mirrors ci.yml: check -> test -> clippy -> fmt.
# RUSTFLAGS=-D warnings makes every warning a hard error, exactly as CI does.
#
# Usage:
#   .\scripts\ci-local.ps1            # run all steps
#   .\scripts\ci-local.ps1 clippy     # run one step by name
#
# Steps: check | test | bench | clippy | fmt

param(
    [string]$Step = "all"
)

$env:RUSTFLAGS = "-D warnings"
$env:CARGO_TERM_COLOR = "always"

$failed = @()

function Run-Step {
    param([string]$Name, [string[]]$Cmd)
    if ($Step -ne "all" -and $Step -ne $Name) { return }
    Write-Host ""
    Write-Host "=== $Name ===" -ForegroundColor Cyan
    & $Cmd[0] $Cmd[1..($Cmd.Length-1)]
    if ($LASTEXITCODE -ne 0) {
        $script:failed += $Name
        Write-Host "FAILED: $Name" -ForegroundColor Red
    } else {
        Write-Host "OK: $Name" -ForegroundColor Green
    }
}

Run-Step "check"  @("cargo", "check", "--workspace")
Run-Step "test"   @("cargo", "test",  "--workspace")
Run-Step "bench"  @("cargo", "build", "--benches", "--workspace")
Run-Step "clippy" @("cargo", "clippy", "--workspace", "--", "-D", "warnings")
Run-Step "fmt"    @("cargo", "fmt",   "--all", "--", "--check")

if ($failed.Count -gt 0) {
    Write-Host ""
    Write-Host "Failed steps: $($failed -join ', ')" -ForegroundColor Red
    exit 1
} elseif ($Step -eq "all") {
    Write-Host ""
    Write-Host "All steps passed." -ForegroundColor Green
}
