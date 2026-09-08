//! 注入用 VK/扫描码小工具（T1 / 小米均可依赖，避免 T1→xiaomi）

/// 与历史小米 `key_mapping::EXTRA_INFO` 一致，标记本应用 SendInput
pub const EXTRA_INFO: usize = 0x584D_4952;

pub const MODIFIER_VKS: [u16; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C];

pub fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12) || MODIFIER_VKS.contains(&vk)
}

/// 右/左 Alt 基扫描码 0x38；Ctrl 0x1D；Win 0x5B
pub fn scan_code_for_vk(vk: u16, mapped: u16) -> u16 {
    match vk {
        0xA4 | 0xA5 | 0x12 => {
            if mapped != 0 {
                mapped
            } else {
                0x38
            }
        }
        0xA2 | 0xA3 | 0x11 => {
            if mapped != 0 {
                mapped
            } else {
                0x1D
            }
        }
        0x5B | 0x5C => {
            if mapped != 0 {
                mapped
            } else {
                0x5B
            }
        }
        _ if mapped != 0 => mapped,
        _ => mapped,
    }
}
