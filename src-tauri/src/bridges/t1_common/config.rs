//! T1 配置

/// T1 默认按键别名
pub fn default_button_aliases() -> Vec<(&'static str, &'static str)> {
    vec![
        ("power", "电源"),
        ("up", "上"),
        ("down", "下"),
        ("left", "左"),
        ("right", "右"),
        ("ok", "确定"),
        ("delete", "删除"),
        ("voice", "语音"),
        ("mute", "静音"),
        ("home", "主页"),
        ("mouse", "鼠标"),
        ("menu", "菜单"),
        ("vol_plus", "音量+"),
        ("vol_minus", "音量-"),
    ]
}
