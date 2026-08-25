//! 配置管理器 — 管理所有设备配置
//!
//! 配置文件存放于 %APPDATA%\RemoteBridgeHub\
//! - xiaomi.json   — 小米遥控器配置
//! - t1.json       — T1 遥控器配置
//! - hanvon.json   — 汉王 V60 配置
//! - settings.json — 全局设置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use parking_lot::Mutex;
use tauri::{AppHandle, Manager};

// ============================================================
// 数据类型定义
// ============================================================

/// 按键动作类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum KeyAction {
    /// 单个虚拟键码
    SingleKey(u16),
    /// 组合键（修饰符 + 键）
    ComboKey(Vec<u16>),
    /// 文本输入
    TextInput(String),
    /// 启动应用
    LaunchApp(String),
    /// 无动作
    None,
}

impl Default for KeyAction {
    fn default() -> Self {
        KeyAction::None
    }
}

/// 触发模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TriggerMode {
    /// 点击型：点一下开始（松手继续），再点结束并提交（需 MIC_OPEN 保活）
    Toggle,
    /// 按住型：按下说话，松开结束
    Hold,
}

impl Default for TriggerMode {
    fn default() -> Self {
        TriggerMode::Hold
    }
}

/// 设备配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// 按键别名：button_id → 显示名称
    pub button_aliases: HashMap<String, String>,
    /// 按键绑定：button_id → 动作
    pub button_bindings: HashMap<String, KeyAction>,
    /// 语音快捷键
    pub voice_hotkey: Option<Vec<String>>,
    /// 语音触发模式
    #[serde(default)]
    pub trigger_mode: TriggerMode,
    /// 蓝牙地址（仅小米）
    pub bluetooth_address: Option<String>,
    /// 语音增益 dB（对齐 Python gain_db）
    #[serde(default = "default_gain_db")]
    pub gain_db: f32,
    /// 断线重连间隔秒
    #[serde(default = "default_retry_delay")]
    pub retry_delay: f32,
    /// 是否启用语音快捷键
    #[serde(default = "default_true")]
    pub voice_shortcut_enabled: bool,
    /// TV 键就绪延迟秒
    #[serde(default = "default_tv_delay")]
    pub tv_action_ready_delay: f32,
    /// 特殊键抑制
    #[serde(default = "default_true")]
    pub special_key_hook_enabled: bool,
    /// HID Tap
    #[serde(default = "default_true")]
    pub hid_report_tap_enabled: bool,
}

fn default_gain_db() -> f32 {
    10.0
}
fn default_retry_delay() -> f32 {
    3.0
}
fn default_tv_delay() -> f32 {
    2.0
}
fn default_true() -> bool {
    true
}

impl DeviceConfig {
    pub fn new() -> Self {
        Self {
            button_aliases: HashMap::new(),
            button_bindings: HashMap::new(),
            voice_hotkey: Some(vec!["rightalt".into()]),
            trigger_mode: TriggerMode::Hold,
            bluetooth_address: None,
            gain_db: default_gain_db(),
            retry_delay: default_retry_delay(),
            voice_shortcut_enabled: true,
            tv_action_ready_delay: default_tv_delay(),
            special_key_hook_enabled: true,
            hid_report_tap_enabled: true,
        }
    }
}

/// 全局设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    /// 开机自启
    pub autostart: bool,
    /// 界面语言
    pub language: String,
    /// 最小化到托盘
    pub minimize_to_tray: bool,
    /// 启动后最小化到托盘（不显示主窗口）
    #[serde(default)]
    pub start_minimized_to_tray: bool,
    /// 用户忽略的更新版本（直到更高版本再提示）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignored_update_version: Option<String>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            autostart: false,
            language: "zh-CN".to_string(),
            minimize_to_tray: true,
            start_minimized_to_tray: false,
            ignored_update_version: None,
        }
    }
}

// ============================================================
// ConfigManager
// ============================================================

pub struct ConfigManager {
    config_dir: PathBuf,
    /// 设备配置内存缓存：按键热路径避免每次读盘+JSON
    device_cache: Mutex<HashMap<String, DeviceConfig>>,
}

