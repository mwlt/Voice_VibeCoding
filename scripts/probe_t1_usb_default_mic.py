#!/usr/bin/env python3
"""Red/green: default capture must be Mic Device for T1 USB IME path."""
import subprocess
import sys
from pathlib import Path

import sounddevice as sd

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "src-tauri" / "assets" / "xiaomi" / "configure-xiaomi-audio.ps1"


def default_input_name() -> str:
    return str(sd.query_devices(kind="input").get("name", ""))


def main() -> int:
    before = default_input_name()
    print(f"BEFORE default_input={before}")

    # Force CABLE default first (repro the bug state) if available
    cable = ROOT / "src-tauri" / "assets" / "xiaomi" / "configure-xiaomi-audio.ps1"
    subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(cable),
            "-Mode",
            "EnsureMic",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    mid = default_input_name()
    print(f"AFTER EnsureMic default_input={mid}")
    if "cable" not in mid.lower():
        print("SKIP: EnsureMic did not set CABLE (VB-CABLE missing?)")
        # still test EnsureUsbMic
    else:
        assert "cable" in mid.lower(), mid

    r = subprocess.run(
        [
            "powershell",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(SCRIPT),
            "-Mode",
            "EnsureUsbMic",
        ],
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    print(r.stdout)
    if r.returncode != 0:
        print(r.stderr)
        return 2

    after = default_input_name()
    print(f"AFTER EnsureUsbMic default_input={after}")
    if "mic device" not in after.lower():
        print("FAIL: default capture is not Mic Device")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
