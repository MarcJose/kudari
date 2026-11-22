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

# The console host of Windows Terminal, signed by Microsoft, and the terminals use it.
$hosts = foreach ($name in 'conpty.dll', 'OpenConsole.exe') {
    $path = Join-Path $program.DirectoryName $name
    if (Test-Path $path) { Get-AuthenticodeSignature $path }
}
Test-Check 'the archive holds the console host of Windows Terminal, signed by Microsoft' (
    @($hosts | Where-Object { $_.Status -eq 'Valid' -and $_.SignerCertificate.Subject -like 'CN=Microsoft Corporation,*' }).Count -eq 2)
Test-Check 'the archive holds the license of the console host' (Test-Path (Join-Path $program.DirectoryName 'licenses\ConPTY-MIT.txt'))
$appdata = (New-Item -ItemType Directory -Force (Join-Path $work 'appdata\Kudari')).Parent.FullName
Set-Content (Join-Path $appdata 'Kudari\kudari.ini') "[Behavior]`nOpenAfterStart=true`n[Dialogs]`nFirstRun=false"
$userData = $env:APPDATA
$env:APPDATA = $appdata
$started = Start-Process $program.FullName -PassThru -WorkingDirectory $appdata
$env:APPDATA = $userData
$consoleHost = $null
foreach ($attempt in 1..40) {
    $consoleHost = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($started.Id)" | Where-Object Name -EQ 'OpenConsole.exe' | Select-Object -First 1
    if ($consoleHost) { break }
    Start-Sleep -Milliseconds 250
}
Test-Check 'a terminal runs in the console host of the archive' ($consoleHost -and $consoleHost.ExecutablePath -eq (Join-Path $program.DirectoryName 'OpenConsole.exe'))
foreach ($child in Get-CimInstance Win32_Process -Filter "ParentProcessId=$($started.Id)") { Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue }
Stop-Process -Id $started.Id -Force
[void] $started.WaitForExit(10000)

# kudari in a shell finds kudari.com, a program of the console, which the shell waits for
# and which hands on the exit code; kudari.exe has no console, and a shell went on at once.
$console = Join-Path $program.DirectoryName 'kudari.com'
$found = & {
    $env:PATH = "$($program.DirectoryName);$env:PATH"
    (Get-Command kudari -CommandType Application | Select-Object -First 1).Source
}
Test-Check 'kudari in a shell finds kudari.com' ($found -eq $console)
$bytes = [IO.File]::ReadAllBytes($console)
$header = [BitConverter]::ToInt32($bytes, 0x3C)
Test-Check 'kudari.com is a program of the console' ([BitConverter]::ToUInt16($bytes, $header + 0x5C) -eq 3)
Test-Check 'kudari.com is signed' ((Get-AuthenticodeSignature $console).SignerCertificate.Thumbprint -eq $certificate.Thumbprint)
# What kudari.exe answers comes through, in order, with its exit code.
$lines = @(cmd /c "`"$console`" --version & echo after")
Test-Check 'kudari.com hands on the answer of kudari.exe' ($lines.Count -eq 2 -and $lines[0] -match '^kudari \d+\.\d+\.\d+' -and $lines[1].Trim() -eq 'after')
& $console --version | Out-Null
Test-Check 'kudari.com hands on the exit code' ($LASTEXITCODE -eq 0)

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

    # From the package, a message of Kudari is a notification of Windows in the group of its
    # event, which Windows keeps in its list: here that Windows refuses the shortcut, which the
    # test holds. Windows makes a message through the tray icon of a package a notification as
    # well, but one without a group and with the icon shown for it. The settings of the
    # package go where Windows keeps the files of the package, not into those of the user.
    # Windows shows notifications only on a desktop where the user has them on; Windows
    # PowerShell reads them, as PowerShell 7 does not know the classes of Windows Runtime.
    $application = "$($package.PackageFamilyName)!Kudari"
    $notifications = {
        param([string] $application, [string] $what)
        [void] [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
        if ($what -eq 'setting') {
            [string] [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($application).Setting
        } else {
            [Windows.UI.Notifications.ToastNotificationManager]::History.GetHistory($application) |
                ForEach-Object { "$($_.Group) $($_.Content.GetXml())" }
        }
    }
    $setting = powershell.exe -NoProfile -Command $notifications -args $application, 'setting'
    if ($setting -ne 'Enabled' -or -not [Environment]::UserInteractive) {
        Write-Host "skip  a message of Kudari from the package is a notification of Windows: they are '$setting' here"
    } else {
        $settings = Join-Path $env:LOCALAPPDATA "Packages\$($package.PackageFamilyName)\LocalCache\Roaming\Kudari"
        New-Item -ItemType Directory -Force $settings | Out-Null
        Set-Content (Join-Path $settings 'kudari.ini') "[Dialogs]`nFirstRun=false`n[Shortcuts]`ntoggle-window-state=Ctrl+Alt+Num+5"
        Add-Type -Namespace KudariPackage -Name Hotkeys -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool RegisterHotKey(System.IntPtr window, int id, uint modifiers, uint key);
[DllImport("user32.dll")] public static extern bool UnregisterHotKey(System.IntPtr window, int id);
'@
        $held = [KudariPackage.Hotkeys]::RegisterHotKey([IntPtr]::Zero, 1, 0x4003, 0x65)
        Start-Process "shell:AppsFolder\$application"
        $told = $null
        foreach ($attempt in 1..30) {
            $told = @(powershell.exe -NoProfile -Command $notifications -args $application, 'history') -match '^shortcut .*does not give Kudari the shortcut'
            if ($told) { break }
            Start-Sleep -Milliseconds 500
        }
        Test-Check 'a message of Kudari from the package is a notification of Windows in the group of its event' ($held -and [bool] $told)
        Get-Process kudari -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$($package.InstallLocation)*" } | Stop-Process -Force
        [void] [KudariPackage.Hotkeys]::UnregisterHotKey([IntPtr]::Zero, 1)
    }
} finally {
    Get-AppxPackage -Name 'io.github.marcjose.kudari' | Remove-AppxPackage
    $store.Remove($certificate)
    $store.Close()
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "$failures failed"
exit [int] ($failures -ne 0)
