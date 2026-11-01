param(
    [Parameter(Mandatory = $true)]
    [string[]]$Files
)

$ErrorActionPreference = "Stop"

$encoded = $env:RESONATE_WINDOWS_PFX_BASE64
$password = $env:RESONATE_WINDOWS_PFX_PASSWORD
$timestamp = "http://timestamp.digicert.com"

if ([string]::IsNullOrEmpty($encoded) -or [string]::IsNullOrEmpty($password)) {
    Write-Output "no Authenticode certificate is configured, so $($Files.Count) file(s) stay unsigned"
    return
}

$tools = Get-ChildItem (Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin\*\x64\signtool.exe")
$signtool = $tools | Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
if ($null -eq $signtool) {
    throw "signtool.exe was not found among the Windows SDK tools"
}

$certificate = Join-Path ([IO.Path]::GetTempPath()) "resonate-signing.pfx"
[IO.File]::WriteAllBytes($certificate, [Convert]::FromBase64String($encoded))
try {
    foreach ($file in $Files) {
        & $signtool.FullName sign /f $certificate /p $password /fd sha256 /tr $timestamp /td sha256 $file
        if ($LASTEXITCODE -ne 0) {
            throw "$file could not be signed"
        }
        & $signtool.FullName verify /pa $file
        if ($LASTEXITCODE -ne 0) {
            throw "$file does not verify after signing"
        }
    }
}
finally {
    Remove-Item -Force $certificate -ErrorAction SilentlyContinue
}
