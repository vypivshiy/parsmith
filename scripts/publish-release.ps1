<#
.SYNOPSIS
    Publishes parsmith release artifacts from Release/ to crates.io, PyPI, npm, and GitHub Releases.

.PARAMETER Cargo
    Publish parsmith to crates.io.

.PARAMETER PyPI
    Upload wheels and sdist from Release/pypi/ to PyPI via maturin.

.PARAMETER Npm
    Publish the packed npm tarball from Release/npm/ to npmjs.com.

.PARAMETER GitHub
    Create a GitHub Release for v<Version> and upload Release/github/* + Release/SHA256SUMS.txt using `gh`.

.PARAMETER All
    Publish to all four targets (Cargo, PyPI, npm, GitHub).

.PARAMETER DryRun
    Verify publishing commands without uploading to remote registries.
#>
[CmdletBinding()]
param(
    [switch]$Cargo,
    [switch]$PyPI,
    [switch]$Npm,
    [switch]$GitHub,
    [switch]$All,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $RepoRoot

try {
    if (-not ($Cargo -or $PyPI -or $Npm -or $GitHub -or $All)) {
        Write-Host "Usage: .\scripts\publish-release.ps1 [-Cargo] [-PyPI] [-Npm] [-GitHub] [-All] [-DryRun]" -ForegroundColor Yellow
        return
    }

    # Auto-load .env from repo root if present
    $EnvFile = Join-Path $RepoRoot ".env"
    if (Test-Path $EnvFile) {
        Write-Host "==> Loading credentials from .env" -ForegroundColor DarkGray
        foreach ($line in Get-Content $EnvFile) {
            $trimmed = $line.Trim()
            if ([string]::IsNullOrWhiteSpace($trimmed) -or $trimmed.StartsWith("#")) {
                continue
            }
            if ($trimmed -match '^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$') {
                $key = $Matches[1]
                $val = $Matches[2].Trim()
                if (($val.StartsWith('"') -and $val.EndsWith('"')) -or ($val.StartsWith("'") -and $val.EndsWith("'"))) {
                    $val = $val.Substring(1, $val.Length - 2)
                }
                if (-not [string]::IsNullOrWhiteSpace($val)) {
                    Set-Item -Path "Env:$key" -Value $val
                }
            }
        }
    }

    $cargoToml = Get-Content -Raw (Join-Path $RepoRoot "Cargo.toml")
    if ($cargoToml -match '(?m)^version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        throw "Could not determine workspace version from Cargo.toml"
    }

    $ReleaseRoot = Join-Path $RepoRoot "Release"
    if (-not (Test-Path $ReleaseRoot)) {
        throw "Release/ directory not found. Run .\scripts\build-release.ps1 first."
    }

    # 1. Cargo (crates.io)
    if ($Cargo -or $All) {
        Write-Host "==> Publishing Cargo crate parsmith (v$Version) to crates.io..." -ForegroundColor Cyan
        if ($DryRun) {
            Write-Host "    [DryRun] Validating parsmith package..." -ForegroundColor DarkGray
            cargo publish -p parsmith --dry-run --allow-dirty
            if ($LASTEXITCODE -ne 0) { throw "cargo publish --dry-run failed for parsmith" }
        } else {
            Write-Host "    Publishing parsmith..." -ForegroundColor Green
            cargo publish -p parsmith
            if ($LASTEXITCODE -ne 0) { throw "cargo publish failed for parsmith" }
        }
    }

    # 2. PyPI
    if ($PyPI -or $All) {
        Write-Host "==> Uploading Python wheels & sdist from Release\pypi\ to PyPI..." -ForegroundColor Cyan
        $pypiFiles = Get-ChildItem -Path (Join-Path $ReleaseRoot "pypi\*") -Include "*.whl", "*.tar.gz"
        if ($pypiFiles.Count -eq 0) {
            throw "No wheels or sdist found in Release\pypi\"
        }
        if ($DryRun) {
            Write-Host "    [DryRun] Would upload $($pypiFiles.Count) files via 'uv run maturin upload':" -ForegroundColor DarkGray
            $pypiFiles | ForEach-Object { Write-Host "      - $($_.Name)" }
        } else {
            uv run maturin upload "$ReleaseRoot\pypi\*"
            if ($LASTEXITCODE -ne 0) { throw "maturin upload failed" }
        }
    }

    # 3. npm
    if ($Npm -or $All) {
        Write-Host "==> Publishing npm package from Release\npm\..." -ForegroundColor Cyan
        $tgz = Join-Path $ReleaseRoot "npm\parsmith-$Version.tgz"
        if (-not (Test-Path $tgz)) {
            throw "npm tarball not found at $tgz"
        }
        $npmAuthArgs = @()
        if (-not [string]::IsNullOrWhiteSpace($env:NPM_TOKEN)) {
            $npmAuthArgs += "--//registry.npmjs.org/:_authToken=$($env:NPM_TOKEN)"
        }
        if ($DryRun) {
            npm publish $tgz --dry-run @npmAuthArgs
            if ($LASTEXITCODE -ne 0) { throw "npm publish --dry-run failed" }
        } else {
            npm publish $tgz --access public @npmAuthArgs
            if ($LASTEXITCODE -ne 0) { throw "npm publish failed" }
        }
    }

    # 4. GitHub Releases
    if ($GitHub -or $All) {
        Write-Host "==> Publishing GitHub Release v$Version..." -ForegroundColor Cyan
        $ghFiles = @()
        $ghFiles += (Get-ChildItem -Path (Join-Path $ReleaseRoot "github\*")).FullName
        $ghFiles += (Get-ChildItem -Path (Join-Path $ReleaseRoot "pypi\*")).FullName
        $ghFiles += (Get-ChildItem -Path (Join-Path $ReleaseRoot "npm\*")).FullName
        $sumsFile = Join-Path $ReleaseRoot "SHA256SUMS.txt"
        if (Test-Path $sumsFile) { $ghFiles += $sumsFile }

        if ($DryRun) {
            Write-Host "    [DryRun] gh release create v$Version with $($ghFiles.Count) assets:" -ForegroundColor DarkGray
            $ghFiles | ForEach-Object { Write-Host "      - $(Split-Path $_ -Leaf)" }
        } else {
            gh release create "v$Version" @ghFiles --title "parsmith v$Version" --generate-notes
            if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
        }
    }
} finally {
    Pop-Location
}
