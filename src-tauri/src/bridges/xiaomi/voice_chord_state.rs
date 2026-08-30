//! 语音组合键状态机（与注入后端无关）。
//!
//! 调用方只提供 `inject(keys, key_up) -> bool`；此处保证：
//! - 记录实际 DOWN 的键位
//! - DOWN 失败立即补偿 KEYUP
//! - KEYUP 至多重试一次

#[derive(Default, Debug)]
pub struct VoiceChordState {
    held: Option<Vec<u16>>,
}

impl VoiceChordState {
    pub const fn empty() -> Self {
        Self { held: None }
    }

    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// 当前按住的 VK（供 F5 中和等外部编排读取）。
    pub fn held_keys(&self) -> Option<Vec<u16>> {
        self.held.clone()
    }

    pub fn press_with<F>(&mut self, keys: &[u16], mut inject: F) -> bool
    where
        F: FnMut(&[u16], bool) -> bool,
    {
        // 连点：若仍 marked held，先走完整 UP（含 sanitizer），再接受新 DOWN
        if let Some(prev) = self.held.take() {
            let _ = inject(&prev, true);
            let _ = inject(&prev, true);
        }
        if inject(keys, false) {
            self.held = Some(keys.to_vec());
            true
        } else {
            // SendInput / WinUHid 可能已写入前半段，必须补一组反向 KEYUP。
            let _ = inject(keys, true);
            false
        }
    }

    pub fn release_with<F>(&mut self, mut inject: F) -> Option<(Vec<u16>, bool)>
    where
        F: FnMut(&[u16], bool) -> bool,
    {
        let keys = self.held.take()?;
        let released = inject(&keys, true) || inject(&keys, true);
        Some((keys, released))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_keys_exposed_for_diagnostics() {
        let mut s = VoiceChordState::empty();
        assert!(s.held_keys().is_none());
        s.press_with(&[0xA2, 0x5B], |_, _| true);
        assert_eq!(s.held_keys(), Some(vec![0xA2, 0x5B]));
        let _ = s.release_with(|_, _| true);
        assert!(s.held_keys().is_none());
    }
}
