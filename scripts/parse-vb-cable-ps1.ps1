$path = "d:\00vscode_workspace\remote-bridge-hub-master\xiaomi_remote_2_pro_rust_deepseek\src-tauri\assets\xiaomi\configure-xiaomi-audio.ps1"
$tokens = $null
$errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors)
if ($errors -and $errors.Count -gt 0) {
  Write-Host "PARSE_FAIL"
  $errors | ForEach-Object { Write-Host $_.ToString() }
  exit 1
}
Write-Host "PARSE_OK"
exit 0