impl ConfigManager {
    /// 创建配置管理器，自动创建配置目录
    pub fn new(app_handle: AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = get_config_dir(&app_handle)?;
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(config_dir.join("logs")).ok();
        Ok(Self {
            config_dir,
            device_cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.config_dir.join("logs")
    }

    /// 获取设备配置文件路径
    fn device_config_path(&self, device: &str) -> PathBuf {
        self.config_dir.join(format!("{}.json", device))
    }

    /// 获取全局设置文件路径
    fn settings_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    // ---- 设备配置 ----

    fn load_device_config_from_disk(&self, device: &str) -> Result<DeviceConfig, String> {
        let path = self.device_config_path(device);
        if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("读取配置失败: {}", e))?;
            let mut config: DeviceConfig = serde_json::from_str(&content)
                .map_err(|e| format!("解析配置失败: {}", e))?;
            if device == "xiaomi" {
                Self::merge_xiaomi_defaults(&mut config);
            }
            Ok(config)
        } else {
            Ok(Self::default_config_for(device))
        }
    }

    /// 获取设备配置（内存缓存；未命中再读盘）
    pub fn get_device_config(&self, device: &str) -> Result<DeviceConfig, String> {
        if let Some(cached) = self.device_cache.lock().get(device).cloned() {
            return Ok(cached);
        }
        let config = self.load_device_config_from_disk(device)?;
        self.device_cache
            .lock()
            .insert(device.to_string(), config.clone());
        Ok(config)
    }

    /// 使缓存失效（外部改文件时可选调用）
    pub fn invalidate_device_config(&self, device: &str) {
        self.device_cache.lock().remove(device);
    }

    /// 对齐 Python schema 升级：补齐缺失的默认绑定/别名，不覆盖用户已有项
    fn merge_xiaomi_defaults(config: &mut DeviceConfig) {
        let defaults = Self::default_config_for("xiaomi");
        for (k, v) in defaults.button_aliases {
            config.button_aliases.entry(k).or_insert(v);
        }
        for (k, v) in defaults.button_bindings {
            config.button_bindings.entry(k).or_insert(v);
        }
        if config.voice_hotkey.as_ref().map(|v| v.is_empty()).unwrap_or(true) {
            config.voice_hotkey = defaults.voice_hotkey;
        }
    }

    /// 保存设备配置（写临时文件 → sync → rename；并更新缓存）
    pub fn save_device_config(&self, device: &str, config: &DeviceConfig) -> Result<(), String> {
        let mut config = config.clone();
        if device == "xiaomi" {
            crate::bridges::xiaomi::key_mapping::sync_voice_from_mic_binding(&mut config);
        }
        let path = self.device_config_path(device);
        let tmp_path = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("序列化配置失败: {}", e))?;

        {
            let mut file = fs::File::create(&tmp_path)
                .map_err(|e| format!("写入临时文件失败: {}", e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("写入临时文件失败: {}", e))?;
            file.sync_all()
                .map_err(|e| format!("同步临时文件失败: {}", e))?;
        }

        fs::rename(&tmp_path, &path).map_err(|e| format!("替换配置文件失败: {}", e))?;

        self.device_cache
            .lock()
            .insert(device.to_string(), config);
        Ok(())
    }

    // ---- 全局设置 ----

