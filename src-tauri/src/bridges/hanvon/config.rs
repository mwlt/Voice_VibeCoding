//! V60 配置

/// V60 默认按键别名
pub fn default_button_aliases() -> Vec<(&'static str, &'static str)> {
    vec![
        ("mic", "麦克风"),
        ("page_up", "上翻页"),
        ("page_down", "下翻页"),
    ]
}
