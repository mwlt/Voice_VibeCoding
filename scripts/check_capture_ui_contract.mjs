/**
 * Feedback loop for: 「按键录入没反应，但实际已录入」
 *
 * Contract: live progress (liveLabels) must not depend ONLY on Tauri events.
 * On some machines emit from a background thread is delayed/dropped while
 * capture_shortcut_poll still delivers the final chord — UI looks dead during
 * hold, then binding appears later (or only after remount).
 *
 * RED = progress updates only via listen("shortcut-capture-progress")
 * GREEN = poll path also refreshes liveLabels (or equivalent)
 *
 * Run: node scripts/check_capture_ui_contract.mjs
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const files = [
  "src/components/KeyMappingStage.vue",
  "src/components/KeyBindingEditor.vue",
];

let red = false;
const lines = [];

for (const rel of files) {
  const vuePath = path.join(root, rel);
  if (!fs.existsSync(vuePath)) {
    lines.push(`WARN: missing ${rel}`);
    continue;
  }
  const src = fs.readFileSync(vuePath, "utf8");

  const listenSetsLiveLabels =
    /listen\s*<[^>]*>\s*\(\s*["']shortcut-capture-progress["'][\s\S]*?liveLabels\.value\s*=/.test(
      src,
    );

  const pollBlock = src.match(/function startPolling\s*\(\)\s*\{[\s\S]*?\n\}/);
  const pollSrc = pollBlock ? pollBlock[0] : "";

  const pollSetsLiveLabels = /liveLabels\.value\s*=/.test(pollSrc);
  const hasProgressPollIpc =
    /capture_shortcut_progress/.test(src) ||
    /progressLabels|progress_labels|\.progress\b|"progress"/.test(pollSrc);

  if (!listenSetsLiveLabels) {
    lines.push(`WARN [${rel}]: no listen→liveLabels path found`);
  }

  if (listenSetsLiveLabels && !pollSetsLiveLabels && !hasProgressPollIpc) {
    red = true;
    lines.push(
      `RED [${rel}]: liveLabels only from shortcut-capture-progress; startPolling does not refresh progress.`,
    );
  } else if (pollSetsLiveLabels || hasProgressPollIpc) {
    lines.push(`GREEN [${rel}]: poll path refreshes live feedback.`);
  }

  const onCaptured = src.match(/async function onCaptured[\s\S]*?\n\}/);
  const onCapturedSrc = onCaptured ? onCaptured[0] : "";
  if (
    onCapturedSrc &&
    /capturing\.value\s*=\s*false/.test(onCapturedSrc) &&
    /await\s+.*save/.test(
      onCapturedSrc.split("capturing.value = false")[0] || "",
    )
  ) {
    red = true;
    lines.push(
      `RED [${rel}]: capturing cleared only after await save — slow disks look like 'no reaction'.`,
    );
  }
}

// Backend: poll must expose progress labels (not only pending result)
const rsPath = path.join(
  root,
  "src-tauri/src/bridges/shared/shortcut_capture.rs",
);
const rs = fs.readFileSync(rsPath, "utf8");
if (!/fn poll_snapshot/.test(rs) || !/peek_progress/.test(rs)) {
  red = true;
  lines.push(
    "RED [shortcut_capture.rs]: missing poll_snapshot/peek_progress — progress not readable via IPC.",
  );
} else {
  lines.push("GREEN [shortcut_capture.rs]: poll_snapshot exposes progress.");
}

const cmdPath = path.join(root, "src-tauri/src/ipc/commands.rs");
const cmd = fs.readFileSync(cmdPath, "utf8");
if (!/ShortcutPollSnapshot/.test(cmd)) {
  red = true;
  lines.push(
    "RED [commands.rs]: capture_shortcut_poll does not return ShortcutPollSnapshot.",
  );
} else {
  lines.push("GREEN [commands.rs]: poll returns ShortcutPollSnapshot.");
}

console.log(lines.join("\n"));
process.exit(red ? 1 : 0);
