param(
    [string]$Version = "",
    [string]$Output = "assets",
    [ValidateSet("x86_64", "aarch64")]
    [string]$Architecture = "x86_64"
)

$ErrorActionPreference = "Stop"

$builds = @{
    x86_64  = @{ Flags = "-C target-cpu=x86-64-v3 -C target-feature=+crt-static"; Triple = ""; Label = "x86_64_v3"; Installs = "x64os" }
    aarch64 = @{ Flags = "-C target-cpu=generic -C target-feature=+crt-static"; Triple = "aarch64-pc-windows-msvc"; Label = "aarch64"; Installs = "arm64" }
}
$build = $builds[$Architecture]

function Find-InnoCompiler {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }
    $installed = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
    if (-not (Test-Path $installed)) {
        choco install innosetup --no-progress -y
        if ($LASTEXITCODE -ne 0) {
            throw "Inno Setup could not be installed"
        }
    }
    if (-not (Test-Path $installed)) {
        throw "ISCC.exe was not found after installing Inno Setup"
    }
    return $installed
}

$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
if ($Version -eq "") {
    $manifest = Get-Content (Join-Path $root "Cargo.toml") -Raw
    if ($manifest -notmatch '(?m)^version = "([^"]+)"') {
        throw "Cargo.toml declares no workspace version"
    }
    $Version = $Matches[1]
}
if ($Version -notmatch '^(\d+)\.(\d+)\.(\d+)') {
    throw "$Version is not a version"
}
$numeric = "$($Matches[1]).$($Matches[2]).$($Matches[3]).0"

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
$signInArguments = $arguments.Clone()
$signInArguments[$signInArguments.IndexOf("resonate")] = "resonate-signin"
cargo @signInArguments
if ($LASTEXITCODE -ne 0) {
    throw "the sign-in window's build failed"
}

$name = "resonate-$Version-$($build.Label)-windows"
$staging = Join-Path $Output $name
Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $staging | Out-Null

$executable = Join-Path $release "resonate.exe"
$signIn = Join-Path $release "resonate-signin.exe"
& (Join-Path $PSScriptRoot "sign.ps1") -Files $executable, $signIn

Copy-Item $executable $staging
Copy-Item $signIn $staging
Copy-Item (Join-Path $root "LICENSE") $staging
Copy-Item (Join-Path $root "COPYRIGHT") $staging
Copy-Item (Join-Path $PSScriptRoot "README.txt") $staging

$archive = Join-Path $Output "$name.zip"
Remove-Item -Force $archive -ErrorAction SilentlyContinue
Compress-Archive -Path $staging -DestinationPath $archive
Remove-Item -Recurse -Force $staging
Write-Output $archive

$setupName = "$name-setup"
$compiler = Find-InnoCompiler
& $compiler /Q `
    "/DAppVersion=$Version" `
    "/DNumericVersion=$numeric" `
    "/DInstallArchitecture=$($build.Installs)" `
    "/DSourceExecutable=$executable" `
    "/DSourceSignIn=$signIn" `
    "/DRepositoryRoot=$root" `
    "/DOutputDirectory=$((Resolve-Path $Output).Path)" `
    "/DOutputName=$setupName" `
    (Join-Path $PSScriptRoot "resonate.iss")
if ($LASTEXITCODE -ne 0) {
    throw "the installer could not be compiled"
}

$installer = Join-Path $Output "$setupName.exe"
& (Join-Path $PSScriptRoot "sign.ps1") -Files $installer
Write-Output $installer
