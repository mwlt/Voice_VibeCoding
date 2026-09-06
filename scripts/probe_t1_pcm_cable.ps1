# Live probe: same 16k→48k LE PCM T1 BLE uses, UDP to audio_router on CABLE.
$ErrorActionPreference = "Stop"
$port = 31680
$udp = New-Object System.Net.Sockets.UdpClient
$udp.Client.ReceiveTimeout = 800
$udp.Connect("127.0.0.1", $port)
$enc = [Text.Encoding]::ASCII
[void]$udp.Send($enc.GetBytes("PING"), 4)
$remote = New-Object System.Net.IPEndPoint([Net.IPAddress]::Any, 0)
try {
  $pong = $udp.Receive([ref]$remote)
} catch {
  Write-Host "FAIL no PONG from pcm router :$port"
  exit 1
}
$pongText = $enc.GetString($pong)
if ($pongText -ne "PONG") {
  Write-Host "FAIL unexpected reply $pongText"
  exit 1
}
Write-Host "PONG ok"

function Encode-16kToCableLe([int16[]]$samples) {
  $out = New-Object System.Collections.Generic.List[byte]
  $last = [Nullable[int16]]$null
  foreach ($s in $samples) {
    $a = if ($last.HasValue) { [int16](([int]$last.Value + [int]$s) / 2) } else { $s }
    $last = $s
    foreach ($sample in @($a, $s, $s)) {
      $bytes = [BitConverter]::GetBytes([int16]$sample)
      $out.Add($bytes[0]); $out.Add($bytes[1])
    }
  }
  ,$out.ToArray()
}

$tone = New-Object int16[] 160
for ($i = 0; $i -lt $tone.Length; $i++) {
  $tone[$i] = [int16]([math]::Sin($i * 2 * [math]::PI * 440 / 16000) * 8000)
}
$pcm = Encode-16kToCableLe $tone
if ($pcm.Length -ne ($tone.Length * 6)) {
  Write-Host "FAIL encode size $($pcm.Length)"
  exit 1
}
[void]$udp.Send($enc.GetBytes("CLEAR"), 5)
[void]$udp.Send($pcm, $pcm.Length)
[void]$udp.Send($enc.GetBytes("END"), 3)
$udp.Close()
Write-Host "PASS sent $($pcm.Length) bytes 48k LE to CABLE router"
exit 0
