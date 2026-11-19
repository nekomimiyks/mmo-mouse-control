param([switch]$TestOnly)

$ErrorActionPreference = 'Stop'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\winlibs\mingw64\bin;$env:PATH"
# GNUリンカーの出力先をASCIIパスにする。
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'mmo-mouse-build'

Push-Location (Join-Path $PSScriptRoot 'source')
try {
    cargo test --locked --offline --workspace
    if ($LASTEXITCODE -ne 0) { throw 'テストに失敗しました。' }
    if (-not $TestOnly) {
        cargo build --locked --offline --release --bin mmo-mouse-control
        if ($LASTEXITCODE -ne 0) { throw 'ビルドに失敗しました。' }
        $destination = Join-Path $PSScriptRoot '実行ファイル'
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        $old = Join-Path (Split-Path -Parent $PSScriptRoot) 'M908制御ソフト/実行ファイル'
        foreach ($name in @('m908.draft', 'm908-custom.txt')) {
            $source = Join-Path $old $name
            $target = Join-Path $destination $name
            if ((Test-Path -LiteralPath $source -PathType Leaf) -and -not (Test-Path -LiteralPath $target)) {
                Copy-Item -LiteralPath $source -Destination $target
            }
        }
        $exe = Join-Path $env:CARGO_TARGET_DIR 'release/mmo-mouse-control.exe'
        Copy-Item -LiteralPath $exe -Destination (Join-Path $destination 'MMOMouseControl.exe') -Force
    }
} finally {
    Pop-Location
}
