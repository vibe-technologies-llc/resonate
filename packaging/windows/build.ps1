param(
    [string]$Version = "",
    [string]$Output = "assets",
    [ValidateSet("x86_64", "aarch64")]
    [string]$Architecture = "x86_64"
)

$ErrorActionPreference = "Stop"

$builds = @{
    x86_64  = @{ Flags = "-C target-cpu=x86-64-v3"; Triple = ""; Label = "x86_64_v3" }
    aarch64 = @{ Flags = "-C target-cpu=generic"; Triple = "aarch64-pc-windows-msvc"; Label = "aarch64" }
}
$build = $builds[$Architecture]

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
if ($Version -eq "") {
    $manifest = Get-Content (Join-Path $root "Cargo.toml") -Raw
    if ($manifest -notmatch '(?m)^version = "([^"]+)"') {
        throw "Cargo.toml declares no workspace version"
    }
    $Version = $Matches[1]
}

$env:RUSTFLAGS = $build.Flags
$release = Join-Path $root "target\release"
$arguments = @("build", "--release", "--locked", "-p", "resonate")
if ($build.Triple -ne "") {
    rustup target add $build.Triple
    if ($LASTEXITCODE -ne 0) {
        throw "the target could not be installed"
    }
    $arguments += @("--target", $build.Triple)
    $release = Join-Path $root "target\$($build.Triple)\release"
}
cargo @arguments
if ($LASTEXITCODE -ne 0) {
    throw "the build failed"
}

$name = "resonate-$Version-$($build.Label)-windows"
$staging = Join-Path $Output $name
Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $staging | Out-Null

Copy-Item (Join-Path $release "resonate.exe") $staging
Copy-Item (Join-Path $root "LICENSE") $staging
Copy-Item (Join-Path $root "COPYRIGHT") $staging
Copy-Item (Join-Path $PSScriptRoot "README.txt") $staging

$archive = Join-Path $Output "$name.zip"
Remove-Item -Force $archive -ErrorAction SilentlyContinue
Compress-Archive -Path $staging -DestinationPath $archive
Remove-Item -Recurse -Force $staging
Write-Output $archive
