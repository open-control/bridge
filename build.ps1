# Open Control Bridge - Cross-Platform Build Script (PowerShell)
# Usage:
#   .\build.ps1                  # Build for current platform
#   .\build.ps1 -Target windows  # Build for Windows
#   .\build.ps1 -Target linux    # Build for Linux
#   .\build.ps1 -Target all      # Build for all platforms
#   .\build.ps1 -Setup           # Install Rust targets

param(
    [ValidateSet("native", "windows", "linux", "all")]
    [string]$Target = "native",
    [switch]$Debug,
    [switch]$Setup,
    [switch]$Clean
)

$ErrorActionPreference = "Stop"

$BinaryName = "oc-bridge"
$TargetWindows = "x86_64-pc-windows-gnu"
$TargetLinux = "x86_64-unknown-linux-gnu"
$DistDir = "dist"

function Write-Header($msg) {
    Write-Host "`n=== $msg ===" -ForegroundColor Cyan
}

function Build-Target($targetTriple, $outputDir, $ext) {
    $releaseFlag = if ($Debug) { "" } else { "--release" }
    $buildType = if ($Debug) { "debug" } else { "release" }

    Write-Header "Building for $targetTriple"

    $cmd = "cargo build $releaseFlag --target $targetTriple"
    Write-Host $cmd -ForegroundColor Yellow
    Invoke-Expression $cmd

    if ($LASTEXITCODE -ne 0) {
        throw "Build failed for $targetTriple"
    }

    # Copy to dist
    $null = New-Item -ItemType Directory -Force -Path $outputDir
    $srcPath = "target/$targetTriple/$buildType/$BinaryName$ext"
    $dstPath = "$outputDir/$BinaryName$ext"

    if (Test-Path $srcPath) {
        Copy-Item $srcPath $dstPath -Force
        Write-Host "Output: $dstPath" -ForegroundColor Green
    }
}

# Setup targets
if ($Setup) {
    Write-Header "Installing Rust targets"
    rustup target add $TargetWindows
    rustup target add $TargetLinux
    Write-Host "Targets installed." -ForegroundColor Green
    exit 0
}

# Clean
if ($Clean) {
    Write-Header "Cleaning build artifacts"
    cargo clean
    if (Test-Path $DistDir) {
        Remove-Item -Recurse -Force $DistDir
    }
    Write-Host "Clean complete." -ForegroundColor Green
    exit 0
}

# Build
switch ($Target) {
    "native" {
        Write-Header "Building for current platform"
        $releaseFlag = if ($Debug) { "" } else { "--release" }
        Invoke-Expression "cargo build $releaseFlag"
    }
    "windows" {
        Build-Target $TargetWindows "$DistDir/windows" ".exe"
    }
    "linux" {
        Build-Target $TargetLinux "$DistDir/linux" ""
    }
    "all" {
        Build-Target $TargetWindows "$DistDir/windows" ".exe"
        Build-Target $TargetLinux "$DistDir/linux" ""
        Write-Header "All builds complete"
        Write-Host "Outputs in $DistDir/" -ForegroundColor Green
    }
}

Write-Host "`nBuild complete!" -ForegroundColor Green
