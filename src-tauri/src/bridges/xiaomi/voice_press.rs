//! 语音键按下编排 — 可测试的步骤顺序（L1）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoicePressStep {
    /// ATVV 解码器 / streaming 状态置位（不含 PCM CLEAR）
    ArmSessionState,
    /// 同步确保 PCM UDP 就绪
    EnsurePcmReady,
    /// WinUHid 快捷键 DOWN（输入法先开）
    ShortcutDown,
    /// UDP CLEAR → 路由开 VB-CABLE 流
    PcmClear,
    NotifyUi,
    MeterOn,
}

/// 遥控语音键按下时的执行顺序。
pub fn voice_remote_press_steps() -> &'static [VoicePressStep] {
    &[
        VoicePressStep::ArmSessionState,
        VoicePressStep::EnsurePcmReady,
        VoicePressStep::ShortcutDown,
        VoicePressStep::PcmClear,
        VoicePressStep::NotifyUi,
        VoicePressStep::MeterOn,
    ]
}
