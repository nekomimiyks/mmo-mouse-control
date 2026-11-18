param(
    [string]$MakeAppx = $env:MAKEAPPX,
    [string]$PackageIdentityName = $env:MMO_STORE_PACKAGE_IDENTITY_NAME,
    [string]$PublisherDisplayName = $env:MMO_STORE_PUBLISHER_DISPLAY_NAME
)

if ($PackageIdentityName -cnotmatch '^[A-Za-z0-9.-]{3,50}$' -or
    [string]::IsNullOrWhiteSpace($PublisherDisplayName)) {
    throw 'Set PackageIdentityName and PublisherDisplayName to the values in Partner Center Product identity.'
}
$escapedPublisherDisplayName = [System.Security.SecurityElement]::Escape($PublisherDisplayName)

$ErrorActionPreference = 'Stop'
if (-not $MakeAppx) {
    $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $MakeAppx = Get-ChildItem -Path "$sdk/*/x64/makeappx.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (-not $MakeAppx) { throw 'Set MAKEAPPX to the Windows SDK makeappx.exe path.' }

$exe = Join-Path $PSScriptRoot '実行ファイル/MMOMouseControl.exe'
$manifest = Join-Path $PSScriptRoot 'source/Cargo.toml'
$version = [regex]::Match((Get-Content -LiteralPath $manifest -Raw), '(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
if (-not $version) { throw 'The application version is missing.' }
$dist = Join-Path $PSScriptRoot 'dist'
New-Item -ItemType Directory -Path $dist -Force | Out-Null
$stage = Join-Path $dist ('msix-' + [Guid]::NewGuid().ToString('N'))
$assets = Join-Path $stage 'Assets'
$output = Join-Path $dist "MMOMouseControl-$version-x64.msix"

try {
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    Copy-Item -LiteralPath $exe,(Join-Path $PSScriptRoot 'LICENSE'),(Join-Path $PSScriptRoot 'THIRD_PARTY.md') -Destination $stage
    Add-Type -AssemblyName System.Drawing
    $icon = [Drawing.Image]::FromFile((Join-Path $PSScriptRoot 'source/assets/icon.png'))
    try {
        foreach ($logo in @(@('StoreLogo',50), @('Square44x44Logo',44), @('Square150x150Logo',150))) {
            $size = $logo[1]
            $bitmap = [Drawing.Bitmap]::new($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.DrawImage($icon, 0, 0, $size, $size)
                $bitmap.Save((Join-Path $assets ($logo[0] + '.png')), [Drawing.Imaging.ImageFormat]::Png)
            } finally {
                $graphics.Dispose()
                $bitmap.Dispose()
            }
        }
    } finally { $icon.Dispose() }
    @"
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="$PackageIdentityName" Publisher="CN=5AE6A6DC-E993-473F-B455-E28AB21B84A6" Version="$version.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>MMO Mouse Control</DisplayName>
    <PublisherDisplayName>$escapedPublisherDisplayName</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Resources><Resource Language="en-us" /><Resource Language="ja-jp" /></Resources>
  <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" /></Dependencies>
  <Applications>
    <Application Id="App" Executable="MMOMouseControl.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="MMO Mouse Control" Description="Configure supported MMO mice on Windows"
          Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png" BackgroundColor="transparent" />
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
"@ | Set-Content -LiteralPath (Join-Path $stage 'AppxManifest.xml') -Encoding utf8
    & $MakeAppx pack /d $stage /p $output /o
    if ($LASTEXITCODE -ne 0) { throw 'MakeAppx could not create the Store package.' }
    Write-Output $output
} finally {
    if (Test-Path -LiteralPath $stage -PathType Container) {
        $resolved = (Resolve-Path -LiteralPath $stage).Path
        if ($resolved -eq $stage -and (Split-Path -Parent $resolved) -eq $dist) {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
}
