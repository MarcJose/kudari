# SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
# SPDX-License-Identifier: MIT
<#
.SYNOPSIS
Measures the Windows front end, to compare a change before and after on the same computer:
its start, showing its window again, what it takes while it waits, its memory, and how fast
output comes through ConPTY to it.

    tests/windows/bench.ps1 [-Program path\to\kudari.exe] [-Runs 5] [-Parts start,idle,loads]
                            [-Loads LOADS] [-IdleSeconds 20] [-Python path\to\python.exe]
                            [-Keep]

Without a path, it takes the release build of cargo, target\release\kudari-windows.exe. Each
run starts Kudari anew with a configuration of its own in a temporary folder.

- start: the milliseconds from the start of the process to its first frame, and from a second
  start, which shows the hidden window, to the frame that shows it, with the time the second
  process took to hand over; the memory with one terminal.
- idle: the CPU time, the context switches of its threads and the frames a second while it
  waits, shown and hidden, with one terminal and with ten, and with a blinking cursor, and
  its memory in each.
- loads: the loads of tools/bench.py, `plain`, `color`, `unicode`, `frames`, `boxes` and
  `seq`, with its data, the same bytes as under Linux. Each counts from its first byte until
  Kudari has read all of it, with the CPU time of Kudari and of the console host, the frames
  a second meanwhile and the seconds the console host took to take the bytes. The console
  host answers queries such as that of the cursor itself, so the end of a load is a copy to
  the clipboard with OSC 52 after it: the loads change the clipboard. They need Python.

The report gives the median of the runs, the fastest and the slowest. A virtual machine draws
in software or on a virtual graphics card, and the console host does much of the work of a
load: compare only runs on the same computer, never with the numbers of tools/bench.sh. It
needs a desktop session, and no other Kudari of the same user may run, as the window and the
pipe for agents belong to one per user.
#>
param(
    [string] $Program,
    [ValidateRange(1, 100)] [int] $Runs = 5,
    [ValidateSet('start', 'idle', 'loads')] [string[]] $Parts = @('start', 'idle', 'loads'),
    [ValidateSet('plain', 'color', 'unicode', 'frames', 'boxes', 'seq')] [string[]] $Loads = @('plain', 'color', 'unicode', 'frames', 'boxes', 'seq'),
    [ValidateRange(1, 600)] [int] $IdleSeconds = 20,
    [string] $Python,
    [switch] $Keep
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Program) {
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root 'target' }
    $Program = Join-Path $target 'release\kudari-windows.exe'
}
if (-not (Test-Path $Program)) {
    Write-Host "No kudari-windows at $Program; build it with: cargo build --release -p kudari-windows"
    exit 2
}
$Program = (Resolve-Path $Program).Path
$name = [IO.Path]::GetFileNameWithoutExtension($Program)
if (Get-Process kudari, kudari-windows -ErrorAction SilentlyContinue) {
    Write-Host 'Kudari runs already; the measurement needs to be the only one.'
    exit 2
}
if ($Parts -contains 'loads') {
    if (-not $Python) {
        $found = Get-Command python, py -CommandType Application -ErrorAction SilentlyContinue |
            Where-Object { $_.Source -notlike '*\WindowsApps\*' } | Select-Object -First 1
        if ($found) { $Python = $found.Source }
    }
    if (-not $Python) {
        Write-Host 'The loads need Python; name it with -Python, or leave them out with -Parts start,idle.'
        exit 2
    }
}

$work = (New-Item -ItemType Directory -Force (Join-Path ([IO.Path]::GetTempPath()) "kudari-bench-$PID")).FullName
$Automation = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]
$Invariant = [Globalization.CultureInfo]::InvariantCulture
$Hosts = 'conhost.exe', 'OpenConsole.exe'

# A folder of its own as APPDATA, with the settings of the measurement and `extra`.
function New-Config([string] $folder, [string] $extra = '') {
    $appdata = Join-Path $work $folder
    New-Item -ItemType Directory -Force (Join-Path $appdata 'Kudari') | Out-Null
    Set-Content (Join-Path $appdata 'Kudari\kudari.ini') "[Behavior]`nOpenAfterStart=true`n[Dialogs]`nFirstRun=false`n$extra"
    $appdata
}

# A text as the value of a setting in kudari.ini, as Qt reads it.
function ConvertTo-IniValue([string] $text) {
    '"' + ($text -replace '\\', '\\' -replace '"', '\"') + '"'
}

