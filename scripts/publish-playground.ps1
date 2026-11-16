<#
.SYNOPSIS
    Builds the parsmith WebAssembly playground (wasm-bindgen + Vite) and publishes it to GitHub Pages (gh-pages) via git/gh.

.DESCRIPTION
    1. Compiles `parsmith-wasm` (`wasm32-unknown-unknown --release`) and emits browser bindings into `playground/src/wasm-pkg/`.
    2. Runs the Playground Vitest suite (unless `-SkipTests` is specified).
    3. Builds the production static bundle into `playground/dist/` (`tsc -b && vite build`), adds `.nojekyll`, and copies `.agents/skills/parsmith-dsl/`.
    4. Verifies GitHub CLI (`gh`) authentication and pushes `playground/dist/` to the `gh-pages` branch on `origin`.

.PARAMETER BuildOnly
    Only rebuild `playground/src/wasm-pkg/` and `playground/dist/` locally without pushing to `gh-pages`.

.PARAMETER SkipTests
    Skip running the Playground Vitest test suite before building.

.PARAMETER DryRun
    Build and stage the `gh-pages` commit, then print what would be pushed without modifying the remote `gh-pages` branch.
#>
[CmdletBinding()]
param(
    [switch]$BuildOnly,
    [switch]$SkipTests,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$PlaygroundDir = Join-Path $RepoRoot "playground"
$DistDir = Join-Path $PlaygroundDir "dist"

Push-Location $RepoRoot
try {
    # Auto-load .env from repo root if present
    $EnvFile = Join-Path $RepoRoot ".env"
    if (Test-Path $EnvFile) {
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

    Write-Host "==> [1/4] Building parsmith-wasm v$Version for web playground..." -ForegroundColor Cyan
    $installedTargets = rustup target list --installed
    if ($installedTargets -notcontains "wasm32-unknown-unknown") {
        rustup target add wasm32-unknown-unknown
        if ($LASTEXITCODE -ne 0) { throw "Failed to add wasm32-unknown-unknown target" }
    }

    Push-Location $PlaygroundDir
    try {
        npm run build:wasm
        if ($LASTEXITCODE -ne 0) { throw "npm run build:wasm failed" }

        if (-not $SkipTests) {
            Write-Host "==> [2/4] Running playground tests (Vitest)..." -ForegroundColor Cyan
            npm test -- --run
            if ($LASTEXITCODE -ne 0) { throw "Playground tests failed" }
        } else {
            Write-Host "==> [2/4] Skipping playground tests (-SkipTests)" -ForegroundColor DarkGray
        }

        Write-Host "==> [3/4] Building production playground bundle (playground/dist)..." -ForegroundColor Cyan
        if (Test-Path $DistDir) {
            Remove-Item -Recurse -Force $DistDir
        }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "npm run build failed" }
    } finally {
        Pop-Location
    }

    # Add .nojekyll to dist/ so GitHub Pages serves all static assets directly
    Set-Content -Path (Join-Path $DistDir ".nojekyll") -Value "" -NoNewline -Encoding ASCII

    if ($BuildOnly) {
        Write-Host "`n==> BuildOnly complete! Updated playground/src/wasm-pkg/ and playground/dist/." -ForegroundColor Green
        return
    }

    Write-Host "==> [4/4] Publishing playground/dist to GitHub Pages (gh-pages)..." -ForegroundColor Cyan
    gh auth status | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "GitHub CLI (gh) is not authenticated. Run 'gh auth login' or set GH_TOKEN in .env."
    }

    $RepoNwo = (gh repo view --json nameWithOwner -q ".nameWithOwner").Trim()
    if ([string]::IsNullOrWhiteSpace($RepoNwo)) {
        throw "Could not resolve GitHub repository via 'gh repo view'."
    }

    $TempDeployDir = Join-Path ([System.IO.Path]::GetTempPath()) "parsmith-gh-pages-$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
    New-Item -ItemType Directory -Force -Path $TempDeployDir | Out-Null

    try {
        Get-ChildItem -Path $DistDir -Force | ForEach-Object {
            Copy-Item -Recurse -Force $_.FullName (Join-Path $TempDeployDir $_.Name)
        }

        $RemoteUrl = (git remote get-url origin).Trim()
        $HeadSha = (git rev-parse --short HEAD).Trim()

        Push-Location $TempDeployDir
        try {
            git init --initial-branch=gh-pages | Out-Null
            git config core.autocrlf false
            git remote add origin $RemoteUrl
            git add -A
            git commit -m "deploy(playground): parsmith v$Version ($HeadSha)" | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "git commit in temporary gh-pages repo failed" }

            if ($DryRun) {
                Write-Host "    [DryRun] Staged gh-pages commit for $RepoNwo (parsmith v$Version, $HeadSha):" -ForegroundColor DarkGray
                git show --stat --oneline HEAD
            } else {
                gh auth setup-git | Out-Null
                git push --force origin gh-pages:gh-pages
                if ($LASTEXITCODE -ne 0) { throw "git push to origin/gh-pages failed" }

                $PagesUrl = (gh api "repos/$RepoNwo/pages" --jq ".html_url" 2>$null)
                Write-Host "`n==> Playground v$Version published to gh-pages!" -ForegroundColor Green
                if (-not [string]::IsNullOrWhiteSpace($PagesUrl)) {
                    Write-Host "    Live URL: $PagesUrl" -ForegroundColor Cyan
                }
            }
        } finally {
            Pop-Location
        }
    } finally {
        if (Test-Path $TempDeployDir) {
            Remove-Item -Recurse -Force $TempDeployDir -ErrorAction SilentlyContinue
        }
    }
} finally {
    Pop-Location
}
