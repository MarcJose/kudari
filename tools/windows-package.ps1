# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
Makes the Windows packages of Kudari: a ZIP archive and an MSIX package with the program,
Qt, the runtime of Visual C++ and the licenses, the program and the package signed.

.DESCRIPTION
    tools/windows-package.ps1 -Out dist [-Certificate signing.pfx] [-Timestamp] [-Sbom crates.cdx.json]

Needs Qt for MSVC (qmake and windeployqt on the path, QMAKE set for cxx-qt), the Windows
SDK, Visual Studio with the C++ tools, and Python. The certificate signs both; its subject
becomes the publisher of the package. Its password comes from the environment variable
KUDARI_CERTIFICATE_PASSWORD. Without a certificate, the script makes a throwaway
self-signed one, enough for checks. -Timestamp has the signatures timestamped, so that
they stay valid after the certificate expired. -Sbom completes the SBOM of the crates,
from cargo-cyclonedx, with Qt and the runtime.

Writes to OUT: kudari-VERSION-x86_64-windows.zip, .msix, .cer (the public certificate
for users to trust), and with -Sbom .cdx.json.
#>
param(
    [Parameter(Mandatory)] [string] $Out,
    [string] $Certificate,
    [switch] $Timestamp,
    [string] $Sbom
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

# The texts of the licenses of Qt, which Kudari uses under the LGPL 3.
$QtLicenses = @{
    'LGPL-3.0-only.txt' = 'da7eabb7bafdf7d3ae5e9f223aa5bdc1eece45ac569dc21b3b037520b4464768'
    'GPL-3.0-only.txt'  = '8ceb4b9ee5adedde47b31e975c1d90c73ad27b6b165a1dcd80c7c545eb65b903'
}
$QtLicenseUrl = 'https://raw.githubusercontent.com/qt/qtbase/v6.8.3/LICENSES'
$TimestampUrl = 'http://timestamp.digicert.com'

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$name = "kudari-$version"
# MSIX wants four numbers and nothing after them.
$packageVersion = ($version -split '-')[0] + '.0'
$Out = (New-Item -ItemType Directory -Force $Out).FullName
$work = (New-Item -ItemType Directory -Force (Join-Path ([IO.Path]::GetTempPath()) "kudari-package-$PID")).FullName
$stage = Join-Path $work $name

function Find-Tool([string] $tool) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem $kits -Directory | Where-Object Name -Match '^\d+\.\d+\.\d+\.\d+$' |
        Sort-Object { [version] $_.Name } -Descending |
        ForEach-Object { Join-Path $_.FullName "x64\$tool" } | Where-Object { Test-Path $_ } |
        Select-Object -First 1
    if (-not $found) { throw "No $tool in the Windows SDK" }
    $found
}
$makeappx = Find-Tool 'makeappx.exe'
$signtool = Find-Tool 'signtool.exe'