    pub fn get_global_settings(&self) -> Result<GlobalSettings, String> {
        let path = self.settings_path();
        if path.exists() {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("读取设置失败: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("解析设置失败: {}", e))
        } else {
            Ok(GlobalSettings::default())
        }
    }

    pub fn save_global_settings(&self, settings: &GlobalSettings) -> Result<(), String> {
        let path = self.settings_path();
        let tmp_path = path.with_extension("json.tmp");

        let content = serde_json::to_string_pretty(settings)
            .map_err(|e| format!("序列化设置失败: {}", e))?;

        fs::write(&tmp_path, &content)
            .map_err(|e| format!("写入临时文件失败: {}", e))?;

        fs::rename(&tmp_path, &path)
            .map_err(|e| format!("替换设置文件失败: {}", e))?;

        Ok(())
    }

    // ---- 默认配置 ----

    /// 返回各设备的默认按键映射
    fn default_config_for(device: &str) -> DeviceConfig {
        match device {
            "xiaomi" => DeviceConfig {
                button_aliases: Self::xiaomi_button_aliases(),
                button_bindings: Self::xiaomi_default_bindings(),
                // 对齐用户调优配置：语音键 = Ctrl+左Win（微信输入法），点击模式
                voice_hotkey: Some(vec!["leftctrl".into(), "leftwin".into()]),
                trigger_mode: TriggerMode::Toggle,
                bluetooth_address: None,
                gain_db: 10.0,
                retry_delay: 3.0,
                voice_shortcut_enabled: true,
                tv_action_ready_delay: 2.0,
                special_key_hook_enabled: true,
                hid_report_tap_enabled: true,
            },
            "t1" => DeviceConfig {
                button_aliases: Self::t1_button_aliases(),
                button_bindings: Self::t1_default_bindings(),
                voice_hotkey: Some(vec!["rightalt".into()]),
                trigger_mode: TriggerMode::Hold,
                bluetooth_address: None,
                ..DeviceConfig::new()
            },
            "hanvon" => DeviceConfig {
                button_aliases: Self::hanvon_button_aliases(),
                button_bindings: Self::hanvon_default_bindings(),
                voice_hotkey: Some(vec!["rightalt".into()]),
                trigger_mode: TriggerMode::Hold,
                bluetooth_address: None,
                ..DeviceConfig::new()
            },
            _ => DeviceConfig::new(),
        }
    }

    // ---- 小米遥控器默认按键 ----
    fn xiaomi_button_aliases() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("power".into(), "电源".into());
        m.insert("volume_up".into(), "音量+".into());
        m.insert("volume_down".into(), "音量-".into());
        m.insert("up".into(), "上".into());
        m.insert("down".into(), "下".into());
        m.insert("left".into(), "左".into());
        m.insert("right".into(), "右".into());
        m.insert("dpad_up".into(), "上".into());
        m.insert("dpad_down".into(), "下".into());
        m.insert("dpad_left".into(), "左".into());
        m.insert("dpad_right".into(), "右".into());
        m.insert("ok".into(), "确定".into());
        m.insert("back".into(), "返回".into());
        m.insert("home".into(), "主页".into());
        m.insert("menu".into(), "菜单".into());
        m.insert("mic".into(), "语音".into());
        m.insert("voice".into(), "语音".into());
        m.insert("volume_mute".into(), "静音".into());
        m.insert("mute".into(), "静音".into());
        m.insert("tv".into(), "TV".into());
        m
    }

    fn xiaomi_default_bindings() -> HashMap<String, KeyAction> {
        // v1.3.14 起对齐用户实测调优后的推荐映射（仅首次安装生成，已有用户文件不覆盖）：
        // - menu → Shift+F10：即「鼠标右键」的键盘等价快捷键，通用性最好
        // - home → 空格：Win+D 会最小化所有窗口，媒体/翻页场景下空格更安全
        // - tv → PrtSc：Alt+Esc 切窗易误触，PrtSc 截图更实用
        // - mic/voice → Ctrl+左Win：微信输入法「启动语音输入」快捷键
        let mut m = HashMap::new();
        m.insert("power".into(), KeyAction::SingleKey(0x1B)); // Esc
        m.insert(
            "mic".into(),
            KeyAction::ComboKey(vec![0xA2, 0x5B]), // Left Ctrl + Left Win（微信语音）
        );
        m.insert("up".into(), KeyAction::SingleKey(0x26));
        m.insert("down".into(), KeyAction::SingleKey(0x28));
        m.insert("left".into(), KeyAction::SingleKey(0x25));
        m.insert("right".into(), KeyAction::SingleKey(0x27));
        m.insert("ok".into(), KeyAction::SingleKey(0x0D));
        m.insert("back".into(), KeyAction::SingleKey(0x08));
        m.insert("volume_up".into(), KeyAction::SingleKey(0xAF));
        m.insert("volume_down".into(), KeyAction::SingleKey(0xAE));
        m.insert("home".into(), KeyAction::SingleKey(0x20)); // Space
        m.insert(
            "menu".into(),
            KeyAction::ComboKey(vec![0xA0, 0x79]), // Shift+F10 = 鼠标右键
        );
        m.insert("tv".into(), KeyAction::SingleKey(0x2C)); // PrintScreen
        m.insert("volume_mute".into(), KeyAction::SingleKey(0xAD));
        // 兼容旧 UI id
        m.insert("dpad_up".into(), KeyAction::SingleKey(0x26));
        m.insert("dpad_down".into(), KeyAction::SingleKey(0x28));
        m.insert("dpad_left".into(), KeyAction::SingleKey(0x25));
        m.insert("dpad_right".into(), KeyAction::SingleKey(0x27));
        m.insert(
            "voice".into(),
            KeyAction::ComboKey(vec![0xA2, 0x5B]), // 与 mic 一致
        );
        m.insert("mute".into(), KeyAction::SingleKey(0xAD));
        m
    }

