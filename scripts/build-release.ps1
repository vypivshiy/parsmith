<#
.SYNOPSIS
    Local multi-target release builder for parsmith (Cargo, PyPI, npm, and GitHub Releases).

.DESCRIPTION
    Builds release artifacts locally on Windows (using Zig via maturin for Linux/macOS cross-compilation)
    and populates the Release/ directory:
      - Release/cargo/   : .crate source package (parsmith)
      - Release/pypi/    : sdist (.tar.gz) + abi3 wheels (Windows, Linux x64/arm64, macOS x64/arm64)
      - Release/npm/     : parsmith-<version>.tgz (native .node addons + universal WASM fallback)
      - Release/github/  : standalone parsmith CLI archives (.zip / .tar.gz) for GitHub Releases
      - Release/SHA256SUMS.txt

.PARAMETER SkipCross
    Build only native Windows artifacts + WASM fallback + sdist + Cargo crates (skips Zig Linux/macOS cross-compilation).

.PARAMETER SkipTests
    Skip running cargo/pytest/node test suites prior to building release artifacts.
#>
[CmdletBinding()]
param(
    [switch]$SkipCross,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $RepoRoot

try {
    # Read version from Cargo.toml
    $cargoToml = Get-Content -Raw (Join-Path $RepoRoot "Cargo.toml")
    if ($cargoToml -match '(?m)^version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        throw "Could not determine workspace version from Cargo.toml"
    }

    Write-Host "==> Preparing parsmith v$Version release in $RepoRoot\Release" -ForegroundColor Cyan

    # Locate Python / Maturin / Zig in .venv
    $VenvDir = Join-Path $RepoRoot ".venv"
    $VenvScripts = Join-Path $VenvDir "Scripts"
    $VenvZigDir = Join-Path $VenvDir "Lib\site-packages\ziglang"
    $MaturinExe = Join-Path $VenvScripts "maturin.exe"

    if (Test-Path $VenvZigDir) {
        $env:PATH = "$VenvZigDir;$VenvScripts;$env:PATH"
    } elseif (Test-Path $VenvScripts) {
        $env:PATH = "$VenvScripts;$env:PATH"
    }

    $ZigCmd = Get-Command zig -ErrorAction SilentlyContinue
    if (-not $SkipCross -and -not $ZigCmd) {
        Write-Host "==> Installing ziglang into .venv for cross-compilation..." -ForegroundColor Yellow
        uv pip install --group release
        if (Test-Path $VenvZigDir) {
            $env:PATH = "$VenvZigDir;$env:PATH"
        }
        $ZigCmd = Get-Command zig -ErrorAction SilentlyContinue
        if (-not $ZigCmd) {
            throw "zig.exe not found even after installing ziglang. Run with -SkipCross or install zig."
        }
    }

    # Prepare clean Release/ directories
    $ReleaseRoot = Join-Path $RepoRoot "Release"
    $CargoOut    = Join-Path $ReleaseRoot "cargo"
    $PyPIOut     = Join-Path $ReleaseRoot "pypi"
    $NpmOut      = Join-Path $ReleaseRoot "npm"
    $GitHubOut   = Join-Path $ReleaseRoot "github"
    $WrappersDir = Join-Path $ReleaseRoot ".zig-wrappers"

    if (Test-Path $ReleaseRoot) {
        Remove-Item -Recurse -Force $ReleaseRoot
    }
    New-Item -ItemType Directory -Force -Path $CargoOut, $PyPIOut, $NpmOut, $GitHubOut | Out-Null

    # -------------------------------------------------------------------------
    # 0. Optional Pre-flight Tests
    # -------------------------------------------------------------------------
    if (-not $SkipTests) {
        Write-Host "==> [0/5] Running Rust workspace tests..." -ForegroundColor Cyan
        cargo test --workspace
        if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }
    }

    # -------------------------------------------------------------------------
    # 1. Cargo Package (.crate)
    # -------------------------------------------------------------------------
    Write-Host "==> [1/5] Validating & packaging Cargo crate (parsmith)..." -ForegroundColor Cyan
    cargo package --list -p parsmith --allow-dirty | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "cargo package --list failed for parsmith" }

    cargo package -p parsmith --allow-dirty --no-verify
    if ($LASTEXITCODE -ne 0) { throw "cargo package failed for parsmith" }
    $crateFile = Join-Path $RepoRoot "target\package\parsmith-$Version.crate"
    Copy-Item -Force $crateFile (Join-Path $CargoOut "parsmith-$Version.crate")
    Write-Host "    Packaged parsmith-$Version.crate" -ForegroundColor Green

    # -------------------------------------------------------------------------
    # 2. Configure Zig Linker Wrappers for Cross Targets
    # -------------------------------------------------------------------------
    $Targets = @(
        @{
            Triple   = "x86_64-pc-windows-msvc"
            ZigTarget = $null
            NodeName = "parsmith.win32-x64.node"
            Cdylib   = "parsmith_js.dll"
            CliBin   = "parsmith.exe"
            Archive  = "zip"
        }
    )

    if (-not $SkipCross) {
        New-Item -ItemType Directory -Force -Path $WrappersDir | Out-Null
        $CrossTargets = @(
            @{
                Triple    = "x86_64-unknown-linux-gnu"
                ZigTarget = "x86_64-linux-gnu.2.17"
                EnvVar    = "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"
                NodeName  = "parsmith.linux-x64.node"
                Cdylib    = "libparsmith_js.so"
                CliBin    = "parsmith"
                Archive   = "tar.gz"
            },
            @{
                Triple    = "aarch64-unknown-linux-gnu"
                ZigTarget = "aarch64-linux-gnu.2.17"
                EnvVar    = "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"
                NodeName  = "parsmith.linux-arm64.node"
                Cdylib    = "libparsmith_js.so"
                CliBin    = "parsmith"
                Archive   = "tar.gz"
            },
            @{
                Triple    = "x86_64-apple-darwin"
                ZigTarget = "x86_64-macos.10.12-none"
                EnvVar    = "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER"
                NodeName  = "parsmith.darwin-x64.node"
                Cdylib    = "libparsmith_js.dylib"
                CliBin    = "parsmith"
                Archive   = "tar.gz"
            },
            @{
                Triple    = "aarch64-apple-darwin"
                ZigTarget = "aarch64-macos.11.0-none"
                EnvVar    = "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER"
                NodeName  = "parsmith.darwin-arm64.node"
                Cdylib    = "libparsmith_js.dylib"
                CliBin    = "parsmith"
                Archive   = "tar.gz"
            }
        )

        # Ensure rustup targets are installed
        $installedTargets = rustup target list --installed
        foreach ($ct in $CrossTargets) {
            if ($installedTargets -notcontains $ct.Triple) {
                Write-Host "    Adding rustup target $($ct.Triple)..." -ForegroundColor DarkGray
                rustup target add $ct.Triple
                if ($LASTEXITCODE -ne 0) { throw "Failed to add rustup target $($ct.Triple)" }
            }

            $batPath = Join-Path $WrappersDir "zigcc-$($ct.Triple).bat"
            $zigExePath = $ZigCmd.Source
            $batContent = @(
                "@echo off",
                "setlocal DisableDelayedExpansion",
                "set `"CARGO_ZIGBUILD_ZIG_COMMAND=$zigExePath`"",
                "`"$MaturinExe`" zig cc -- -g -fno-sanitize=all -target $($ct.ZigTarget) %*"
            ) -join "`r`n"
            Set-Content -Path $batPath -Value $batContent -Encoding ASCII
            Set-Item -Path "Env:$($ct.EnvVar)" -Value $batPath
            $Targets += $ct
        }
    }

    # -------------------------------------------------------------------------
    # 3. Build CLI & Node-API (.node) Binaries Across Targets
    # -------------------------------------------------------------------------
    Write-Host "==> [2/5] Building CLI (parsmith) and Node-API (parsmith-js) binaries..." -ForegroundColor Cyan
    $JsDir = Join-Path $RepoRoot "js"
    Get-ChildItem -Path $JsDir -Filter "*.node" -ErrorAction SilentlyContinue | Remove-Item -Force

    foreach ($t in $Targets) {
        $triple = $t.Triple
        Write-Host "    -> Building target: $triple" -ForegroundColor Green
        cargo build --release --target $triple -p parsmith -p parsmith-js
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $triple" }

        $targetReleaseDir = Join-Path $RepoRoot "target\$triple\release"

        # Copy .node addon into js/
        $cdylibPath = Join-Path $targetReleaseDir $t.Cdylib
        $nodeDest   = Join-Path $JsDir $t.NodeName
        Copy-Item -Force $cdylibPath $nodeDest

        # Package standalone CLI archive into Release/github/
        $stageName = "parsmith-v$Version-$triple"
        $stageDir  = Join-Path $ReleaseRoot $stageName
        New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
        Copy-Item -Force (Join-Path $targetReleaseDir $t.CliBin) (Join-Path $stageDir $t.CliBin)
        Copy-Item -Force (Join-Path $RepoRoot "README.md") (Join-Path $stageDir "README.md")
        Copy-Item -Force (Join-Path $RepoRoot "LICENSE") (Join-Path $stageDir "LICENSE")

        if ($t.Archive -eq "zip") {
            $archivePath = Join-Path $GitHubOut "$stageName.zip"
            Compress-Archive -Path "$stageDir\*" -DestinationPath $archivePath -Force
        } else {
            $archivePath = Join-Path $GitHubOut "$stageName.tar.gz"
            tar -czf $archivePath -C $ReleaseRoot $stageName
            if ($LASTEXITCODE -ne 0) { throw "tar failed for $stageName" }
        }
        Remove-Item -Recurse -Force $stageDir
    }

    # -------------------------------------------------------------------------
    # 4. Build WASM Fallback & Pack npm Package (Release/npm/)
    # -------------------------------------------------------------------------
    Write-Host "==> [3/5] Building Node.js WASM fallback and packing npm tarball..." -ForegroundColor Cyan
    $installedTargets = rustup target list --installed
    if ($installedTargets -notcontains "wasm32-unknown-unknown") {
        rustup target add wasm32-unknown-unknown
    }
    cargo build --release --target wasm32-unknown-unknown -p parsmith-wasm
    if ($LASTEXITCODE -ne 0) { throw "cargo build parsmith-wasm failed" }

    $JsWasmDir = Join-Path $JsDir "wasm"
    if (Test-Path $JsWasmDir) { Remove-Item -Recurse -Force $JsWasmDir }
    wasm-bindgen (Join-Path $RepoRoot "target\wasm32-unknown-unknown\release\parsmith_wasm.wasm") --out-dir $JsWasmDir --target nodejs
    if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen failed" }

    Copy-Item -Force (Join-Path $RepoRoot "README.md") (Join-Path $JsDir "README.md")
    Copy-Item -Force (Join-Path $RepoRoot "LICENSE") (Join-Path $JsDir "LICENSE")

    Push-Location $JsDir
    try {
        if (-not $SkipTests) {
            node --test tests/*.test.mjs
            if ($LASTEXITCODE -ne 0) { throw "Node.js native SDK tests failed" }
            $env:PARSMITH_FORCE_WASM = "1"
            node --test tests/*.test.mjs
            $wasmTestExit = $LASTEXITCODE
            Remove-Item Env:PARSMITH_FORCE_WASM -ErrorAction SilentlyContinue
            if ($wasmTestExit -ne 0) { throw "Node.js WASM fallback SDK tests failed" }
        }
        npm pack --pack-destination $NpmOut
        if ($LASTEXITCODE -ne 0) { throw "npm pack failed" }
    } finally {
        Pop-Location
    }

    # -------------------------------------------------------------------------
    # 5. Build Python sdist & Wheels via Maturin (Release/pypi/)
    # -------------------------------------------------------------------------
    Write-Host "==> [4/5] Building Python sdist and abi3 wheels..." -ForegroundColor Cyan
    # Clean local .pyd/.pdb in python/parsmith so sdist stays clean
    Get-ChildItem -Path (Join-Path $RepoRoot "python\parsmith") -Include "*.pyd", "*.pdb", "*.so", "*.dylib" -Recurse -ErrorAction SilentlyContinue | Remove-Item -Force

    # Native Windows wheel + sdist
    uv run maturin build --release --sdist --out $PyPIOut
    if ($LASTEXITCODE -ne 0) { throw "maturin build (Windows + sdist) failed" }

    if (-not $SkipCross) {
        foreach ($pyTarget in @("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin")) {
            Write-Host "    -> Building Python abi3 wheel for $pyTarget..." -ForegroundColor Green
            uv run maturin build --release --target $pyTarget --zig -i python --out $PyPIOut
            if ($LASTEXITCODE -ne 0) { throw "maturin build failed for $pyTarget" }
        }
    }

    # Restore the built Windows .pyd into python/parsmith/ so local `uv run pytest` remains ready
    $winWheel = Get-ChildItem -Path $PyPIOut -Filter "*-win_amd64.whl" | Select-Object -First 1
    if ($winWheel) {
        tar -xf $winWheel.FullName -C (Join-Path $RepoRoot "python") parsmith/_parsmith.pyd
    }

    # Cleanup temporary zig wrappers dir
    if (Test-Path $WrappersDir) {
        Remove-Item -Recurse -Force $WrappersDir
    }

    # -------------------------------------------------------------------------
    # 6. Generate SHA256SUMS.txt
    # -------------------------------------------------------------------------
    Write-Host "==> [5/5] Generating Release/SHA256SUMS.txt..." -ForegroundColor Cyan
    $sumLines = @()
    $allFiles = Get-ChildItem -Path $ReleaseRoot -Recurse -File | Where-Object { $_.Name -ne "SHA256SUMS.txt" } | Sort-Object FullName
    foreach ($f in $allFiles) {
        $hash = (Get-FileHash -Path $f.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        $rel  = $f.FullName.Substring($ReleaseRoot.Length + 1).Replace('\', '/')
        $sumLines += "$hash  $rel"
    }
    $sumsPath = Join-Path $ReleaseRoot "SHA256SUMS.txt"
    Set-Content -Path $sumsPath -Value ($sumLines -join "`n") -Encoding UTF8

    Write-Host "`n==> Release build complete! Artifacts in Release/:" -ForegroundColor Green
    Get-ChildItem -Path $ReleaseRoot -Recurse -File | Select-Object @{
        Name = "Artifact"; Expression = { $_.FullName.Substring($ReleaseRoot.Length + 1) }
    }, @{
        Name = "Size (KB)"; Expression = { [math]::Round($_.Length / 1KB, 1) }
    } | Format-Table -AutoSize
} finally {
    Pop-Location
}
