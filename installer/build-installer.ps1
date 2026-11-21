param(
    [string]$Iscc = 'ISCC.exe',
    [string]$OutputDir = ''
)

$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
if (-not $OutputDir) {
    $OutputDir = Join-Path $project 'target\installer'
}
$OutputDir = [System.IO.Path]::GetFullPath($OutputDir)

$cargo = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}
if (-not (Test-Path -LiteralPath $cargo)) {
    throw 'Cargo не найден'
}
if (-not (Test-Path -LiteralPath $Iscc)) {
    $resolved = (Get-Command $Iscc -ErrorAction SilentlyContinue).Source
    if (-not $resolved) { throw 'ISCC.exe не найден. Установите Inno Setup 7 или передайте -Iscc.' }
    $Iscc = $resolved
}

$manifest = Get-Content (Join-Path $project 'Cargo.toml') -Raw
$versionMatch = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) { throw 'Версия в Cargo.toml не найдена' }
$version = $versionMatch.Groups[1].Value

Push-Location $project
try {
    & $cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw "Cargo завершился с кодом $LASTEXITCODE" }
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    & $Iscc "/DAppVersion=$version" "--output-dir=$OutputDir" (Join-Path $PSScriptRoot 'ArizonaRustLauncher.iss')
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup завершился с кодом $LASTEXITCODE" }
    Join-Path $OutputDir "ArizonaRustLauncher-$version-Setup.exe"
}
finally {
    Pop-Location
}
