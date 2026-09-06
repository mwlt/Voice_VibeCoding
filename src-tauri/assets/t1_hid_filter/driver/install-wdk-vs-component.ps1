$ErrorActionPreference = "Continue"
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$inst = & $vswhere -latest -products * -property installationPath
Write-Host "VS=$inst"
$setup = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\setup.exe"

$args = @(
  "modify",
  "--installPath", $inst,
  "--add", "Microsoft.VisualStudio.Component.WindowsDriverKit",
  "--quiet",
  "--norestart",
  "--force"
)
Write-Host "Running setup $($args -join ' ')"
$p = Start-Process -FilePath $setup -ArgumentList $args -Wait -PassThru
Write-Host "ExitCode=$($p.ExitCode)"

foreach ($v in @("v180","v170")) {
  $ts = Join-Path $inst "MSBuild\Microsoft\VC\$v\Platforms\x64\PlatformToolsets"
  Write-Host "toolsets $ts"
  if (Test-Path $ts) { Get-ChildItem $ts | ForEach-Object { $_.Name } }
}
