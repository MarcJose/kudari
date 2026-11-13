# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
GUI test of the Windows front end: starts Kudari with a configuration of its own and
drives its window through UI Automation, as a screen reader would, without mouse or keys.
It needs a desktop session, and no other Kudari of the same user may run, as the window
and the pipe for agents belong to one per user.

    tests/windows/gui.ps1 [path\to\kudari.exe]

Without a path, it takes the build of cargo, target\debug\kudari-windows.exe.
#>
param([string] $Program)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Program) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $Program = Join-Path $target 'debug\kudari-windows.exe'
}
if (-not (Test-Path $Program)) {
    Write-Host "No kudari-windows at $Program; build it with: cargo build -p kudari-windows"
    exit 2
}
# kudari.exe in the packages, kudari-windows.exe as cargo builds it.
if (Get-Process kudari, kudari-windows -ErrorAction SilentlyContinue) {
    Write-Host 'Kudari runs already; the test needs to be the only one.'
    exit 2
}

$failures = 0
function Test-Check([string] $name, [bool] $passed) {
    if ($passed) {
        Write-Host "ok    $name"
    } else {
        Write-Host "FAIL  $name"
        $script:failures++
    }
}

# A configuration of its own, so that the test touches nothing of the user's.
$work = (New-Item -ItemType Directory -Force (Join-Path ([IO.Path]::GetTempPath()) "kudari-gui-$PID")).FullName
$ini = Join-Path $work 'Kudari\kudari.ini'
New-Item -ItemType Directory -Force (Split-Path $ini) | Out-Null
Set-Content $ini "[Behavior]`nOpenAfterStart=true`n[Dialogs]`nFirstRun=false`n[Shortcuts]`ntoggle-window-state=Ctrl+Alt+F12"
$log = Join-Path $work 'log.txt'
$env:APPDATA = $work
$env:QT_FORCE_STDERR_LOGGING = '1'
# The texts the test looks for are the English ones of the code.
$env:LANGUAGE = 'en'
$process = Start-Process $Program -PassThru -RedirectStandardError $log -WorkingDirectory $work

$Automation = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$ByProcess = New-Object System.Windows.Automation.PropertyCondition ($Automation::ProcessIdProperty, $process.Id)

# Waits up to `seconds` for `condition` to give something.
function Wait-For([scriptblock] $condition, [double] $seconds = 5) {
    $until = (Get-Date).AddSeconds($seconds)
    do {
        $result = & $condition
        if ($result) { return $result }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $until)
    return $null
}

# The windows of Kudari: the one of the terminals, its menus, dialogs and the settings.
function Get-Windows {
    $Automation::RootElement.FindAll($Scope::Children, $ByProcess)
}

function Find-All([string] $name) {
    $byName = New-Object System.Windows.Automation.PropertyCondition ($Automation::NameProperty, $name)
    foreach ($window in Get-Windows) {
        $window.FindAll($Scope::Descendants, $byName)
    }
}

function Find-Element([string] $name, [double] $seconds = 5) {
    Wait-For { Find-All $name | Select-Object -First 1 } $seconds
}

function Find-ByType([System.Windows.Automation.ControlType] $type) {
    $byType = New-Object System.Windows.Automation.PropertyCondition ($Automation::ControlTypeProperty, $type)
    foreach ($window in Get-Windows) {
        $window.FindAll($Scope::Descendants, $byType)
    }
}

# Clicks what has the name and can be clicked. Qt lets labels be invoked as well, to no
# effect, so they do not count, and what can be switched is switched.
function Invoke-Element([string] $name) {
    $done = Wait-For {
        foreach ($element in Find-All $name) {
            if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Text) {
                continue
            }
            $pattern = $null
            if ($element.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref] $pattern)) {
                $pattern.Toggle()
                return $true
            }
            if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref] $pattern)) {
                $pattern.Invoke()
                return $true
            }
        }
    }
    return [bool] $done
}

function Get-TabCount {
    @(Find-ByType ([System.Windows.Automation.ControlType]::TabItem)).Count
}

function Get-Setting([string] $key) {
    $line = Get-Content $ini | Where-Object { $_ -match "^$key=" } | Select-Object -Last 1
    if ($line) { $line.Substring($key.Length + 1) } else { '' }
}

