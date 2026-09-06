import json
from pathlib import Path

root = Path(
    r"C:/Users/mwlt_/.cursor/projects/d-00vscode-workspace-remote-bridge-hub-master-xiaomi-remote-2-pro-rust-deepseek/agent-transcripts"
)
for p in root.rglob("*.jsonl"):
    with p.open(encoding="utf-8", errors="ignore") as f:
        for i, line in enumerate(f, 1):
            if "runtime.rs" not in line or "Write" not in line:
                continue
            if "bridges" not in line or "t1" not in line:
                continue
            try:
                o = json.loads(line)
            except Exception:
                continue
            for part in o.get("message", {}).get("content", []):
                if part.get("name") != "Write":
                    continue
                path = str(part.get("input", {}).get("path", "")).replace("\\", "/")
                if not path.endswith("bridges/t1/runtime.rs"):
                    continue
                c = part["input"].get("contents", "")
                print(p.name, i, len(c), "TriggerMode" in c, "VOICE_LATCH" in c)
                out = Path(
                    r"d:/00vscode_workspace/remote-bridge-hub-master/xiaomi_remote_2_pro_rust_deepseek/_restore_snippets"
                ) / f"runtime.write.{p.stem[:8]}.{i}.rs"
                out.write_text(c, encoding="utf-8")