# The certificate goes into the store of the user for signtool, which then needs no
# password on its command line, and out again at the end.
$X509 = 'System.Security.Cryptography.X509Certificates'
$keep = [Security.Cryptography.X509Certificates.X509KeyStorageFlags] 'PersistKeySet, UserKeySet'
if ($Certificate) {
    $password = $env:KUDARI_CERTIFICATE_PASSWORD
    if (-not $password) { throw 'KUDARI_CERTIFICATE_PASSWORD is not set' }
    $signer = New-Object "$X509.X509Certificate2" ((Resolve-Path $Certificate).Path, $password, $keep)
} else {
    Write-Host 'No certificate: signing with a throwaway one.'
    $key = [Security.Cryptography.RSA]::Create(3072)
    $request = New-Object "$X509.CertificateRequest" ('CN=Kudari Test', $key,
        [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $request.CertificateExtensions.Add((New-Object "$X509.X509KeyUsageExtension" (
        [Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true)))
    $codeSigning = New-Object Security.Cryptography.OidCollection
    [void] $codeSigning.Add((New-Object Security.Cryptography.Oid '1.3.6.1.5.5.7.3.3'))
    $request.CertificateExtensions.Add((New-Object "$X509.X509EnhancedKeyUsageExtension" ($codeSigning, $false)))
    $request.CertificateExtensions.Add((New-Object "$X509.X509BasicConstraintsExtension" ($false, $false, 0, $true)))
    $made = $request.CreateSelfSigned([DateTimeOffset]::Now.AddMinutes(-5), [DateTimeOffset]::Now.AddDays(2))
    # Through PKCS #12, so that Windows keeps the key for signtool.
    $signer = New-Object "$X509.X509Certificate2" ($made.Export('Pfx', 'throwaway'), 'throwaway', $keep)
}
$store = New-Object "$X509.X509Store" ('My', 'CurrentUser')
$store.Open('ReadWrite')
$store.Add($signer)
function Invoke-Sign([string] $file) {
    $arguments = @('sign', '/fd', 'SHA256', '/sha1', $signer.Thumbprint, '/s', 'My')
    if ($Timestamp) { $arguments += @('/tr', $TimestampUrl, '/td', 'SHA256') }
    & $signtool @arguments $file
}

try {
    $target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
    cargo +stable build --locked --release --package kudari-windows
    New-Item -ItemType Directory $stage | Out-Null
    # Users know the program as kudari; its crate needs another name next to the one of KDE.
    Copy-Item (Join-Path $target 'release\kudari-windows.exe') (Join-Path $stage 'kudari.exe')

    # Qt, with the QML modules that the pages import; Windows brings Direct3D along. Neither
    # the QML debugger (`qmltooling`) nor touch input over the network (`generic`) is used, and
    # the translations of Qt are needed only for the languages of Kudari.
    windeployqt --release --no-compiler-runtime --no-opengl-sw --no-system-d3d-compiler `
        --skip-plugin-types qmltooling,generic --translations de,en `
        --qmldir crates\windows\qml --qmldir crates\qt\qml --dir $stage (Join-Path $stage 'kudari.exe')
    # windeployqt takes every style that QtQuick.Controls might load. Kudari sets FluentWinUI3,
    # which builds on Fusion and Basic, so the other styles and the native one stay out.
    foreach ($style in 'Material', 'Imagine', 'Universal', 'Windows') {
        Remove-Item -Recurse (Join-Path $stage "qml\QtQuick\Controls\$style")
        Remove-Item (Join-Path $stage "Qt6QuickControls2$style*.dll")
    }
    Remove-Item -Recurse (Join-Path $stage 'qml\QtQuick\NativeStyle')

    # The runtime of Visual C++ next to the program, as the package cannot run its installer.
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $studio = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $redist = Get-ChildItem (Join-Path $studio 'VC\Redist\MSVC') -Directory | Where-Object Name -Match '^\d+\.\d+\.\d+$' |
        Sort-Object { [version] $_.Name } -Descending | Select-Object -First 1
    $runtime = Get-ChildItem (Join-Path $redist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Select-Object -First 1
    Copy-Item (Join-Path $runtime.FullName '*.dll') $stage

    $licenses = (New-Item -ItemType Directory (Join-Path $stage 'licenses')).FullName
    Copy-Item LICENSES\MIT.txt (Join-Path $licenses 'Kudari-MIT.txt')
    Copy-Item THIRD_PARTY_LICENSES (Join-Path $licenses 'THIRD_PARTY_LICENSES.txt')
    foreach ($file in $QtLicenses.Keys) {
        $path = Join-Path $licenses "Qt-$file"
        Invoke-WebRequest "$QtLicenseUrl/$file" -OutFile $path
        if ((Get-FileHash $path -Algorithm SHA256).Hash -ne $QtLicenses[$file]) { throw "Unexpected text of $file" }
    }
    $qtVersion = (qmake -query QT_VERSION).Trim()
    @"
Kudari $version is under the MIT license, in Kudari-MIT.txt. The Rust crates it is built
from are listed with their licenses in THIRD_PARTY_LICENSES.txt.

It uses Qt $qtVersion, unchanged, under the GNU Lesser General Public License version 3, in
Qt-LGPL-3.0-only.txt with the GNU General Public License it refers to in
Qt-GPL-3.0-only.txt. The Qt libraries are the Qt6*.dll files and the plugins and QML
modules beside them, which can be replaced by others of the same version. The source of
Qt is at https://download.qt.io/official_releases/qt/.

The runtime of Microsoft Visual C++ $($redist.Name) (msvcp140*.dll, vcruntime140*.dll,
concrt140.dll) is redistributed under the license terms of Microsoft Visual Studio.
"@ | Set-Content (Join-Path $licenses 'README.txt') -Encoding utf8NoBOM

    Invoke-Sign (Join-Path $stage 'kudari.exe')

    $zip = Join-Path $Out "$name-x86_64-windows.zip"
    Compress-Archive -Path $stage -DestinationPath $zip -Force

    # The package: the same files, with the manifest and the logos.
    $layout = Join-Path $work 'msix'
    Copy-Item $stage $layout -Recurse
    # Rendered from the icon by tools/icons.py, like the icon of the program.
    Copy-Item crates\windows\package\Assets (Join-Path $layout 'Assets') -Recurse
    $manifest = (Get-Content crates\windows\package\AppxManifest.xml -Raw).
        Replace('@VERSION@', $packageVersion).
        Replace('@PUBLISHER@', [Security.SecurityElement]::Escape($signer.Subject))
    Set-Content (Join-Path $layout 'AppxManifest.xml') $manifest -Encoding utf8NoBOM -NoNewline
    $msix = Join-Path $Out "$name-x86_64-windows.msix"
    & $makeappx pack /o /h SHA256 /d $layout /p $msix
    Invoke-Sign $msix

    [IO.File]::WriteAllBytes((Join-Path $Out "$name-x86_64-windows.cer"), $signer.Export('Cert'))

    if ($Sbom) {
        $cdx = Join-Path $Out "$name-x86_64-windows.cdx.json"
        python tools/windows_sbom.py $Sbom $stage $qtVersion $redist.Name | Set-Content $cdx -Encoding utf8NoBOM
    }
    Get-ChildItem $Out | ForEach-Object { Write-Host $_.FullName }
} finally {
    $store.Remove($signer)
    $store.Close()
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
