//! T1 BLE 主机状态快照（独立于小米 host status）

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

static ATVV_OK: AtomicBool = AtomicBool::new(false);

pub fn set_atvv_ok(ok: bool) {
    ATVV_OK.store(ok, Ordering::SeqCst);
}

pub fn atvv_ok() -> bool {
    ATVV_OK.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct T1BleHostStatusItem {
    pub id: String,
    pub label: String,
    pub state_label: String,
    /// ok | warn | error
    pub tone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct T1BleHostStatus {
    pub ble_alive: bool,
    pub usb_alive: bool,
    pub audio_alive: bool,
    pub cable_ready: bool,
    pub winuhid_ready: bool,
    pub atvv_ok: bool,
    pub l0_ready: bool,
    pub status_text: String,
    pub detail: String,
    pub tone: String,
    pub items: Vec<T1BleHostStatusItem>,
}

fn item(id: &str, label: &str, ok: bool, ok_label: &str, bad_label: &str) -> T1BleHostStatusItem {
    T1BleHostStatusItem {
        id: id.into(),
        label: label.into(),
        state_label: if ok {
            ok_label.into()
        } else {
            bad_label.into()
        },
        tone: if ok { "ok".into() } else { "error".into() },
    }
}

fn item_warn(id: &str, label: &str, ok: bool, ok_label: &str, bad_label: &str) -> T1BleHostStatusItem {
    T1BleHostStatusItem {
        id: id.into(),
        label: label.into(),
        state_label: if ok {
            ok_label.into()
        } else {
            bad_label.into()
        },
        tone: if ok {
            "ok".into()
        } else {
            "warn".into()
        },
    }
}

/// 纯函数：由运行标志拼装状态（便于单测）
pub fn build_host_status(
    ble_alive: bool,
    usb_alive: bool,
    audio_alive: bool,
    cable_ready: bool,
    winuhid_ready: bool,
    atvv_ok: bool,
    l0_ready: bool,
) -> T1BleHostStatus {
    let items = vec![
        item("cable", "虚拟声卡", cable_ready, "已安装", "未检测到"),
        item("winuhid", "虚拟键盘", winuhid_ready, "已就绪", "未就绪"),
        item_warn("l0", "Search剥离", l0_ready, "已就绪", "未安装"),
        item("audio", "语音路由", audio_alive, "运行中", "已停止"),
        item("ble", "蓝牙桥接", ble_alive, "已连接", "未启动"),
        item("atvv", "ATVV 语音", atvv_ok, "已订阅", "未订阅"),
        item("usb", "USB 桥接", usb_alive, "监听中", "未启动"),
    ];

    let (status_text, detail, tone) =
        if ble_alive && atvv_ok && audio_alive && cable_ready && winuhid_ready && l0_ready {
            ("蓝牙运行正常".into(), String::new(), "ok".into())
        } else if ble_alive && !l0_ready {
            (
                "Search 剥离未就绪".into(),
                "T1 蓝牙专属 L0 驱动可从根上挡住 Windows 搜索。点下方「自动修复 Search 剥离」。".into(),
                "warn".into(),
            )
        } else if ble_alive && !winuhid_ready {
            (
                "虚拟键盘未就绪".into(),
                "映射按键 / 语音唤醒需要 WinUHid。可到小米页「修复虚拟键盘」，或安装后重试。".into(),
                "warn".into(),
            )
        } else if ble_alive && !atvv_ok {
            (
                "ATVV 未就绪".into(),
                "蓝牙已连但语音通道未订阅。可断开后重连蓝牙。".into(),
                "warn".into(),
            )
        } else if ble_alive && !cable_ready {
            (
                "虚拟声卡未就绪".into(),
                "BLE 语音走 VB-CABLE。可到小米页「虚拟声卡修复」。".into(),
                "warn".into(),
            )
        } else if ble_alive && !audio_alive {
            (
                "语音路由未运行".into(),
                "audio_router 未就绪，PCM 可能无法进 CABLE。".into(),
                "warn".into(),
            )
        } else if ble_alive {
            ("蓝牙已连接".into(), String::new(), "ok".into())
        } else if usb_alive {
            (
                "仅 USB 运行中".into(),
                "点「蓝牙连接」可启用 ATVV 长时语音。".into(),
                "warn".into(),
            )
        } else {
            (
                "未启动".into(),
                "USB 与蓝牙均可分别连接。".into(),
                "error".into(),
            )
        };

    T1BleHostStatus {
        ble_alive,
        usb_alive,
        audio_alive,
        cable_ready,
        winuhid_ready,
        atvv_ok,
        l0_ready,
        status_text,
        detail,
        tone,
        items,
    }
}

pub fn host_status_now(app: &AppHandle) -> T1BleHostStatus {
    let ble_alive = app
        .try_state::<std::sync::Arc<crate::bridges::t1::ble_runtime::T1BleRuntime>>()
        .map(|r| crate::bridges::t1::ble_runtime::is_running(&r))
        .unwrap_or(false);
    let usb_alive = app
        .try_state::<std::sync::Arc<crate::bridges::t1::runtime::T1Runtime>>()
        .map(|r| r.is_running())
        .unwrap_or(false);
    let audio_alive = crate::audio::pcm_router::audio_router_ready()
        || crate::audio::pcm_router::audio_router_process_alive();
    let cable_ready = crate::audio::vb_cable::voice_env_status().ready;
    let winuhid_ready = crate::bridges::xiaomi::hid_injector::is_ready_cached();
    let l0_ready = crate::bridges::t1::t1_hid_filter_env::l0_ready_fast();
    build_host_status(
        ble_alive,
        usb_alive,
        audio_alive,
        cable_ready,
        winuhid_ready,
        atvv_ok(),
        l0_ready,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_ok_is_normal() {
        let s = build_host_status(true, false, true, true, true, true, true);
        assert_eq!(s.status_text, "蓝牙运行正常");
        assert_eq!(s.tone, "ok");
        assert!(s.items.iter().any(|i| i.id == "l0" && i.tone == "ok"));
    }

    #[test]
    fn ble_without_l0_warns() {
        let s = build_host_status(true, false, true, true, true, true, false);
        assert_eq!(s.tone, "warn");
        assert!(s.status_text.contains("Search"));
        assert!(s.items.iter().any(|i| i.id == "l0" && i.tone == "warn"));
    }

    #[test]
    fn ble_without_atvv_warns() {
        let s = build_host_status(true, false, true, true, true, false, true);
        assert_eq!(s.tone, "warn");
        assert!(s.status_text.contains("ATVV"));
    }

    #[test]
    fn idle_is_error_tone() {
        let s = build_host_status(false, false, false, false, false, false, false);
        assert_eq!(s.tone, "error");
        assert_eq!(s.ble_alive, false);
    }
}
