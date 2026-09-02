//! 麦克风增益 (dB) 热更新：配置保存与 ATVV 音频路径共享同一 live 值。
//!
//! 范围与前端 `XiaomiSettings.vue` 的 `GAIN_MIN` / `GAIN_MAX` 保持一致。

use std::sync::atomic::{AtomicU32, Ordering};

/// 与 UI 步进器一致
pub const GAIN_DB_MIN: f32 = -12.0;
pub const GAIN_DB_MAX: f32 = 30.0;
pub const GAIN_DB_DEFAULT: f32 = 10.0;

static LIVE_GAIN_DB: AtomicU32 = AtomicU32::new(GAIN_DB_DEFAULT.to_bits());

/// 规范化增益 dB（NaN → 默认，超出范围 clamp）。
pub fn normalize_gain_db(gain_db: f32) -> f32 {
    if gain_db.is_nan() {
        GAIN_DB_DEFAULT
    } else {
        gain_db.clamp(GAIN_DB_MIN, GAIN_DB_MAX)
    }
}

/// 更新 live 增益（配置落盘成功后或会话建立时调用）。
pub fn set_gain_db(gain_db: f32) {
    let clamped = normalize_gain_db(gain_db);
    let prev = f32::from_bits(LIVE_GAIN_DB.load(Ordering::Acquire));
    LIVE_GAIN_DB.store(clamped.to_bits(), Ordering::Release);
    if (prev - clamped).abs() > f32::EPSILON {
        log::info!("voice gain live: {prev:.1} dB -> {clamped:.1} dB");
    }
}

/// 当前应用于 PCM postprocess 的增益 dB。
pub fn gain_db() -> f32 {
    f32::from_bits(LIVE_GAIN_DB.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_clamps_to_ui_range() {
        set_gain_db(99.0);
        assert_eq!(gain_db(), GAIN_DB_MAX);
        set_gain_db(-99.0);
        assert_eq!(gain_db(), GAIN_DB_MIN);
        set_gain_db(14.5);
        assert!((gain_db() - 14.5).abs() < f32::EPSILON);
        set_gain_db(f32::NAN);
        assert_eq!(gain_db(), GAIN_DB_DEFAULT);
        assert_eq!(normalize_gain_db(f32::INFINITY), GAIN_DB_MAX);
        assert_eq!(normalize_gain_db(f32::NEG_INFINITY), GAIN_DB_MIN);
        set_gain_db(GAIN_DB_DEFAULT);
    }
}
