param(
    [string]$Iscc = $env:ISCC,
    [string]$Executable = (Join-Path $PSScriptRoot '実行ファイル/MMOMouseControl.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'dist')
)

$ErrorActionPreference = 'Stop'
if (!$Iscc) { $Iscc = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source }
if (!$Iscc) { throw 'Set ISCC to the path of the Inno Setup compiler.' }
$manifest = Join-Path $PSScriptRoot 'source/Cargo.toml'
$version = [regex]::Match((Get-Content -LiteralPath $manifest -Raw), '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
if (!$version) { throw 'The application version is missing.' }
if (!(Test-Path -LiteralPath $Executable -PathType Leaf)) { throw 'Build MMOMouseControl.exe first.' }

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$executablePath = (Resolve-Path -LiteralPath $Executable).Path
$outputPath = (Resolve-Path -LiteralPath $OutputDirectory).Path
& $Iscc "/DAppVersion=$version" "/DAppExe=$executablePath" "/DOutputFolder=$outputPath" (Join-Path $PSScriptRoot 'installer.iss')
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup could not build the installer.' }
