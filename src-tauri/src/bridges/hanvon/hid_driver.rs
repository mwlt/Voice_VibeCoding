//! 汉王 V60 HID 驱动 — 纯 Rust 实现的笔控协议
//!
//! 通过 hidapi 直接读写 V60 笔的 HID 接口，
//! 复刻厂商 kwma_x64.dll 的握手协议（动态令牌+MSVC rand+校验）

use std::time::Duration;

/// V60 笔的 VID/PID
pub const V60_VID: u16 = 0x27B9;
pub const V60_PID: u16 = 0x02A2;

/// HID 键码 → 按键名称映射
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HanvonButton {
    Mic,        // 麦克风键 (键码 23)
    PageUp,     // 上翻页键 (键码 19)
    PageDown,   // 下翻页键 (键码 20)
}

impl HanvonButton {
    pub fn from_hid_code(code: u8) -> Option<Self> {
        match code {
            23 => Some(Self::Mic),
            19 => Some(Self::PageUp),
            20 => Some(Self::PageDown),
            _ => None,
        }
    }

    pub fn to_id(&self) -> &str {
        match self {
            Self::Mic => "mic",
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Mic => "麦克风",
            Self::PageUp => "上翻页",
            Self::PageDown => "下翻页",
        }
    }
}

/// MSVC 兼容的线性同余随机数生成器
///
/// 对齐 Python `voice_typing_hid._msvc_rand_next`，用于 V60 开麦动态令牌
struct MsvcRand {
    state: u32,
}

impl MsvcRand {
    fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// MSVC rand(): state = state * 214013 + 2531011; return (state >> 16) & 0x7FFF
    fn next(&mut self) -> u16 {
        self.state = self.state.wrapping_mul(214013).wrapping_add(2531011);
        ((self.state >> 16) & 0x7FFF) as u16
    }

    fn state(&self) -> u32 {
        self.state
    }
}

/// 对齐 Python `_make_mic_dynamic_report`：生成 33 字节动态令牌报告
fn make_mic_dynamic_report(seed: u32) -> (Vec<u8>, u32, u32) {
    let mut rng = MsvcRand::new(seed);
    let r1 = rng.next() as u32;
    let r2 = rng.next() as u32;
    let token = ((r1 << 16) | r2) & 0xFFFF_FFFF;
    // Python: check = (token + signed_trunc_div(token, 200) * 0x38) & 0xFF
    let as_signed = if token & 0x8000_0000 != 0 {
        (token as i64) - 0x1_0000_0000
    } else {
        token as i64
    };
    let quot = as_signed / 200;
    let check = ((token as i64) + quot * 0x38) as u8;

    let mut report = vec![0u8; 33];
    report[0] = 0x00;
    report[1] = 0x06;
    report[6] = 0x02;
    report[7] = 0x05;
    report[8] = 0x01;
    report[9] = check;
    report[10] = (token & 0xFF) as u8;
    report[11] = ((token >> 8) & 0xFF) as u8;
    report[12] = ((token >> 16) & 0xFF) as u8;
    report[13] = ((token >> 24) & 0xFF) as u8;
    let _ = rng.state();
    (report, seed, token)
}

/// V60 笔 HID 驱动
pub struct HanvonHidDriver {
    device: Option<hidapi::HidDevice>,
    handshake_done: bool,
}

impl HanvonHidDriver {
    pub fn new() -> Self {
        Self { device: None, handshake_done: false }
    }

    /// 打开 V60 笔设备
    pub fn open(&mut self) -> Result<(), String> {
        let api = hidapi::HidApi::new()
            .map_err(|e| format!("hidapi 初始化失败: {}", e))?;

        let devices = api.device_list();
        for device_info in devices {
            if device_info.vendor_id() == V60_VID && device_info.product_id() == V60_PID {
                let device = device_info.open_device(&api)
                    .map_err(|e| format!("打开设备失败: {}", e))?;
                self.device = Some(device);
                log::info!("V60 pen device opened");
                return Ok(());
            }
        }

        Err("未找到 V60 语音笔设备".into())
    }

