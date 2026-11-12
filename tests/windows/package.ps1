# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
Installs the MSIX package that tools/windows-package.ps1 made, checks it and removes it
again: the signature, the alias `kudari`, and `kudari mcp`. Trusts its certificate for
that on the computer, so it is for a throwaway machine such as a runner of the CI.

    tests/windows/package.ps1 dist
#>
param([Parameter(Mandatory)] [string] $Dist)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$failures = 0
function Test-Check([string] $name, [bool] $passed) {
    if ($passed) {
        Write-Host "ok    $name"
    } else {
        Write-Host "FAIL  $name"
        $script:failures++
    }
}

$msix = Get-ChildItem $Dist -Filter '*.msix' | Select-Object -First 1
$cer = Get-ChildItem $Dist -Filter '*.cer' | Select-Object -First 1
$zip = Get-ChildItem $Dist -Filter '*.zip' | Select-Object -First 1
$certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 $cer.FullName

# The program in the archive carries the signature of the package.
$work = (New-Item -ItemType Directory -Force (Join-Path ([IO.Path]::GetTempPath()) "kudari-check-$PID")).FullName
Expand-Archive $zip.FullName -DestinationPath $work
$program = Get-ChildItem $work -Recurse -Filter 'kudari.exe' | Select-Object -First 1
$signature = Get-AuthenticodeSignature $program.FullName
Test-Check 'the program in the archive is signed' ($signature.SignerCertificate.Thumbprint -eq $certificate.Thumbprint)
Test-Check 'the archive holds the licenses' (Test-Path (Join-Path $program.DirectoryName 'licenses\Qt-LGPL-3.0-only.txt'))
Test-Check 'the archive holds the runtime of Visual C++' (Test-Path (Join-Path $program.DirectoryName 'vcruntime140.dll'))

# Windows installs a package only from a publisher it trusts.
$store = New-Object System.Security.Cryptography.X509Certificates.X509Store ('TrustedPeople', 'LocalMachine')
$store.Open('ReadWrite')
$store.Add($certificate)
try {
    Add-AppxPackage -Path $msix.FullName
    $package = Get-AppxPackage -Name 'io.github.marcjose.kudari'
    Test-Check 'the package installs' ($null -ne $package)

    $alias = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\kudari.exe'
    Test-Check 'the alias kudari exists' (Test-Path $alias)
    $version = & $alias --version
    Test-Check 'kudari --version names the version' ("$version" -match '^kudari \d+\.\d+\.\d+')
    $requests = @(
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}'
        '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
        '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_terminals","arguments":{}}}'
    )
    $answers = ($requests | & $alias mcp) -join "`n"
    Test-Check 'kudari mcp lists the tools' ($answers -match '"list_terminals"')
    Test-Check 'kudari mcp says that Kudari is not running' ($answers -match 'Kudari is not running')
    if ($failures) { Write-Host $answers }
} finally {
    Get-AppxPackage -Name 'io.github.marcjose.kudari' | Remove-AppxPackage
    $store.Remove($certificate)
    $store.Close()
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "$failures failed"
exit [int] ($failures -ne 0)
