#Requires -RunAsAdministrator
<#
.SYNOPSIS
  Disable USB selective suspend / enhanced power management for Google T1
  (VID_1915&PID_1025) and USB Root Hubs. Use when Mic Device goes silent ~15s
  even in Windows Voice Recorder.
#>
$ErrorActionPreference = "Stop"

Write-Host "Disabling USB selective suspend in current power plan..."
powercfg /SETACVALUEINDEX SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 0 | Out-Null
powercfg /SETDCVALUEINDEX SCHEME_CURRENT 2a737441-1930-4402-8d77-b2bebba308a3 48e6b7a6-50f5-4782-a5d4-53bb8f07e226 0 | Out-Null
powercfg /SETACTIVE SCHEME_CURRENT | Out-Null

function Set-UsbPowerOff([string]$DeviceParametersPath) {
    if (-not (Test-Path $DeviceParametersPath)) { return $false }
    New-ItemProperty -Path $DeviceParametersPath -Name "EnhancedPowerManagementEnabled" -PropertyType DWord -Value 0 -Force | Out-Null
    New-ItemProperty -Path $DeviceParametersPath -Name "SelectiveSuspendEnabled" -PropertyType DWord -Value 0 -Force | Out-Null
    return $true
}

$n = 0
Get-ChildItem "HKLM:\SYSTEM\CurrentControlSet\Enum\USB" |
    Where-Object { $_.PSChildName -like "VID_1915*" } |
    ForEach-Object {
        Get-ChildItem $_.PSPath | ForEach-Object {
            $dp = Join-Path $_.PSPath "Device Parameters"
            if (Set-UsbPowerOff $dp) {
                Write-Host "T1 OK: $dp"
                $n++
            }
        }
    }

Get-ChildItem "HKLM:\SYSTEM\CurrentControlSet\Enum\USB" |
    Where-Object { $_.PSChildName -like "ROOT_HUB*" } |
    ForEach-Object {
        Get-ChildItem $_.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
            $dp = Join-Path $_.PSPath "Device Parameters"
            if (Set-UsbPowerOff $dp) {
                Write-Host "HUB OK: $dp"
                $n++
            }
        }
    }

Write-Host "Updated $n Device Parameters keys."
Write-Host "Please unplug/replug the T1 receiver (or reboot), then retest Voice Recorder >20s."
