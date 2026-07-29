#Requires -Version 5.1
<#
.SYNOPSIS
  Compare audiodg / remote-bridge-hub handle counts before and after quitting Voice VibeCoding.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\compare_audiodg_handles.ps1
  powershell -ExecutionPolicy Bypass -File scripts\compare_audiodg_handles.ps1 -Samples 10 -IntervalSec 3
#>
param(
    [int]$Samples = 6,
    [int]$IntervalSec = 2
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Continue"

function Get-ProcSnapshot {
    param([string[]]$Names)
    $rows = @()
    foreach ($name in $Names) {
        $procs = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
        if ($procs.Count -eq 0) {
            $rows += [pscustomobject]@{
                Time    = Get-Date
                Name    = $name
                PID     = $null
                Handles = $null
                WS_MB   = $null
                Priv_MB = $null
                CmdHint = "(not running)"
            }
            continue
        }
        foreach ($p in $procs) {
            $cmd = ""
            try {
                $cim = Get-CimInstance Win32_Process -Filter "ProcessId=$($p.Id)" -ErrorAction SilentlyContinue
                if ($cim -and $cim.CommandLine) {
                    $cmd = $cim.CommandLine
                    if ($cmd.Length -gt 90) { $cmd = $cmd.Substring(0, 90) + "..." }
                }
            } catch {}
            $rows += [pscustomobject]@{
                Time    = Get-Date
                Name    = $p.Name
                PID     = $p.Id
                Handles = $p.HandleCount
                WS_MB   = [math]::Round($p.WorkingSet64 / 1MB, 1)
                Priv_MB = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
                CmdHint = $cmd
            }
        }
    }
    return $rows
}

function Show-SnapshotTable {
    param($Rows, [string]$Title)
    Write-Host ""
    Write-Host "=== $Title ===" -ForegroundColor Cyan
    $Rows | Format-Table Time, Name, PID, Handles, WS_MB, Priv_MB, CmdHint -AutoSize
}

function Measure-AudiodgGrowth {
    param($Series)
    $audio = @($Series | Where-Object { $_.Name -eq "audiodg" -and $null -ne $_.Handles })
    if ($audio.Count -lt 2) {
        return [pscustomobject]@{
            Samples = $audio.Count
            First   = $null
            Last    = $null
            Delta   = $null
            Seconds = $null
            PerSec  = $null
            Note    = "not enough audiodg samples"
        }
    }
    $first = $audio[0]
    $last = $audio[-1]
    $sec = ($last.Time - $first.Time).TotalSeconds
    $delta = $last.Handles - $first.Handles
    $perSec = if ($sec -gt 0) { [math]::Round($delta / $sec, 1) } else { $null }
    $note = if ($perSec -gt 100) {
        "VERY HIGH growth"
    } elseif ($perSec -gt 10) {
        "high growth"
    } elseif ($perSec -gt 0) {
        "slow growth"
    } elseif ($perSec -eq 0) {
        "flat"
    } else {
        "falling"
    }
    return [pscustomobject]@{
        Samples = $audio.Count
        First   = $first.Handles
        Last    = $last.Handles
        Delta   = $delta
        Seconds = [math]::Round($sec, 1)
        PerSec  = $perSec
        Note    = $note
    }
}

function Sample-Phase {
    param(
        [string]$PhaseName,
        [int]$Count,
        [int]$Interval
    )
    $all = @()
    Write-Host ""
    Write-Host (">>> {0}: {1} samples, interval {2}s" -f $PhaseName, $Count, $Interval) -ForegroundColor Yellow
    for ($i = 1; $i -le $Count; $i++) {
        $snap = Get-ProcSnapshot -Names @("audiodg", "remote-bridge-hub")
        $all += $snap
        $audio = $snap | Where-Object { $_.Name -eq "audiodg" } | Select-Object -First 1
        $apps = @($snap | Where-Object { $_.Name -eq "remote-bridge-hub" -and $null -ne $_.PID })
        $appHint = if ($apps.Count -gt 0) {
            "app x$($apps.Count) h=$($apps[0].Handles)"
        } else {
            "app=QUIT"
        }
        $ah = if ($audio -and $null -ne $audio.Handles) { $audio.Handles } else { "n/a" }
        Write-Host ("  [{0}/{1}] audiodg handles={2}  {3}" -f $i, $Count, $ah, $appHint)
        if ($i -lt $Count) { Start-Sleep -Seconds $Interval }
    }
    return $all
}

Write-Host ""
Write-Host "Voice VibeCoding / audiodg handle compare"
Write-Host "-----------------------------------------"
Write-Host "This script does NOT kill processes."
Write-Host "Quit Voice VibeCoding (including tray) when prompted."
Write-Host ""
Write-Host "Rough normal ranges:"
Write-Host "  remote-bridge-hub main     hundreds to ~2k"
Write-Host "  audio-router child         ~100-300"
Write-Host "  audiodg                    hundreds to a few thousand; millions = leak"
Write-Host ""

$running = @(Get-Process -Name "remote-bridge-hub" -ErrorAction SilentlyContinue)
if ($running.Count -eq 0) {
    Write-Host "remote-bridge-hub not found. Start Voice VibeCoding first, then rerun." -ForegroundColor Red
    exit 1
}

Write-Host ("Found remote-bridge-hub x{0}. Starting Phase A (app RUNNING)." -f $running.Count) -ForegroundColor Green

$phaseA = Sample-Phase -PhaseName "Phase A (app RUNNING)" -Count $Samples -Interval $IntervalSec
Show-SnapshotTable -Rows ($phaseA | Select-Object -Last 12) -Title "Phase A last snapshots"
$growA = Measure-AudiodgGrowth -Series $phaseA

Write-Host ""
Write-Host ("Phase A audiodg: first={0} last={1} delta={2} over {3}s  ~{4}/s  [{5}]" -f `
    $growA.First, $growA.Last, $growA.Delta, $growA.Seconds, $growA.PerSec, $growA.Note) -ForegroundColor Magenta

Write-Host ""
Write-Host "Now FULLY quit Voice VibeCoding (tray too)." -ForegroundColor Yellow
Write-Host "Press Enter when quit is done."
[void][System.Console]::ReadLine()

$deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline) {
    $left = @(Get-Process -Name "remote-bridge-hub" -ErrorAction SilentlyContinue)
    if ($left.Count -eq 0) {
        Write-Host "Confirmed: remote-bridge-hub is gone." -ForegroundColor Green
        break
    }
    Write-Host ("Still seeing remote-bridge-hub x{0}; waiting 2s (quit tray again if needed)..." -f $left.Count)
    Start-Sleep -Seconds 2
}
$still = @(Get-Process -Name "remote-bridge-hub" -ErrorAction SilentlyContinue)
if ($still.Count -gt 0) {
    Write-Host "WARN: app still running; Phase B may be inaccurate." -ForegroundColor Red
}

Start-Sleep -Seconds 2
$phaseB = Sample-Phase -PhaseName "Phase B (app QUIT)" -Count $Samples -Interval $IntervalSec
Show-SnapshotTable -Rows ($phaseB | Select-Object -Last 12) -Title "Phase B last snapshots"
$growB = Measure-AudiodgGrowth -Series $phaseB

Write-Host ""
Write-Host "======== RESULT ========" -ForegroundColor Cyan
Write-Host ("Phase A (RUNNING): {0} -> {1}  delta={2}  ~{3}/s  [{4}]" -f $growA.First, $growA.Last, $growA.Delta, $growA.PerSec, $growA.Note)
Write-Host ("Phase B (QUIT):    {0} -> {1}  delta={2}  ~{3}/s  [{4}]" -f $growB.First, $growB.Last, $growB.Delta, $growB.PerSec, $growB.Note)

$verdict = "inconclusive"
if ($null -ne $growA.PerSec -and $null -ne $growB.PerSec) {
    if ($growA.PerSec -gt 50 -and $growB.PerSec -lt ($growA.PerSec * 0.3)) {
        $verdict = "LIKELY RELATED: growth dropped a lot after quit (check PCM->VB-CABLE path)"
    } elseif ($growA.PerSec -gt 50 -and $growB.PerSec -gt ($growA.PerSec * 0.7)) {
        $verdict = "LIKELY UNRELATED: still growing fast after quit (system audio / driver / APO)"
    } elseif ($growA.PerSec -le 10 -and $growB.PerSec -le 10) {
        $verdict = "Growth low in this window; huge stockpile may be older leak. Restart Windows Audio / kill audiodg and retest."
    } else {
        $verdict = "Mild change; rerun with -Samples 10 -IntervalSec 3"
    }
}

Write-Host ""
Write-Host ("Verdict: {0}" -f $verdict) -ForegroundColor Yellow
Write-Host ""
Write-Host "Note: audiodg.exe is NOT this project's process."
Write-Host "This app only uses WASAPI -> VB-CABLE via the Windows audio stack."
Write-Host ""

$outDir = Join-Path $PSScriptRoot "..\logs"
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }
$stamp = Get-Date -Format "yyyyMMdd_HHmmss"
$csv = Join-Path $outDir ("audiodg_handle_compare_{0}.csv" -f $stamp)
($phaseA + $phaseB) | Export-Csv -Path $csv -NoTypeInformation -Encoding UTF8
Write-Host ("CSV saved: {0}" -f $csv)
