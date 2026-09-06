$ErrorActionPreference = "Stop"
$NugetC = (Resolve-Path (Join-Path $PSScriptRoot "packages\Microsoft.Windows.WDK.x64.10.0.26100.6584\c")).Path
if (-not $NugetC.EndsWith("\")) { $NugetC += "\" }

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$inst = & $vswhere -latest -products * -property installationPath
$vcPlatforms = Join-Path $inst "MSBuild\Microsoft\VC\v180\Platforms\x64"
$toolsetRoot = Join-Path $vcPlatforms "PlatformToolsets\WindowsKernelModeDriver10.0"
$importAfter = Join-Path $vcPlatforms "ImportAfter"
New-Item -ItemType Directory -Force -Path $toolsetRoot | Out-Null
New-Item -ItemType Directory -Force -Path $importAfter | Out-Null

$props = @"
<?xml version="1.0" encoding="utf-8"?>
<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <PropertyGroup>
    <WDKContentRoot>$NugetC</WDKContentRoot>
    <WDKBuildFolder>10.0.26100.0</WDKBuildFolder>
    <IsKernelModeToolset>true</IsKernelModeToolset>
    <IsUserModeToolset>false</IsUserModeToolset>
    <DDKPlatform Condition="'`$(Platform)' == 'x64'">x64</DDKPlatform>
    <WindowsTargetPlatformVersion Condition="'`$(WindowsTargetPlatformVersion)' == ''">10.0.26100.0</WindowsTargetPlatformVersion>
    <TargetPlatformVersion Condition="'`$(TargetPlatformVersion)' == ''">`$(WindowsTargetPlatformVersion)</TargetPlatformVersion>
    <Driver_SpectreMitigation Condition="'`$(Driver_SpectreMitigation)' == ''">false</Driver_SpectreMitigation>
    <SpectreMitigation Condition="'`$(SpectreMitigation)' == ''">false</SpectreMitigation>
  </PropertyGroup>
  <Import Project="`$(WDKContentRoot)build\`$(WDKBuildFolder)\WindowsDriver.Default.props" Condition="Exists('`$(WDKContentRoot)build\`$(WDKBuildFolder)\WindowsDriver.Default.props')" />
  <Import Project="`$(WDKContentRoot)DesignTime\CommonConfiguration\Neutral\WDK\`$(WindowsTargetPlatformVersion)\WDK.props" Condition="Exists('`$(WDKContentRoot)DesignTime\CommonConfiguration\Neutral\WDK\`$(WindowsTargetPlatformVersion)\WDK.props')" />
  <Import Project="`$(WDKContentRoot)build\`$(WDKBuildFolder)\`$(Platform)\WindowsKernelModeDriver\WDK.`$(Platform).WindowsKernelModeDriver.props" Condition="Exists('`$(WDKContentRoot)build\`$(WDKBuildFolder)\`$(Platform)\WindowsKernelModeDriver\WDK.`$(Platform).WindowsKernelModeDriver.props')" />
  <Import Project="`$(VCTargetsPath)\Platforms\`$(Platform)\PlatformToolsets\v145\Toolset.props" />
</Project>
"@
$targets = @"
<?xml version="1.0" encoding="utf-8"?>
<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <Import Project="`$(VCTargetsPath)\Platforms\`$(Platform)\PlatformToolsets\v145\Toolset.targets" />
  <Import Project="`$(WDKContentRoot)build\`$(WDKBuildFolder)\WindowsDriver.Common.targets" Condition="Exists('`$(WDKContentRoot)build\`$(WDKBuildFolder)\WindowsDriver.Common.targets')" />
</Project>
"@
$bridge = @"
<?xml version="1.0" encoding="utf-8"?>
<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <Import Condition="'`$(IsKernelModeToolset)'=='true' and Exists('`$(WDKContentRoot)build\`$(WDKBuildFolder)\x64\ImportAfter\WDK.x64.WindowsKernelModeDriver.Platform.props')"
          Project="`$(WDKContentRoot)build\`$(WDKBuildFolder)\x64\ImportAfter\WDK.x64.WindowsKernelModeDriver.Platform.props" />
</Project>
"@

$props = $props.Replace('`$(', '$(')
$targets = $targets.Replace('`$(', '$(')
$bridge = $bridge.Replace('`$(', '$(')

[System.IO.File]::WriteAllText((Join-Path $toolsetRoot "Toolset.props"), $props)
[System.IO.File]::WriteAllText((Join-Path $toolsetRoot "Toolset.targets"), $targets)
[System.IO.File]::WriteAllText((Join-Path $importAfter "Microsoft.Cpp.x64.WindowsKernelModeDriver.props"), $bridge)

$nugetBin = Join-Path $NugetC "build\10.0.26100.0\bin"
$taskDll = Join-Path $nugetBin "Microsoft.DriverKit.Build.Tasks.18.0.dll"
if (-not (Test-Path $taskDll)) {
  $sysDll = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter "Microsoft.DriverKit.Build.Tasks.18.0.dll" -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty FullName
  if ($sysDll) {
    New-Item -ItemType Directory -Force -Path $nugetBin | Out-Null
    Copy-Item $sysDll $taskDll -Force
    Write-Host "Copied DriverKit tasks from $sysDll"
  }
}

Write-Host "Updated KMDF toolset: $toolsetRoot"
Write-Host "WDKContentRoot=$NugetC"
