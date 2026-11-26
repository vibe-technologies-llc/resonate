param(
    [string]$Version = "",
    [string]$Output = "assets",
    [ValidateSet("x86_64", "aarch64")]
    [string]$Architecture = "x86_64",
    [ValidateSet("Whole", "Programs", "Package")]
    [string]$Stage = "Whole",
    [string]$Programs = ""
)

$ErrorActionPreference = "Stop"

$builds = @{
    x86_64  = @{ Flags = "-C target-cpu=x86-64-v3 -C target-feature=+crt-static"; Triple = ""; Label = "x86_64_v3"; Installs = "x64os" }
    aarch64 = @{ Flags = "-C target-cpu=generic -C target-feature=+crt-static"; Triple = "aarch64-pc-windows-msvc"; Label = "aarch64"; Installs = "arm64" }
}
$build = $builds[$Architecture]
$programNames = @("resonate.exe", "resonate-signin.exe")

function Find-InnoCompiler {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }
    $installed = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
    if (-not (Test-Path $installed)) {
        choco install innosetup --no-progress -y | Out-Host
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

function Build-Programs {
    $env:RUSTFLAGS = $build.Flags
    $release = Join-Path $root "target\release"
    $arguments = @("build", "--release", "--locked", "-p", "resonate")
    if ($build.Triple -ne "") {
        rustup target add $build.Triple | Out-Host
        if ($LASTEXITCODE -ne 0) {
            throw "the target could not be installed"
        }
        $arguments += @("--target", $build.Triple)
        $release = Join-Path $root "target\$($build.Triple)\release"
    }
    cargo @arguments | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "the build failed"
    }
    $signInArguments = $arguments.Clone()
    $signInArguments[$signInArguments.IndexOf("resonate")] = "resonate-signin"
    cargo @signInArguments | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "the sign-in window's build failed"
    }
    return $release
}

function Write-Package([string]$From) {
    $executable = (Resolve-Path (Join-Path $From "resonate.exe")).Path
    $signIn = (Resolve-Path (Join-Path $From "resonate-signin.exe")).Path

    $name = "resonate-$Version-$($build.Label)-windows"
    $staging = Join-Path $Output $name
    Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $staging | Out-Null

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
        (Join-Path $PSScriptRoot "resonate.iss") | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "the installer could not be compiled"
    }
    Write-Output (Join-Path $Output "$setupName.exe")
}

New-Item -ItemType Directory -Force $Output | Out-Null
$signing = Join-Path $PSScriptRoot "sign.ps1"
switch ($Stage) {
    "Programs" {
        $release = Build-Programs
        foreach ($program in $programNames) {
            Copy-Item (Join-Path $release $program) $Output
            Write-Output (Join-Path $Output $program)
        }
    }
    "Package" {
        if ($Programs -eq "") {
            throw "-Stage Package packs the programs a -Programs folder holds"
        }
        Write-Package $Programs
    }
    "Whole" {
        $release = Build-Programs
        & $signing -Files ($programNames | ForEach-Object { Join-Path $release $_ })
        $written = Write-Package $release
        $written
        & $signing -Files ($written | Where-Object { $_ -like "*-setup.exe" })
    }
}