    // ---- T1 遥控器默认按键 ----
    fn t1_button_aliases() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("power".into(), "电源".into());
        m.insert("up".into(), "上".into());
        m.insert("down".into(), "下".into());
        m.insert("left".into(), "左".into());
        m.insert("right".into(), "右".into());
        m.insert("ok".into(), "确定".into());
        m.insert("delete".into(), "删除".into());
        m.insert("voice".into(), "语音".into());
        m.insert("mute".into(), "静音".into());
        m.insert("home".into(), "主页".into());
        m.insert("mouse".into(), "鼠标".into());
        m.insert("menu".into(), "菜单".into());
        m.insert("vol_plus".into(), "音量+".into());
        m.insert("vol_minus".into(), "音量-".into());
        m
    }

    fn t1_default_bindings() -> HashMap<String, KeyAction> {
        let mut m = HashMap::new();
        m.insert("up".into(), KeyAction::SingleKey(0x26));
        m.insert("down".into(), KeyAction::SingleKey(0x28));
        m.insert("left".into(), KeyAction::SingleKey(0x25));
        m.insert("right".into(), KeyAction::SingleKey(0x27));
        m.insert("ok".into(), KeyAction::SingleKey(0x0D));
        m.insert("delete".into(), KeyAction::SingleKey(0x08));
        m.insert("home".into(), KeyAction::ComboKey(vec![0x5B]));
        m.insert("vol_plus".into(), KeyAction::SingleKey(0xAF));
        m.insert("vol_minus".into(), KeyAction::SingleKey(0xAE));
        m.insert("mute".into(), KeyAction::SingleKey(0xAD));
        m
    }

    // ---- 汉王 V60 默认按键 ----
    fn hanvon_button_aliases() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("mic".into(), "麦克风".into());
        m.insert("page_up".into(), "上翻页".into());
        m.insert("page_down".into(), "下翻页".into());
        m
    }

    fn hanvon_default_bindings() -> HashMap<String, KeyAction> {
        let mut m = HashMap::new();
        // 麦克风键 → 右Alt（切换语音输入）
        m.insert("mic".into(), KeyAction::ComboKey(vec![0xA5])); // VK_RMENU
        // 上翻页 → 光标移到末尾+退格
        m.insert("page_up".into(), KeyAction::ComboKey(vec![0x23, 0x08])); // End+Backspace
        // 下翻页 → 点击回车
        m.insert("page_down".into(), KeyAction::SingleKey(0x0D));
        m
    }
}

// ============================================================
// 辅助函数
// ============================================================

