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

export interface ImeSetupStep {
  text: string;
  /** Optional smaller note shown under this step (not numbered) */
  aside?: string;
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
  steps: ImeSetupStep[];
  /** Optional unnumbered tip (e.g. 快捷设置方法), shown separately in blue */
  quickTip?: string;
}

export function imeStepLines(steps: ImeSetupStep[]): string {
  return steps
    .map((s) => (s.aside ? `${s.text}\n${s.aside}` : s.text))
    .join("\n");
}

export interface ImeFaqSection {
  title: string;
  items: string[];
}

export const IME_FAQ = {
  warnTitle:
    "有些输入法占用了你要录的快捷键，本软件就录不进去。可先改输入法快捷键，或直接用「快速应用」。",
  sections: [
    {
      title: "怎么录快捷键",
      items: [
        "录之前，先把输入法里的语音快捷键临时关掉或改掉。",
        "也可以不录：点「快速应用」，本软件会自动写好。",
        "先录别的组合键，录完再把输入法改成一样的也行。",
      ],
    },
    {
      title: "虚拟键盘",
      items: [
        "微信 / 豆包 / 千问的组合键唤醒，需要状态栏显示「虚拟键盘 · 已就绪」。",
        "未就绪时点「修复虚拟键盘」安装驱动（需管理员确认）。",
        "按住遥控语音键 = 按住本软件映射的快捷键，松手 = 松开。",
        "微信例外：微信「按住说话」须设为「F5 + 本软件快捷键」（例：软件左 Ctrl+左 Win → 微信 F5+左 Ctrl+左 Win）。",
      ],
    },
    {
      title: "其它输入法",
      items: [
        "豆包 / 千问等：在「按键映射」里把语音键设成和输入法一样的组合键即可。",
        "微信请看「微信输入法」Tab：微信侧要多加一个 F5。",
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
    presetIds: ["doubao-hold"],
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
    tag: "软件Ctrl+Win · 微信F5+同键",
    shortcutVks: [0xa2, 0x5b],
    voiceHotkey: ["leftctrl", "leftwin"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：左 Ctrl + 左 Win（微信侧请设 F5 + 该组合）",
    logMessage: "设置建议：已快速应用微信按住说话（本软件：左 Ctrl + 左 Win；微信：F5 + 左 Ctrl + 左 Win）",
    steps: [
      {
        text: "先在本软件设好语音快捷键：点「快速应用」可用推荐的「左 Ctrl + 左 Win」，或在按键映射里改成你想要的组合。",
      },
      {
        text: "打开微信输入法 → 设置 → 快捷键，把「按住说话」设为「F5 + 本软件里的那组快捷键」。",
        aside:
          "举例：本软件是「左 Ctrl + 左 Win」时，微信应设为「F5 + 左 Ctrl + 左 Win」（见下图）。",
      },
      {
        text: "麦克风选 CABLE Output。按住遥控语音键说话，松手结束并上屏。",
      },
    ],
    quickTip:
      "快捷设置方法：本软件中设置好映射按键之后，在微信语音「按住说话」点击进入右侧的输入框，点下遥控器的语音键即可。",
  },
  "doubao-hold": {
    id: "doubao-hold",
    title: "长按语音",
    tag: "右 Alt",
    shortcutVks: [0xa5],
    voiceHotkey: ["rightalt"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：右 Alt",
    logMessage: "设置建议：已快速应用豆包长按语音（右 Alt）",
    steps: [
      { text: "点「快速应用」，语音键会设为右 Alt。" },
      { text: "豆包设置里打开「长按模式」，快捷键也设为右 Alt（见下图）。" },
      { text: "麦克风选 CABLE Output，光标放在输入框。" },
      { text: "按住遥控语音键说话，松手结束并上屏。" },
    ],
  },
  "doubao-hands-free": {
    id: "doubao-hands-free",
    title: "免按语音",
    tag: "右 Alt + 空格",
    shortcutVks: [0xa5, 0x20],
    voiceHotkey: ["rightalt", "space"],
    triggerMode: "Toggle",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：右 Alt + 空格",
    logMessage: "设置建议：已快速应用豆包免按语音（右 Alt + 空格）",
    steps: [],
  },
  "qianwen-hold": {
    id: "qianwen-hold",
    title: "右 Alt",
    tag: "",
    shortcutVks: [0xa5],
    voiceHotkey: ["rightalt"],
    triggerMode: "Hold",
    voiceReleaseBehavior: "None",
    applyHint: "已应用：右 Alt",
    logMessage: "设置建议：已快速应用千问（右 Alt）",
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
    applyHint: "已应用：左 Win + 左 Alt",
    logMessage: "设置建议：已快速应用千问（左 Win + 左 Alt）",
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
    applyHint: "已应用：左 Ctrl + 左 Win",
    logMessage: "设置建议：已快速应用千问（左 Ctrl + 左 Win）",
    steps: [],
  },
};

/** 千问 Tab：共用说明 + 三个快速应用按钮 */
export const QIANWEN_GUIDE = {
  title: "按住说话",
  tag: "三选一",
  steps: [
    { text: "在千问里选一个按住语音快捷键，和下方「快速应用」按钮一致。" },
    { text: "点对应按钮，本软件会自动写好映射。" },
    { text: "麦克风选 CABLE Output。" },
    { text: "Ctrl+Win、Win+Alt 需要虚拟键盘就绪。" },
    { text: "按住遥控语音键说话，松手结束并上屏。" },
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
    segments: ["右 Alt", "空格 Space"],
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
