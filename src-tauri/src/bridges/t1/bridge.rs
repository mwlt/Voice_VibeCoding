//! T1 桥接 — Raw Input 键盘/鼠标桥接
//!
//! T1 谷歌遥控器通过 USB 接收器连接，使用 Raw Input API 监听按键

use crate::bridges::shared::raw_input::{RawInputBridge, RawInputEvent, RawInputDeviceType};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// T1 按钮名称
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum T1Button {
    Power, Up, Down, Left, Right, Ok, Delete,
    Voice, Mute, Home, Mouse, Menu, VolPlus, VolMinus,
}

impl T1Button {
    pub fn to_id(&self) -> &str {
        match self {
            T1Button::Power => "power",
            T1Button::Up => "up",
            T1Button::Down => "down",
            T1Button::Left => "left",
            T1Button::Right => "right",
            T1Button::Ok => "ok",
            T1Button::Delete => "delete",
            T1Button::Voice => "voice",
            T1Button::Mute => "mute",
            T1Button::Home => "home",
            T1Button::Mouse => "mouse",
            T1Button::Menu => "menu",
            T1Button::VolPlus => "vol_plus",
            T1Button::VolMinus => "vol_minus",
        }
    }
}

/// T1 桥接管理器
pub struct T1Bridge {
    raw_bridge: RawInputBridge,
    running: Arc<AtomicBool>,
    key_mappings: HashMap<String, Vec<u16>>, // button_id → VK codes
}

impl T1Bridge {
    pub fn new() -> Self {
        Self {
            raw_bridge: RawInputBridge::new(),
            running: Arc::new(AtomicBool::new(false)),
            key_mappings: HashMap::new(),
        }
    }

    /// 设置按键映射
    pub fn set_key_mappings(&mut self, mappings: HashMap<String, Vec<u16>>) {
        self.key_mappings = mappings;
    }

    /// 启动 T1 桥接
    pub fn start<F>(&mut self, on_event: F) -> Result<(), String>
    where
        F: Fn(T1Button, bool) + Send + 'static,
    {
        if self.running.load(Ordering::SeqCst) {
            return Err("T1 桥接已在运行".into());
        }
        self.running.store(true, Ordering::SeqCst);

        let _ = self.raw_bridge.start(move |event: RawInputEvent| {
            // 将 RawInput 事件映射到 T1 按钮
            if let Some(button) = map_raw_to_t1(&event) {
                on_event(button, event.pressed);
            }
        });

        log::info!("T1 bridge started");
        Ok(())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.raw_bridge.stop();
        log::info!("T1 bridge stopped");
    }

    pub fn is_running(&self) -> bool { self.running.load(Ordering::SeqCst) }
}

/// 将 RawInput 事件映射到 T1 按钮
fn map_raw_to_t1(event: &RawInputEvent) -> Option<T1Button> {
    match event.device_type {
        RawInputDeviceType::Keyboard => match event.usage_id {
            0x42 => Some(T1Button::Up),
            0x43 => Some(T1Button::Down),
            0x44 => Some(T1Button::Left),
            0x45 => Some(T1Button::Right),
            0x41 => Some(T1Button::Ok),
            0x4C => Some(T1Button::Delete),
            0x29 => Some(T1Button::Delete),  // ESC → Delete
            0xE9 => Some(T1Button::VolPlus),
            0xEA => Some(T1Button::VolMinus),
            0xE2 => Some(T1Button::Mute),
            0xCF => Some(T1Button::Voice),
            0x30 => Some(T1Button::Power),
            _ => None,
        },
        RawInputDeviceType::Mouse => {
            if event.usage_id == 0x01 {
                Some(T1Button::Mouse) // Mouse movement
            } else {
                None
            }
        }
        RawInputDeviceType::HID => None,
    }
}