/// 获取配置目录路径
fn get_config_dir(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let appdata = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {}", e))?;
    Ok(appdata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_xiaomi_config() {
        let config = ConfigManager::default_config_for("xiaomi");
        assert_eq!(config.button_aliases.len(), 13);
        assert!(config.button_bindings.contains_key("volume_up"));
        // v1.3.14 默认对齐用户调优配置：语音 = Ctrl+左Win，点击模式
        assert_eq!(
            config.voice_hotkey,
            Some(vec!["leftctrl".to_string(), "leftwin".to_string()])
        );
        assert_eq!(config.trigger_mode, TriggerMode::Toggle);
    }

    #[test]
    fn test_default_xiaomi_bindings_match_tuned_profile() {
        // 全新安装默认映射 = 用户调优后的推荐映射
        let b = ConfigManager::default_config_for("xiaomi").button_bindings;
        assert_eq!(b.get("power"), Some(&KeyAction::SingleKey(0x1B))); // Esc
        assert_eq!(
            b.get("menu"),
            Some(&KeyAction::ComboKey(vec![0xA0, 0x79])) // Shift+F10 = 鼠标右键
        );
        assert_eq!(b.get("home"), Some(&KeyAction::SingleKey(0x20))); // Space
        assert_eq!(b.get("tv"), Some(&KeyAction::SingleKey(0x2C))); // PrtSc
        assert_eq!(
            b.get("mic"),
            Some(&KeyAction::ComboKey(vec![0xA2, 0x5B])) // Ctrl+左Win
        );
        assert_eq!(b.get("voice"), b.get("mic"));
    }

    #[test]
    fn test_merge_xiaomi_defaults_never_overrides_user_bindings() {
        // 已有用户文件：改过 menu/home/tv/voice → merge 只补缺失项，绝不覆盖
        let user_json = r#"{
            "button_aliases": {"power": "电源"},
            "button_bindings": {
                "power": {"type": "SingleKey", "value": 27},
                "menu": {"type": "SingleKey", "value": 91},
                "home": {"type": "SingleKey", "value": 32},
                "tv": {"type": "SingleKey", "value": 44},
                "mic": {"type": "ComboKey", "value": [162, 91]}
            },
            "voice_hotkey": ["leftctrl", "leftwin"],
            "trigger_mode": "Toggle"
        }"#;
        let mut config: DeviceConfig = serde_json::from_str(user_json).unwrap();
        ConfigManager::merge_xiaomi_defaults(&mut config);

        assert_eq!(
            config.button_bindings.get("menu"),
            Some(&KeyAction::SingleKey(0x5B)),
            "用户的 menu=左Win 必须保留"
        );
        assert_eq!(
            config.button_bindings.get("home"),
            Some(&KeyAction::SingleKey(0x20)),
            "用户的 home=空格 必须保留"
        );
        assert_eq!(
            config.button_bindings.get("tv"),
            Some(&KeyAction::SingleKey(0x2C)),
            "用户的 tv=PrtSc 必须保留"
        );
        assert_eq!(
            config.button_bindings.get("back"),
            Some(&KeyAction::SingleKey(0x08)),
            "缺失的 back 由 merge 补齐为默认 Backspace"
        );
        assert_eq!(config.trigger_mode, TriggerMode::Toggle);
    }

    #[test]
    fn test_default_t1_config() {
        let config = ConfigManager::default_config_for("t1");
        assert_eq!(config.button_aliases.len(), 14);
        assert!(config.button_bindings.contains_key("ok"));
    }

    #[test]
    fn test_default_hanvon_config() {
        let config = ConfigManager::default_config_for("hanvon");
        assert_eq!(config.button_aliases.len(), 3);
        assert!(config.button_bindings.contains_key("mic"));
    }

    #[test]
    fn test_global_settings_default() {
        let settings = GlobalSettings::default();
        assert!(!settings.autostart);
        assert_eq!(settings.language, "zh-CN");
        assert!(settings.minimize_to_tray);
    }

    #[test]
    fn test_key_action_serialization() {
        let action = KeyAction::SingleKey(0x41);
        let json = serde_json::to_string(&action).unwrap();
        let decoded: KeyAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, decoded);

        let combo = KeyAction::ComboKey(vec![0x11, 0x41]);
        let json = serde_json::to_string(&combo).unwrap();
        let decoded: KeyAction = serde_json::from_str(&json).unwrap();
        assert_eq!(combo, decoded);
    }
}
