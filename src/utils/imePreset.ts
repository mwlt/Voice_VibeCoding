import type { DeviceConfig, KeyAction, TriggerMode, VoiceReleaseBehavior } from "../types";

export type ImePresetId =
  | "wechat-hold"
  | "doubao-hold"
  | "doubao-hands-free"
  | "qianwen-hold"
  | "qianwen-win-alt"
  | "qianwen-ctrl-win";

export type ImeTabId = "wechat" | "doubao" | "qianwen" | "faq";

export interface ImeTabDefinition {
  id: ImeTabId;
  label: string;
  presetIds: ImePresetId[];
}

export interface ImePresetDefinition {
  id: ImePresetId;
  title: string;
  tag: string;
  shortcutVks: number[];
  voiceHotkey: string[];
  triggerMode: TriggerMode;
  voiceReleaseBehavior: VoiceReleaseBehavior;
  applyHint: string;
  logMessage: string;
  /** Short setup steps shown in 输入法设置 */
  steps: string[];
}

export interface ImeFaqSection {
  title: string;
  items: string[];
}

export const IME_FAQ = {
  warnTitle:
    "某些输入法（例如微信输入法）设置了某快捷键/组合快捷键后，会阻碍本软件录入相同快捷键/组合，请参考「可行设置流程」进行设置。",
  sections: [
    {
      title: "可行设置流程",
      items: [
        "录入前先临时关掉或改掉输入法语音快捷键（或先切到其它输入法），本软件录完后再改回。",
        "不必现场录入：在对应输入法 Tab 点「快速应用」，直接写好本软件映射。",
        "先录一个输入法暂未占用的组合键，录完后再把输入法快捷键改成与本软件一致。",
      ],
    },
    {
      title: "虚拟键盘（WinUHid）",
      items: [
        "豆包 / 千问语音唤醒需要虚拟键盘已就绪（状态栏「虚拟键盘 · 已就绪」）。",
        "若未就绪，点「修复虚拟键盘」安装内嵌驱动（需管理员确认）。",
        "千问「左 Win + 左 Alt」「左 Ctrl + 左 Win」等多修饰键组合会自动分步注入。",
      ],
    },
    {
      title: "其它输入法",
      items: [
        "在「按键映射」中把语音键设成与输入法相同的组合，并选择「按住」或「点击」。",
      ],
    },
  ] satisfies ImeFaqSection[],
};

export const IME_TABS: ImeTabDefinition[] = [
  {
    id: "wechat",
    label: "微信输入法",
    presetIds: ["wechat-hold"],
  },
  {
    id: "doubao",
    label: "豆包",
    presetIds: ["doubao-hold", "doubao-hands-free"],
  },
  {
    id: "qianwen",
    label: "千问",
    presetIds: ["qianwen-ctrl-win", "qianwen-win-alt", "qianwen-hold"],
  },
  { id: "faq", label: "常见问题", presetIds: [] },
];

