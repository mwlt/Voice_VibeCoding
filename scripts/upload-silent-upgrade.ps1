param(
  [string]$Version = "1.6.2",
  [string]$Tag = "v1.6.2"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$ghAsset = Join-Path $root "dist\Voice.VibeCoding_${Version}_SilentUpgrade.exe"
$giteeAsset = Join-Path $root "dist\Voice VibeCoding_${Version}_SilentUpgrade.exe"
if (-not (Test-Path $ghAsset)) { throw "Missing $ghAsset" }
if (-not (Test-Path $giteeAsset)) {
  Copy-Item $ghAsset $giteeAsset -Force
}

if (Test-Path "$env:USERPROFILE\.gitee_token") {
  $env:GITEE_TOKEN = (Get-Content -Raw "$env:USERPROFILE\.gitee_token").Trim()
}
if (-not $env:GITHUB_TOKEN) {
  $t = & gh auth token 2>$null
  if ($t) { $env:GITHUB_TOKEN = $t.Trim() }
}
if (-not $env:GITEE_TOKEN) { throw "GITEE_TOKEN missing" }
if (-not $env:GITHUB_TOKEN) { throw "GITHUB_TOKEN missing" }

# Gitee
$api = "https://gitee.com/api/v5/repos/mwlt/remote-voice-vibe-coding/releases"
$tokenEsc = [uri]::EscapeDataString($env:GITEE_TOKEN)
$rel = Invoke-RestMethod -Uri "$api/tags/${Tag}?access_token=$tokenEsc" -Method Get
Write-Host "Gitee release id: $($rel.id)"
$resolved = (Resolve-Path -LiteralPath $giteeAsset).Path
curl.exe -sf -X POST "https://gitee.com/api/v5/repos/mwlt/remote-voice-vibe-coding/releases/$($rel.id)/attach_files" `
  -F "access_token=$($env:GITEE_TOKEN)" -F "file=@$resolved"
Write-Host "Gitee SilentUpgrade uploaded"

# GitHub
$headers = @{
  Authorization = "Bearer $($env:GITHUB_TOKEN)"
  Accept = "application/vnd.github+json"
  "X-GitHub-Api-Version" = "2022-11-28"
}
$gh = Invoke-RestMethod -Uri "https://api.github.com/repos/mwlt/Voice_VibeCoding/releases/tags/$Tag" -Headers $headers
Write-Host "GitHub release id: $($gh.id)"
foreach ($a in @($gh.assets)) {
  if ($a.name -eq "Voice.VibeCoding_${Version}_SilentUpgrade.exe") {
    Invoke-RestMethod -Uri "https://api.github.com/repos/mwlt/Voice_VibeCoding/releases/assets/$($a.id)" -Headers $headers -Method Delete
    Write-Host "deleted old SilentUpgrade asset"
  }
}
$uploadHeaders = @{
  Authorization = "Bearer $($env:GITHUB_TOKEN)"
  Accept = "application/vnd.github+json"
  "Content-Type" = "application/octet-stream"
  "X-GitHub-Api-Version" = "2022-11-28"
}
$nameEsc = [uri]::EscapeDataString("Voice.VibeCoding_${Version}_SilentUpgrade.exe")
$uploadUrl = ($gh.upload_url -replace '\{\?name,label\}', "?name=$nameEsc")
$up = Invoke-RestMethod -Uri $uploadUrl -Headers $uploadHeaders -Method Post -InFile $ghAsset
Write-Host "GitHub uploaded: $($up.browser_download_url) size=$($up.size)"
Write-Host "Done."
