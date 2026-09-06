#!/usr/bin/env python3
"""Probe T1 Mic Device RMS for ~30s to see when USB audio goes silent."""

import sys
import time


def main() -> int:
    try:
        import numpy as np
        import sounddevice as sd
    except Exception as exc:
        print(f"NO_SOUNDDEVICE {exc}")
        print("pip install sounddevice numpy")
        return 2

    devices = sd.query_devices()
    mic_idx = None
    for i, d in enumerate(devices):
        name = str(d.get("name", ""))
        if int(d.get("max_input_channels", 0) or 0) > 0 and "mic device" in name.lower():
            mic_idx = i
            print(f"FOUND idx={i} name={name} rate={d.get('default_samplerate')}")
            break
    if mic_idx is None:
        print("DEVICES:")
        for i, d in enumerate(devices):
            if int(d.get("max_input_channels", 0) or 0) > 0:
                print(i, d.get("name"))
        return 1

    sr = int(devices[mic_idx].get("default_samplerate") or 48000)
    block = max(1, int(sr * 0.5))
    print(f"RECORD sr={sr} block=0.5s for 30s — please speak into the remote")
    t0 = time.time()
    for _ in range(60):
        data = sd.rec(block, samplerate=sr, channels=1, dtype="float32", device=mic_idx)
        sd.wait()
        arr = np.asarray(data, dtype=np.float32)
        rms = float(np.sqrt(np.mean(np.square(arr))))
        peak = float(np.max(np.abs(arr)))
        elapsed = time.time() - t0
        flag = "SILENT" if rms < 1e-4 and peak < 1e-3 else "ok"
        print(f"{elapsed:5.1f}s  rms={rms:.6f}  peak={peak:.6f}  {flag}")
    print("DONE")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
