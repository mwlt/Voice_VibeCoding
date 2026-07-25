//! UDP 音频服务器 — 接收来自 BLE 桥接的 PCM 音频数据
//!
//! 监听端口 31681，接收 JSON 控制命令 + 原始 PCM 数据包

use crate::bridges::shared::udp_protocol::{UdpMessage, UdpResponse, XIAOMI_AUDIO_PORT};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// 音频控制命令
#[derive(Debug, Clone)]
pub enum AudioCommand {
    Open { mic_device: String },
    Close,
    Status,
    PcmData(Vec<u8>),  // 原始 PCM 音频数据
}

/// 启动音频 UDP 服务器
pub async fn start_audio_udp_server(
    mut cmd_tx: mpsc::UnboundedSender<AudioCommand>,
) -> Result<(), String> {
    let addr = format!("127.0.0.1:{}", XIAOMI_AUDIO_PORT);
    let socket = UdpSocket::bind(&addr)
        .await
        .map_err(|e| format!("音频 UDP 绑定失败: {}", e))?;

    log::info!("Audio UDP server listening on {}", addr);

    let mut buf = [0u8; 65536];
    loop {
        let (n, peer) = socket.recv_from(&mut buf)
            .await
            .map_err(|e| format!("音频接收失败: {}", e))?;

        let data = &buf[..n];

        // 尝试解析为 JSON 控制命令
        if let Ok(msg) = serde_json::from_slice::<UdpMessage>(data) {
            let response = handle_audio_command(&msg, &mut cmd_tx);
            if let Some(resp) = response {
                if let Ok(json) = serde_json::to_vec(&resp) {
                    let _ = socket.send_to(&json, peer).await;
                }
            }
        } else {
            // 非 JSON = PCM 原始音频数据，转发到混音器
            let _ = cmd_tx.send(AudioCommand::PcmData(data.to_vec()));
        }
    }
}

/// 处理音频控制命令
fn handle_audio_command(
    msg: &UdpMessage,
    cmd_tx: &mut mpsc::UnboundedSender<AudioCommand>,
) -> Option<UdpResponse> {
    match msg.command.as_str() {
        "open" => {
            let mic = msg.params.get("device")
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string();
            let _ = cmd_tx.send(AudioCommand::Open { mic_device: mic });
            Some(UdpResponse::ok(
                serde_json::json!({"status": "opened"}),
                msg.request_id,
            ))
        }
        "close" => {
            let _ = cmd_tx.send(AudioCommand::Close);
            Some(UdpResponse::ok(
                serde_json::json!({"status": "closed"}),
                msg.request_id,
            ))
        }
        "status" => {
            let _ = cmd_tx.send(AudioCommand::Status);
            Some(UdpResponse::ok(
                serde_json::json!({"running": true}),
                msg.request_id,
            ))
        }
        _ => None,
    }
}
