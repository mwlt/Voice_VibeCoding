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

    pub fn press_with<F>(&mut self, keys: &[u16], mut inject: F) -> bool
    where
        F: FnMut(&[u16], bool) -> bool,
    {
        if self.held.is_some() {
            return false;
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
