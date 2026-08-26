//! 语音键抬起后的附加行为决策（纯函数，便于测试）。

use crate::config::manager::VoiceReleaseBehavior;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceReleaseDecision {
    NoExtraTap,
    TapSameChord,
}

pub fn should_tap_same_chord_after_up(behavior: VoiceReleaseBehavior) -> VoiceReleaseDecision {
    match behavior {
        VoiceReleaseBehavior::None => VoiceReleaseDecision::NoExtraTap,
        VoiceReleaseBehavior::TapSameChord => VoiceReleaseDecision::TapSameChord,
    }
}