# A word of a command, in quotes where it has a space.
function ConvertTo-Word([string] $text) {
    if ($text -match '\s') { "`"$text`"" } else { $text }
}

function Start-Kudari([string] $appdata) {
    $env:APPDATA = $appdata
    $env:QSG_RENDER_TIMING = '1'
    $env:QT_FORCE_STDERR_LOGGING = '1'
    $env:QT_MESSAGE_PATTERN = '%{time yyyy-MM-ddTHH:mm:ss.zzz} %{category} %{message}'
    $env:LANGUAGE = 'en'
    Start-Process $Program -PassThru -WorkingDirectory $appdata -RedirectStandardError (Join-Path $appdata 'log.txt')
}

# A second start, which hands over to the one that runs: how long it took, in milliseconds.
function Send-Toggle {
    $started = Get-Date
    Start-Process $Program -Wait -WorkingDirectory $env:APPDATA
    ((Get-Date) - $started).TotalMilliseconds
}

# The children of a process: its shells and their console hosts.
function Get-Children([int] $parent) {
    @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$parent")
}

function Stop-Kudari($process) {
    $children = Get-Children $process.Id
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    [void] $process.WaitForExit(10000)
    foreach ($child in $children) { Stop-Process -Id $child.ProcessId -Force -ErrorAction SilentlyContinue }
    Remove-Item Env:QSG_RENDER_TIMING, Env:QT_MESSAGE_PATTERN -ErrorAction SilentlyContinue
}

# The times of the frames Qt Quick drew, from the log, which Kudari still writes.
function Get-Frames([string] $appdata) {
    if (-not (Test-Path (Join-Path $appdata 'log.txt'))) { return }
    $stream = [IO.File]::Open((Join-Path $appdata 'log.txt'), 'Open', 'Read', 'ReadWrite')
    try {
        $text = (New-Object IO.StreamReader $stream).ReadToEnd()
    } finally {
        $stream.Dispose()
    }
    foreach ($line in $text -split "`n") {
        if ($line -match '^(\S+) .*frame rendered') {
            [datetime]::ParseExact($Matches[1], 'yyyy-MM-ddTHH:mm:ss.fff', $Invariant)
        }
    }
}

# The first frame after `after`, waiting up to `seconds` for it.
function Wait-Frame([string] $appdata, [datetime] $after, [double] $seconds = 15) {
    $until = (Get-Date).AddSeconds($seconds)
    do {
        $frame = Get-Frames $appdata | Where-Object { $_ -gt $after } | Select-Object -First 1
        if ($frame) { return $frame }
        Start-Sleep -Milliseconds 50
    } while ((Get-Date) -lt $until)
    $null
}

function Get-Memory($process) {
    $process.Refresh()
    [pscustomobject] @{ Private = $process.PrivateMemorySize64 / 1MB; Resident = $process.WorkingSet64 / 1MB }
}

function Get-Median([double[]] $values) {
    $sorted = @($values | Sort-Object)
    if ($sorted.Count -eq 0) { return [double]::NaN }
    $middle = [int][math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2) { $sorted[$middle] } else { ($sorted[$middle - 1] + $sorted[$middle]) / 2 }
}

# Median, fastest and slowest, as `format` gives a number.
function Format-Spread([double[]] $values, [string] $format = '{0:N0}') {
    if (@($values).Count -eq 0) { return '-' }
    $low = ($values | Measure-Object -Minimum).Minimum
    $high = ($values | Measure-Object -Maximum).Maximum
    ($format -f (Get-Median $values)) + ' (' + ($format -f $low) + '-' + ($format -f $high) + ')'
}

function Write-Table([string[]] $header, [object[]] $rows) {
    $cells = @(, $header) + @($rows | ForEach-Object { , [string[]] $_ })
    $widths = foreach ($column in 0..($header.Count - 1)) { ($cells | ForEach-Object { $_[$column].Length } | Measure-Object -Maximum).Maximum }
    $lines = foreach ($row in $cells) {
        (0..($header.Count - 1) | ForEach-Object { if ($_ -eq 0) { $row[$_].PadRight($widths[$_]) } else { $row[$_].PadLeft($widths[$_]) } }) -join '  '
    }
    $lines[0]
    ($widths | ForEach-Object { '-' * $_ }) -join '  '
    $lines | Select-Object -Skip 1
}