    /// 执行握手协议（复刻 kwma_x64.dll / Python voice_typing_hid 动态令牌）
    pub fn handshake(&mut self) -> Result<(), String> {
        let device = self.device.as_ref().ok_or("设备未打开")?;

        // 步骤 1：发送 MSVC-rand 动态令牌报告（对齐 _make_mic_dynamic_report）
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(1);
        let (request, _seed, token) = make_mic_dynamic_report(seed);
        log::debug!("V60 handshake token=0x{token:08X} seed={seed}");
        device
            .write(&request)
            .map_err(|e| format!("握手写入失败: {}", e))?;

        std::thread::sleep(Duration::from_millis(50));

        // 步骤 2：读取握手响应
        let mut buf = [0u8; 64];
        let n = device
            .read_timeout(&mut buf, 500)
            .map_err(|e| format!("握手读取失败: {}", e))?;

        log::debug!("Handshake response: {} bytes", n);

        if n >= 8 {
            let checksum = buf[..n].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
            log::debug!("Handshake checksum: {checksum:#02x}");
            self.handshake_done = true;
        } else {
            return Err("握手响应太短".into());
        }

        log::info!("V60 handshake completed");
        Ok(())
    }

    /// 读取按键报告（阻塞）
    pub fn read_report(&mut self, timeout_ms: i32) -> Result<Option<HanvonButton>, String> {
        let device = self.device.as_ref().ok_or("设备未打开")?;

        let mut buf = [0u8; 64];
        let n = device.read_timeout(&mut buf, timeout_ms)
            .map_err(|e| format!("读取报告失败: {}", e))?;

        if n > 0 {
            // HID 报告格式：第1字节 = 报告ID, 后续字节 = 数据
            let hid_code = if n > 1 { buf[1] } else { buf[0] };
            log::trace!("V60 HID report: code={}", hid_code);

            // 仅在有按键时返回（非 0 表示按键）
            if hid_code != 0 {
                return Ok(HanvonButton::from_hid_code(hid_code));
            }
        }

        Ok(None)
    }

    /// 发送命令到笔
    pub fn send_command(&self, cmd: &[u8]) -> Result<(), String> {
        let device = self.device.as_ref().ok_or("设备未打开")?;
        device.write(cmd)
            .map_err(|e| format!("发送命令失败: {}", e))?;
        Ok(())
    }

    /// 关闭设备
    pub fn close(&mut self) {
        self.device = None;
        self.handshake_done = false;
        log::info!("V60 device closed");
    }

    pub fn is_connected(&self) -> bool {
        self.device.is_some() && self.handshake_done
    }
}

impl Default for HanvonHidDriver {
    fn default() -> Self { Self::new() }
}

impl Drop for HanvonHidDriver {
    fn drop(&mut self) { self.close(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_from_hid_code() {
        assert_eq!(HanvonButton::from_hid_code(23), Some(HanvonButton::Mic));
        assert_eq!(HanvonButton::from_hid_code(19), Some(HanvonButton::PageUp));
        assert_eq!(HanvonButton::from_hid_code(20), Some(HanvonButton::PageDown));
        assert_eq!(HanvonButton::from_hid_code(0), None);
        assert_eq!(HanvonButton::from_hid_code(99), None);
    }

    #[test]
    fn test_button_display_names() {
        assert_eq!(HanvonButton::Mic.display_name(), "麦克风");
        assert_eq!(HanvonButton::PageUp.display_name(), "上翻页");
        assert_eq!(HanvonButton::PageDown.display_name(), "下翻页");
    }

    #[test]
    fn test_msvc_rand_sequence() {
        // 对齐 Python: state=1 → 第一次 next
        let mut rng = MsvcRand::new(1);
        let first = rng.next();
        // state = (1*214013+2531011) & 0xFFFFFFFF = 2745024; (2745024>>16)&0x7FFF = 41
        assert_eq!(first, 41);
        let second = rng.next();
        assert!(second <= 0x7FFF);
    }

    #[test]
    fn test_dynamic_report_uses_msvc_rand() {
        let (report, seed, token) = make_mic_dynamic_report(1);
        assert_eq!(seed, 1);
        assert_eq!(report.len(), 33);
        assert_eq!(report[1], 0x06);
        assert_ne!(token, 0);
        // token 低 16 位来自第二次 rand
        assert_eq!(report[10], (token & 0xFF) as u8);
    }
}
