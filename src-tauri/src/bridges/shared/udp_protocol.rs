//! UDP JSON 协议 — 本地进程间通信
//!
//! 端口分配:
//! - 31681  Xiaomi 音频控制
//! - 30682  T1 控制（中心模式）
//! - 31682  T1 控制（独立模式）
//! - 30683  V60 控制
//! - 28690  中心 SHOW 命令

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

pub const XIAOMI_AUDIO_PORT: u16 = 31681;
pub const T1_CONTROL_PORT: u16 = 30682;
pub const T1_STANDALONE_PORT: u16 = 31682;
pub const V60_CONTROL_PORT: u16 = 30683;
pub const HUB_SHOW_PORT: u16 = 28690;
pub const LOCALHOST: &str = "127.0.0.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpMessage {
    pub command: String,
    #[serde(default)]
    pub params: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpResponse {
    pub status: String,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
}

impl UdpMessage {
    pub fn new(command: impl Into<String>) -> Self {
        Self { command: command.into(), params: serde_json::Value::Null, request_id: None }
    }
    pub fn with_id(command: impl Into<String>, request_id: u64) -> Self {
        Self { command: command.into(), params: serde_json::Value::Null, request_id: Some(request_id) }
    }
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }
}

impl UdpResponse {
    pub fn is_ok(&self) -> bool { self.status == "ok" }
    pub fn ok(data: serde_json::Value, request_id: Option<u64>) -> Self {
        Self { status: "ok".into(), data, request_id }
    }
    pub fn error(msg: impl Into<String>, request_id: Option<u64>) -> Self {
        Self { status: "error".into(), data: serde_json::Value::String(msg.into()), request_id }
    }
}

/// 发送 UDP 命令并等待响应（带 2 秒超时）
pub async fn send_udp_command(port: u16, msg: &UdpMessage) -> Result<UdpResponse, String> {
    let addr = format!("{}:{}", LOCALHOST, port);
    let socket = UdpSocket::bind(format!("{}:0", LOCALHOST))
        .await
        .map_err(|e| format!("绑定失败: {}", e))?;
    socket.connect(&addr).await.map_err(|e| format!("连接失败: {}", e))?;

    let payload = serde_json::to_vec(msg).map_err(|e| format!("序列化: {}", e))?;
    socket.send(&payload).await.map_err(|e| format!("发送失败: {}", e))?;

    let mut buf = [0u8; 4096];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        socket.recv(&mut buf),
    )
    .await
    .map_err(|_| "UDP 响应超时".to_string())?
    .map_err(|e| format!("接收失败: {}", e))?;

    serde_json::from_slice(&buf[..n]).map_err(|e| format!("解析响应: {}", e))
}

/// 启动 UDP 监听器
pub async fn start_udp_listener<F>(port: u16, handler: F) -> Result<(), String>
where
    F: Fn(UdpMessage) -> Option<UdpResponse> + Send + Sync + 'static,
{
    let addr = format!("{}:{}", LOCALHOST, port);
    let socket = UdpSocket::bind(&addr)
        .await
        .map_err(|e| format!("监听绑定失败: {}", e))?;

    let handler = std::sync::Arc::new(handler);
    let mut buf = [0u8; 4096];

    loop {
        let (n, peer) = socket.recv_from(&mut buf).await.map_err(|e| format!("接收失败: {}", e))?;
        let msg: UdpMessage = match serde_json::from_slice(&buf[..n]) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let request_id = msg.request_id;
        let h = handler.clone();
        if let Some(response) = h(msg) {
            let mut resp = response;
            if resp.request_id.is_none() { resp.request_id = request_id; }
            if let Ok(json) = serde_json::to_vec(&resp) {
                let _ = socket.send_to(&json, peer).await;
            }
        }
    }
}

/// SHOW 命令处理器
pub fn handle_show(msg: &UdpMessage) -> Option<UdpResponse> {
    Some(UdpResponse::ok(serde_json::Value::String("shown".into()), msg.request_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_roundtrip() {
        let msg = UdpMessage { command: "open".into(), params: serde_json::json!({"d":"x"}), request_id: Some(42) };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: UdpMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.command, "open");
        assert_eq!(decoded.request_id, Some(42));
    }

    #[test]
    fn test_response_serialization() {
        let resp = UdpResponse::ok(serde_json::json!({"t":"abc"}), Some(7));
        assert!(resp.is_ok());
        assert_eq!(resp.request_id, Some(7));
    }
}