# Clicks the button with the name in the window of the process.
function Invoke-Button([int] $id, [string] $label) {
    $byProcess = New-Object System.Windows.Automation.PropertyCondition ($Automation::ProcessIdProperty, $id)
    $byName = New-Object System.Windows.Automation.PropertyCondition ($Automation::NameProperty, $label)
    foreach ($window in $Automation::RootElement.FindAll($Scope::Children, $byProcess)) {
        foreach ($button in $window.FindAll($Scope::Descendants, $byName)) {
            $pattern = $null
            if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref] $pattern)) {
                $pattern.Invoke()
                return $true
            }
        }
    }
    $false
}

function Get-TerminalCount([int] $id) {
    @(Get-Children $id | Where-Object { $Hosts -contains $_.Name }).Count
}

# What Kudari takes while it waits, for `IdleSeconds`: CPU, context switches of its threads,
# frames and memory.
function Measure-Idle($process, [string] $appdata, [string] $state) {
    Start-Sleep -Seconds 3
    $process.Refresh()
    $cpu = $process.TotalProcessorTime.TotalMilliseconds
    $from = Get-Date
    $switches = $null
    # A rate over the whole time: the counter takes its first value at the start. A thread
    # that ended meanwhile has a sample that is not valid, and makes it an error.
    $sample = Get-Counter "\Thread($name/*)\Context Switches/sec" -SampleInterval $IdleSeconds -MaxSamples 1 -ErrorAction SilentlyContinue
    if ($sample) {
        $switches = ($sample.CounterSamples | Where-Object Status -EQ 0 | Measure-Object CookedValue -Sum).Sum
    } else {
        # Counters with names in another language, or none at all.
        $left = $IdleSeconds - ((Get-Date) - $from).TotalSeconds
        if ($left -gt 0) { Start-Sleep -Milliseconds ([int] ($left * 1000)) }
    }
    $to = Get-Date
    $process.Refresh()
    $seconds = ($to - $from).TotalSeconds
    $frames = @(Get-Frames $appdata | Where-Object { $_ -gt $from -and $_ -le $to }).Count
    $memory = Get-Memory $process
    , @($state, ('{0:N2}' -f (($process.TotalProcessorTime.TotalMilliseconds - $cpu) / $seconds)),
        $(if ($null -eq $switches) { '-' } else { '{0:N0}' -f $switches }), ('{0:N1}' -f ($frames / $seconds)),
        ('{0:N0}' -f $memory.Private), ('{0:N0}' -f $memory.Resident))
}

function Get-Description {
    $printed = Join-Path $work 'version.txt'
    Start-Process $Program -ArgumentList '--version' -Wait -NoNewWindow -RedirectStandardOutput $printed
    $version = (Get-Content $printed -ErrorAction SilentlyContinue | Select-Object -First 1)
    $hash = (Get-FileHash $Program -Algorithm SHA256).Hash.Substring(0, 16).ToLower()
    $qt = Get-Command Qt6Core.dll -ErrorAction SilentlyContinue | Select-Object -First 1
    $beside = Join-Path (Split-Path $Program) 'Qt6Core.dll'
    $core = if (Test-Path $beside) { $beside } elseif ($qt) { $qt.Source } else { $null }
    $processor = Get-CimInstance Win32_Processor | Select-Object -First 1
    $system = Get-CimInstance Win32_OperatingSystem
    $video = Get-CimInstance Win32_VideoController | Select-Object -First 1
    "Kudari    $Program, $version, sha256 $hash"
    "Qt        $(if ($core) { (Get-Item $core).VersionInfo.ProductVersion + ' from ' + $core } else { 'not found' })"
    "Computer  $($processor.Name.Trim()), $($processor.NumberOfLogicalProcessors) threads; $($system.Caption) $($system.Version)"
    "Screen    $($video.Name), $($video.CurrentHorizontalResolution)x$($video.CurrentVerticalResolution) at $($video.CurrentRefreshRate) Hz"
    "Runs      $Runs"
}

