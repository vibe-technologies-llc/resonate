param(
    [string]$Version = "",
    [string]$Output = "assets"
)

$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
if ($Version -eq "") {
    $manifest = Get-Content (Join-Path $root "Cargo.toml") -Raw
    if ($manifest -notmatch '(?m)^version = "([^"]+)"') {
        throw "Cargo.toml declares no workspace version"
    }
    $Version = $Matches[1]
}

$env:RUSTFLAGS = "-C target-cpu=x86-64-v3"
cargo build --release --locked -p resonate
if ($LASTEXITCODE -ne 0) {
    throw "the build failed"
}

$name = "resonate-$Version-x86_64_v3-windows"
$staging = Join-Path $Output $name
Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $staging | Out-Null

Copy-Item (Join-Path $root "target\release\resonate.exe") $staging
Copy-Item (Join-Path $root "LICENSE") $staging
Copy-Item (Join-Path $root "COPYRIGHT") $staging
Copy-Item (Join-Path $PSScriptRoot "README.txt") $staging

$archive = Join-Path $Output "$name.zip"
Remove-Item -Force $archive -ErrorAction SilentlyContinue
Compress-Archive -Path $staging -DestinationPath $archive
Remove-Item -Recurse -Force $staging
Write-Output $archive
