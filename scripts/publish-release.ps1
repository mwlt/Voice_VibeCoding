# Publish installers + WinUHid manual zip to Gitee + GitHub Releases.
# Usage:
#   $env:GITEE_TOKEN = 'your_gitee_personal_access_token'
#   $env:GITHUB_TOKEN = 'your_github_pat_with_repo_scope'
#   .\scripts\pack-winuhid-release.ps1 -Version 1.6.1
#   .\scripts\publish-release.ps1 -Version 1.6.1 -Tag v1.6.1

param(
    [string]$Version = "1.6.1",
    [string]$Tag = "v1.6.1"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$nsis = Join-Path $root "src-tauri\target\release\bundle\nsis\Voice VibeCoding_${Version}_x64-setup.exe"
$msi = Join-Path $root "src-tauri\target\release\bundle\msi\Voice VibeCoding_${Version}_x64_zh-CN.msi"
$winuhidZip = Join-Path $root "dist\WinUHid_Manual_$Version.zip"
$ghNsis = Join-Path $env:TEMP "Voice.VibeCoding_${Version}_x64-setup.exe"
$ghMsi = Join-Path $env:TEMP "Voice.VibeCoding_${Version}_x64_zh-CN.msi"

if (-not (Test-Path $nsis)) { throw "Missing NSIS build: $nsis" }
if (-not (Test-Path $msi)) { throw "Missing MSI build: $msi" }
if (-not (Test-Path $winuhidZip)) { throw "Missing WinUHid zip: $winuhidZip (run scripts/pack-winuhid-release.ps1)" }
Copy-Item $nsis $ghNsis -Force
Copy-Item $msi $ghMsi -Force

$body = @"
## v$Version

1. **微信输入法**：务必按应用内「输入法设置」说明配置（推荐 **F5 + 本软件快捷键**）
2. **唤醒语音优化**：首包 / 唤醒路径延迟改进
3. **F5 键策略修改**：仅语音按住相关窗口吞 F5，不误伤真键盘
4. **忽略此版本优化**：「不再提醒此版本」仅抑制自动提醒，检查更新仍可用
5. **界面优化**：遥控器示意、输入法引导、按键映射与主机操作区等
6. **再次点击遥控器按键取消选定**：键位映射页可取消选中 / 取消录入
7. **真键盘 Home / 音量± / Menu 被挡修正**：LL 仅 recent 吞固件残留，不再因 Tap 就绪整键吞掉
8. **初始化黄黑图标**：未就绪时任务栏/托盘黄黑图标；语音就绪后恢复蓝标

详见 README。
"@

function Ensure-GiteeRelease {
    if (-not $env:GITEE_TOKEN) { throw "GITEE_TOKEN is required" }
    $api = "https://gitee.com/api/v5/repos/mwlt/remote-voice-vibe-coding/releases"
    $token = [uri]::EscapeDataString($env:GITEE_TOKEN)
    try {
        $existing = Invoke-RestMethod -Uri "$api/tags/$Tag?access_token=$token" -Method Get
        if ($existing -and $existing.id) { return $existing.id }
    } catch {
        # 404 when release tag does not exist yet
    }
    $created = Invoke-RestMethod -Uri $api -Method Post -Body @{
        access_token = $env:GITEE_TOKEN
        tag_name = $Tag
        name = $Tag
        body = $body
        target_commitish = "main"
        prerelease = "false"
    }
    return $created.id
}

function Upload-GiteeAsset($releaseId, $filePath) {
    $uri = "https://gitee.com/api/v5/repos/mwlt/remote-voice-vibe-coding/releases/$releaseId/attach_files"
    $resolved = (Resolve-Path -LiteralPath $filePath).Path
    curl.exe -sf -X POST $uri -F "access_token=$($env:GITEE_TOKEN)" -F "file=@$resolved"
}

function Ensure-GithubRelease {
    $token = if ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { $env:GH_TOKEN }
    if (-not $token) { throw "GITHUB_TOKEN or GH_TOKEN is required" }
    $headers = @{
        Authorization = "Bearer $token"
        Accept = "application/vnd.github+json"
        "X-GitHub-Api-Version" = "2022-11-28"
    }
    try {
        $existing = Invoke-RestMethod -Uri "https://api.github.com/repos/mwlt/Voice_VibeCoding/releases/tags/$Tag" -Headers $headers -Method Get
        if ($existing -and $existing.id) { return $existing }
    } catch {
        # 404 when release tag does not exist yet
    }
    return Invoke-RestMethod -Uri "https://api.github.com/repos/mwlt/Voice_VibeCoding/releases" -Headers $headers -Method Post -Body (@{
        tag_name = $Tag
        name = $Tag
        body = $body
        draft = $false
        prerelease = $false
    } | ConvertTo-Json -Depth 3)
}

function Upload-GithubAsset($release, $filePath, $assetName) {
    $token = if ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { $env:GH_TOKEN }
    $headers = @{
        Authorization = "Bearer $token"
        Accept = "application/vnd.github+json"
        "Content-Type" = "application/octet-stream"
        "X-GitHub-Api-Version" = "2022-11-28"
    }
    $encodedName = [uri]::EscapeDataString($assetName)
    $uploadUrl = ($release.upload_url -replace '\{\?name,label\}', "?name=$encodedName")
    Invoke-RestMethod -Uri $uploadUrl -Headers $headers -Method Post -InFile $filePath
}

Write-Host "Publishing $Tag ..."
if ($env:GITEE_TOKEN) {
    $gid = Ensure-GiteeRelease
    Write-Host "Gitee release id: $gid"
    Upload-GiteeAsset $gid $nsis
    Upload-GiteeAsset $gid $msi
    Upload-GiteeAsset $gid $winuhidZip
    Write-Host "Gitee upload done."
} else {
    Write-Warning "Skip Gitee: GITEE_TOKEN not set"
}

$ghToken = if ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } elseif ($env:GH_TOKEN) { $env:GH_TOKEN } else { $null }
if ($ghToken) {
    if (-not $env:GITHUB_TOKEN) { $env:GITHUB_TOKEN = $ghToken }
    $gh = Ensure-GithubRelease
    Write-Host "GitHub release id: $($gh.id)"
    Upload-GithubAsset $gh $ghNsis (Split-Path $ghNsis -Leaf)
    Upload-GithubAsset $gh $ghMsi (Split-Path $ghMsi -Leaf)
    Upload-GithubAsset $gh $winuhidZip (Split-Path $winuhidZip -Leaf)
    Write-Host "GitHub upload done."
} else {
    Write-Warning "Skip GitHub: GITHUB_TOKEN/GH_TOKEN not set"
}

Write-Host "Done."
