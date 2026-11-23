$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$tag = $env:MMO_UPDATE_TAG
if ($tag -cnotmatch '^(?<version>\d+\.\d+\.\d+)(?:-setup\.\d+)?$') { exit 1 }
$version = $Matches['version']

$repo = 'nekomimiyks/mmo-mouse-control'
$assetName = "MMOMouseControl-Setup-$version-x64.exe"
$download = "https://github.com/$repo/releases/download/v$tag/$assetName"
$folder = Join-Path ([IO.Path]::GetTempPath()) ('MMOMouseControl-update-' + ([guid]::NewGuid()).ToString('N'))
$installer = Join-Path $folder $assetName
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/tags/v$tag" -TimeoutSec 15
    $assets = @($release.assets | Where-Object { $_.name -ceq $assetName })
    if ($release.tag_name -cne "v$tag" -or $release.draft -or $release.prerelease -or $assets.Count -ne 1) {
        throw 'The public release is unavailable.'
    }
    $asset = $assets[0]
    if ($asset.browser_download_url -cne $download -or $asset.digest -cnotmatch '^sha256:[0-9a-f]{64}$' -or
        $asset.size -lt 1 -or $asset.size -gt 100MB) {
        throw 'Invalid installer metadata.'
    }

    [void][IO.Directory]::CreateDirectory($folder)
    Invoke-WebRequest -Uri $download -UseBasicParsing -TimeoutSec 60 -MaximumRedirection 5 -OutFile $installer
    $hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    if ((Get-Item -LiteralPath $installer).Length -ne $asset.size -or "sha256:$hash" -cne $asset.digest) {
        throw 'Installer verification failed.'
    }

    $arguments = '/LEGACYDIR="' + $env:MMO_UPDATE_FROM + '"'
    $setup = Start-Process -FilePath $installer -ArgumentList $arguments -PassThru
    [Console]::WriteLine('started')
    $setup.WaitForExit()
} catch {
    exit 1
} finally {
    if (Test-Path -LiteralPath $installer -PathType Leaf) { Remove-Item -LiteralPath $installer }
    if (Test-Path -LiteralPath $folder -PathType Container) { Remove-Item -LiteralPath $folder }
}