try {
    $window = Wait-For { Get-Windows | Where-Object { -not $_.Current.BoundingRectangle.IsEmpty } | Select-Object -First 1 } 15
    Test-Check 'the window opens' ($null -ne $window)

    $buttons = 'New Session', 'New Session With', 'Keep open when the focus moves elsewhere', 'Open Menu', 'Quit'
    $missing = @($buttons | Where-Object { -not (Find-Element $_ 3) })
    Test-Check 'the bar has its buttons' ($missing.Count -eq 0)
    if ($missing) { Write-Host "      missing: $($missing -join ', ')" }

    $tabs = Wait-For { Get-TabCount }
    [void] (Invoke-Element 'New Session')
    Test-Check 'a new session gets a tab' ($null -ne (Wait-For { if ((Get-TabCount) -eq $tabs + 1) { $true } }))
    [void] (Invoke-Element 'Close Session')
    Test-Check 'the close button of a tab closes its session' ($null -ne (Wait-For { if ((Get-TabCount) -eq $tabs) { $true } }))

    [void] (Invoke-Element 'Open Menu')
    $entries = 'About Kudari', 'Full Screen Mode', 'Width', 'Height', 'Shortcuts', 'Settings'
    $missing = @($entries | Where-Object { -not (Find-Element $_ 3) })
    Test-Check 'the menu shows all its entries' ($missing.Count -eq 0)
    if ($missing) { Write-Host "      missing: $($missing -join ', ')" }

    [void] (Invoke-Element 'About Kudari')
    Test-Check 'the about dialog opens' ($null -ne (Find-Element 'A drop-down terminal for Windows and KDE Plasma'))
    [void] (Invoke-Element 'Close')

    [void] (Invoke-Element 'Open Menu')
    [void] (Invoke-Element 'Settings')
    # It belongs to the window of the terminals, below which UI Automation shows it.
    $settingsWindow = New-Object System.Windows.Automation.AndCondition (
        (New-Object System.Windows.Automation.PropertyCondition ($Automation::NameProperty, 'Settings')),
        (New-Object System.Windows.Automation.PropertyCondition ($Automation::ControlTypeProperty, [System.Windows.Automation.ControlType]::Window)))
    $settings = Wait-For { foreach ($window in Get-Windows) { $window.FindFirst($Scope::Subtree, $settingsWindow) } }
    Test-Check 'the settings open from the menu' ($null -ne $settings)
    if (-not $settings) { Write-Host "      windows: $((Get-Windows | ForEach-Object { "'$($_.Current.Name)'" }) -join ', ')" }

    # Each page by a setting that only it has; the shortcuts page by an action.
    $pages = [ordered] @{
        'Window' = 'Show the tab bar'
        'Behavior' = 'Show an icon in the system tray'
        'Appearance' = 'Color scheme:'
        'Terminal' = 'Font family:'
        'Terminal Behavior' = 'Shell:'
        'Security' = 'Allow AI agents to read terminals'
        'Shortcuts' = 'Full Screen Mode'
    }
    $unseen = @()
    foreach ($page in $pages.Keys) {
        [void] (Invoke-Element $page)
        if (-not (Find-Element $pages[$page] 3)) { $unseen += $page }
    }
    Test-Check 'every page of the settings shows' ($unseen.Count -eq 0)
    if ($unseen) { Write-Host "      not shown: $($unseen -join ', ')" }

    [void] (Invoke-Element 'Window')
    $first = Invoke-Element 'Show the tab bar'
    $hidden = Wait-For { if ((Get-Setting 'ShowTabBar') -eq 'false') { $true } }
    $second = Invoke-Element 'Show the tab bar'
    $shown = Wait-For { if ((Get-Setting 'ShowTabBar') -eq 'true') { $true } }
    Test-Check 'a switch of the settings changes the setting' ($hidden -and $shown)
    if (-not ($hidden -and $shown)) {
        Write-Host "      clicked: $first $second, now: '$(Get-Setting 'ShowTabBar')'"
        foreach ($element in Find-All 'Show the tab bar') {
            Write-Host "      $($element.Current.ControlType.ProgrammaticName) patterns: $(($element.GetSupportedPatterns() | ForEach-Object ProgrammaticName) -join ', ')"
        }
        Get-Content $ini | ForEach-Object { Write-Host "      ini: $_" }
    }

    if ($settings) {
        $settings.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
    }

    [void] (Invoke-Element 'Quit')
    Test-Check 'the quit button ends Kudari' ($process.WaitForExit(10000))

    $errors = @(Get-Content $log -ErrorAction SilentlyContinue | Where-Object { $_ -match 'qrc:/|TypeError|ReferenceError|QML' })
    Test-Check 'the QML reports no errors' ($errors.Count -eq 0)
    if ($errors) { $errors | Select-Object -First 10 | ForEach-Object { Write-Host "      $_" } }
} catch {
    Test-Check "the test runs through: $($_.Exception.Message)" $false
} finally {
    if (-not $process.HasExited) { $process | Stop-Process -Force }
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host "$failures failed"
exit [int] ($failures -ne 0)