$report = New-Object Collections.Generic.List[string]
$failed = $false
try {
    if ($Parts -contains 'start') {
        $starts, $shows, $handovers, $private, $resident = @(), @(), @(), @(), @()
        foreach ($run in 1..$Runs) {
            Write-Host "start, run $run of $Runs"
            $appdata = New-Config "start-$run"
            $process = Start-Kudari $appdata
            try {
                $first = Wait-Frame $appdata $process.StartTime
                if (-not $first) { throw 'Kudari drew no frame within 15 s.' }
                $starts += ($first - $process.StartTime).TotalMilliseconds
                Start-Sleep -Seconds 2
                $memory = Get-Memory $process
                $private += $memory.Private
                $resident += $memory.Resident
                # Hides the window, then shows it again.
                [void] (Send-Toggle)
                Start-Sleep -Milliseconds 1500
                $asked = Get-Date
                $handovers += Send-Toggle
                $shown = Wait-Frame $appdata $asked
                if ($shown) { $shows += ($shown - $asked).TotalMilliseconds }
            } finally {
                Stop-Kudari $process
            }
        }
        $report.Add('')
        $report.Add('Start in milliseconds: median of the runs, in brackets the fastest and the slowest')
        $report.AddRange([string[]] (Write-Table @('what', 'ms') @(
            , @('start to the first frame', (Format-Spread $starts))
            , @('second start to the frame that shows the window', (Format-Spread $shows))
            , @('the second start itself, until it ended', (Format-Spread $handovers)))))
        $report.Add("Memory with one terminal, MB: private $(Format-Spread $private), resident $(Format-Spread $resident)")
    }

    if ($Parts -contains 'idle') {
        Write-Host "idle, $IdleSeconds s for each state"
        $rows = @()
        $appdata = New-Config 'idle'
        $process = Start-Kudari $appdata
        try {
            if (-not (Wait-Frame $appdata $process.StartTime)) { throw 'Kudari drew no frame within 15 s.' }
            $rows += , (Measure-Idle $process $appdata 'one terminal, shown')
            [void] (Send-Toggle)
            $rows += , (Measure-Idle $process $appdata 'one terminal, hidden')
            [void] (Send-Toggle)
            Start-Sleep -Seconds 1
            foreach ($tab in 2..10) {
                if (-not (Invoke-Button $process.Id 'New Session')) { throw 'There was no button New Session.' }
                Start-Sleep -Milliseconds 700
            }
            Start-Sleep -Seconds 2
            $count = Get-TerminalCount $process.Id
            if ($count -ne 10) { throw "Kudari has $count terminals instead of 10." }
            $rows += , (Measure-Idle $process $appdata 'ten terminals, shown')
            [void] (Send-Toggle)
            $rows += , (Measure-Idle $process $appdata 'ten terminals, hidden')
        } finally {
            Stop-Kudari $process
        }
        $appdata = New-Config 'blinking' "[Terminal]`nBlinkingCursor=true"
        $process = Start-Kudari $appdata
        try {
            if (-not (Wait-Frame $appdata $process.StartTime)) { throw 'Kudari drew no frame within 15 s.' }
            $rows += , (Measure-Idle $process $appdata 'one terminal, shown, blinking cursor')
        } finally {
            Stop-Kudari $process
        }
        $report.Add('')
        $report.Add("While Kudari waits, over $IdleSeconds s each: CPU in ms a second, which Windows counts in steps")
        $report.Add("of about 16 ms, so that less than $('{0:N2}' -f (15.625 / $IdleSeconds)) shows as 0; context switches of its threads a")
        $report.Add('second ("-" without the counters in English); frames a second; memory in MB')
        $report.AddRange([string[]] (Write-Table @('state', 'CPU ms/s', 'switches/s', 'frames/s', 'private', 'resident') $rows))
    }

    if ($Parts -contains 'loads') {
        $data = (New-Item -ItemType Directory -Force (Join-Path $work 'data')).FullName
        Write-Host 'making the data'
        & $Python -X utf8 (Join-Path $root 'tools\bench.py') data $data
        if ($LASTEXITCODE) { throw "tools/bench.py data failed with $LASTEXITCODE." }
        $loader = Join-Path $PSScriptRoot 'bench-load.py'
        $results = @{}
        $size = $null
        foreach ($run in 1..$Runs) {
            Write-Host "loads, run $run of $Runs"
            $folder = "loads-$run"
            $runDirectory = Join-Path $work "$folder\run"
            New-Item -ItemType Directory -Force $runDirectory | Out-Null
            $command = (@($Python, '-X', 'utf8', $loader, $data, $runDirectory) + $Loads | ForEach-Object { ConvertTo-Word $_ }) -join ' '
            $appdata = New-Config $folder "[Terminal]`nCommand=$(ConvertTo-IniValue $command)"
            $process = Start-Kudari $appdata
            try {
                $until = (Get-Date).AddSeconds(180)
                while (-not (Test-Path (Join-Path $runDirectory 'ready'))) {
                    if ((Get-Date) -gt $until -or $process.HasExited) { throw 'The loads did not start; see the log in ' + $appdata }
                    Start-Sleep -Milliseconds 200
                }
                New-Item (Join-Path $runDirectory 'go') | Out-Null
                $out = Join-Path $runDirectory 'out.txt'
                $until = (Get-Date).AddSeconds(150 * $Loads.Count)
                while (-not ((Test-Path $out) -and (Select-String -Quiet -SimpleMatch 'done' $out))) {
                    if ((Get-Date) -gt $until -or $process.HasExited) { throw 'The loads did not end; see ' + $out }
                    Start-Sleep -Milliseconds 500
                }
                $frames = @(Get-Frames $appdata)
                foreach ($line in Get-Content $out) {
                    $words = $line -split ' '
                    if ($words[0] -eq 'size') { $size = "$($words[1])x$($words[2])"; continue }
                    if ($Loads -notcontains $words[0]) { continue }
                    $from = [DateTimeOffset]::FromUnixTimeMilliseconds([long]([double]$words[7] * 1000)).LocalDateTime
                    $to = [DateTimeOffset]::FromUnixTimeMilliseconds([long]([double]$words[8] * 1000)).LocalDateTime
                    $seconds = [double]::Parse($words[2], $Invariant)
                    $drawn = @($frames | Where-Object { $_ -gt $from -and $_ -le $to }).Count
                    if (-not $results.ContainsKey($words[0])) { $results[$words[0]] = @() }
                    $results[$words[0]] += [pscustomobject] @{
                        Bytes = [double] $words[1]; Seconds = $seconds; Written = [double]::Parse($words[3], $Invariant)
                        Answered = $words[4] -eq 'answered'; Kudari = [double]::Parse($words[5], $Invariant)
                        Host = [double]::Parse($words[6], $Invariant); Fps = $(if ($seconds -gt 0) { $drawn / $seconds } else { 0 })
                    }
                }
            } finally {
                Stop-Kudari $process
            }
        }
        $rows = @(foreach ($load in $Loads) {
            $runsOf = @(if ($results.ContainsKey($load)) { $results[$load] })
            if ($runsOf.Count -eq 0) { , @($load, '-', '-', '-', '-', '-', '-', '-', '-'); continue }
            $megabytes = $runsOf[0].Bytes / 1MB
            $median = Get-Median ($runsOf | ForEach-Object Seconds)
            , @($load, ('{0:N1}' -f $megabytes), (Format-Spread ($runsOf | ForEach-Object Seconds) '{0:N2}'),
                ('{0:N1}' -f ($megabytes / $median)), ('{0:N2}' -f (Get-Median ($runsOf | ForEach-Object Written))),
                ('{0:N0}' -f (Get-Median ($runsOf | ForEach-Object Kudari))), ('{0:N0}' -f (Get-Median ($runsOf | ForEach-Object Host))),
                ('{0:N1}' -f (Get-Median ($runsOf | ForEach-Object Fps))), [string] @($runsOf | Where-Object { -not $_.Answered }).Count)
        })
        $report.Add('')
        $report.Add("Loads in a terminal of $size cells: the seconds from the first byte until Kudari read the")
        $report.Add('last (median, the fastest and the slowest), the throughput of the median, the seconds the')
        $report.Add('console host took to take the bytes, the CPU time of Kudari and of the console host in ms,')
        $report.Add('the frames a second meanwhile, and the runs Kudari did not end within 120 s')
        $report.AddRange([string[]] (Write-Table @('load', 'MB', 's', 'MB/s', 'host s', 'Kudari ms', 'host ms', 'frames/s', 'unended') $rows))
    }
} catch {
    Write-Host "bench: $($_.Exception.Message)"
    $failed = $true
} finally {
    $description = Get-Description
    if ($Keep -or $failed) {
        Write-Host "The data and the logs are in $work"
    } else {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
}
$description | Write-Host
$report | Write-Host
if ($failed) { exit 1 }