export const IME_PRESETS: Record<ImePresetId, ImePresetDefinition> = {
  "wechat-hold": {
    id: "wechat-hold",
    title: "按住说话",
    tag: "按住 · Ctrl+Win",
    shortcutVks: [0xa2, 0x5b],
    voiceHotkey: ["leftctrl", "leftwin"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：语音键 = 左 Ctrl + 左 Win，触发模式 = 按住",
    logMessage: "设置建议：已快速应用微信按住说话映射（左 Ctrl + 左 Win）",
    steps: [
      "本软件语音键设为「左 Ctrl + 左 Win」，触发模式选「按住」（可用下方快速应用）。",
      "打开微信输入法 → 设置 → 快捷键，将「按住说话」设为与本软件相同的组合键。",
      "听写麦克风选 CABLE Output（VB-Audio Virtual Cable）。",
    ],
  },
  "doubao-hold": {
    id: "doubao-hold",
    title: "长按语音",
    tag: "按住 · 右 Alt",
    shortcutVks: [0xa5],
    voiceHotkey: ["rightalt"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：语音键 = 右 Alt，触发模式 = 按住",
    logMessage: "设置建议：已快速应用豆包长按语音映射（右 Alt）",
    steps: [
      "本软件语音键设为「右 Alt」，触发模式选「按住」。",
      "在豆包中启用长按语音，并将快捷键设为右 Alt（与本软件一致）。",
      "听写麦克风选 CABLE Output；焦点放在可输入的文本框。",
      "按住遥控语音键说话，松开后豆包应结束听写并上屏。",
    ],
  },
  "doubao-hands-free": {
    id: "doubao-hands-free",
    title: "免按语音",
    tag: "点击 · 右 Alt+空格",
    shortcutVks: [0xa5, 0x20],
    voiceHotkey: ["rightalt", "space"],
    triggerMode: "Toggle",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：语音键 = 右 Alt + 空格，触发模式 = 点击",
    logMessage: "设置建议：已快速应用豆包免按语音映射（右 Alt + 空格）",
    steps: [
      "本软件语音键设为「右 Alt + 空格」，触发模式选「点击」。",
      "在豆包中开启免按/开关式语音，并将快捷键设为右 Alt + 空格。",
      "听写麦克风选 CABLE Output。",
    ],
  },
  "qianwen-hold": {
    id: "qianwen-hold",
    title: "右 Alt",
    tag: "",
    shortcutVks: [0xa5],
    voiceHotkey: ["rightalt"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：语音键 = 右 Alt，触发模式 = 按住",
    logMessage: "设置建议：已快速应用千问按住说话映射（右 Alt）",
    steps: [],
  },
  "qianwen-win-alt": {
    id: "qianwen-win-alt",
    title: "左 Win + 左 Alt",
    tag: "",
    shortcutVks: [0x5b, 0xa4],
    voiceHotkey: ["leftwin", "leftalt"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：语音键 = 左 Win + 左 Alt，触发模式 = 按住",
    logMessage: "设置建议：已快速应用千问按住说话映射（左 Win + 左 Alt）",
    steps: [],
  },
  "qianwen-ctrl-win": {
    id: "qianwen-ctrl-win",
    title: "左 Ctrl + 左 Win",
    tag: "",
    shortcutVks: [0xa2, 0x5b],
    voiceHotkey: ["leftctrl", "leftwin"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：语音键 = 左 Ctrl + 左 Win，触发模式 = 按住",
    logMessage: "设置建议：已快速应用千问按住说话映射（左 Ctrl + 左 Win）",
    steps: [],
  },
};

/** 千问 Tab 合并展示：共用说明 + 三个快速应用按钮 */
export const QIANWEN_GUIDE = {
  title: "按住说话",
  tag: "三种快捷键任选其一",
  steps: [
    "在千问设置中，将按住语音快捷键选为「左 Ctrl + 左 Win」「左 Win + 左 Alt」或「右 Alt」之一（须与下方快速应用按钮一致）。",
    "点下方对应「快速应用」按钮，本软件语音键与触发模式（按住）会自动写好。",
    "听写麦克风选 CABLE Output；焦点放在可输入文本框。",
    "若选前两种组合，需虚拟键盘（WinUHid）已就绪；多修饰键会分步注入以匹配真实按键时序。",
    "按住遥控语音键说话，松开后结束听写并上屏。",
  ],
} as const;

export const QIANWEN_PRESET_IDS: ImePresetId[] = [
  "qianwen-ctrl-win",
  "qianwen-win-alt",
  "qianwen-hold",
];

/** 按键映射页 · 语音键一行快速应用（TV 卡片下方） */
export interface VoiceQuickPreset {
  id: string;
  presetId: ImePresetId;
  /** 键帽分段文案，组合键按顺序展示 */
  segments: string[];
}

export const VOICE_QUICK_PRESETS: VoiceQuickPreset[] = [
  {
    id: "ctrl-win",
    presetId: "wechat-hold",
    segments: ["左 Ctrl", "左 Win"],
  },
  {
    id: "win-alt",
    presetId: "qianwen-win-alt",
    segments: ["左 Win", "左 Alt"],
  },
  {
    id: "ralt-space",
    presetId: "doubao-hands-free",
    segments: ["右 Alt", "Space"],
  },
  {
    id: "ralt",
    presetId: "qianwen-hold",
    segments: ["右 Alt"],
  },
];

const PRESET_ORDER: ImePresetId[] = IME_TABS.flatMap((tab) => tab.presetIds);

export function listImePresets(): ImePresetDefinition[] {
  return PRESET_ORDER.map((id) => IME_PRESETS[id]);
}

export function listImeTabs(): ImeTabDefinition[] {
  return IME_TABS;
}

export function getPresetsForTab(tabId: ImeTabId): ImePresetDefinition[] {
  const tab = IME_TABS.find((t) => t.id === tabId);
  if (!tab) return [];
  return tab.presetIds.map((id) => IME_PRESETS[id]);
}

function shortcutAction(shortcutVks: readonly number[]): KeyAction {
  if (shortcutVks.length === 1) {
    return { type: "SingleKey", value: shortcutVks[0] };
  }
  return { type: "ComboKey", value: [...shortcutVks] };
}

/** Build a complete voice-key configuration for an input-method preset. */
export function applyImePresetConfig(
  config: DeviceConfig,
  presetId: ImePresetId,
): DeviceConfig {
  const definition = IME_PRESETS[presetId];
  const action = shortcutAction(definition.shortcutVks);

  return {
    ...config,
    button_bindings: {
      ...config.button_bindings,
      mic: action,
      voice: action,
    },
    voice_hotkey: [...definition.voiceHotkey],
    voice_shortcut_enabled: true,
    trigger_mode: definition.triggerMode,
    voice_release_behavior: definition.voiceReleaseBehavior,
  };
}
