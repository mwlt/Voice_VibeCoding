#!/usr/bin/env python3
"""Check whether reopening Mic Device after silence restores audio."""

import time

import numpy as np
import sounddevice as sd


def find_mic():
    for i, d in enumerate(sd.query_devices()):
        if int(d.get("max_input_channels", 0) or 0) > 0 and "mic device" in str(d.get("name", "")).lower():
            return i, int(d.get("default_samplerate") or 48000)
    raise SystemExit("Mic Device not found")


def rms_block(idx, sr, seconds=0.5):
    n = max(1, int(sr * seconds))
    data = sd.rec(n, samplerate=sr, channels=1, dtype="float32", device=idx)
    sd.wait()
    arr = np.asarray(data, dtype=np.float32)
    return float(np.sqrt(np.mean(np.square(arr)))), float(np.max(np.abs(arr)))


def main():
    idx, sr = find_mic()
    print(f"device={idx} sr={sr}")
    print("Phase1: continuous 20s")
    t0 = time.time()
    while time.time() - t0 < 20:
        rms, peak = rms_block(idx, sr)
        print(f"{time.time()-t0:5.1f}s rms={rms:.6f} peak={peak:.6f}")
    print("Phase2: close 1s then reopen blocks")
    time.sleep(1.0)
    # force re-query / new stream each call already; try abort
    sd.stop()
    time.sleep(0.5)
    for i in range(20):
        rms, peak = rms_block(idx, sr)
        print(f"reopen[{i}] rms={rms:.6f} peak={peak:.6f}")
    print("Phase3: reopen after 3s idle")
    time.sleep(3.0)
    for i in range(10):
        rms, peak = rms_block(idx, sr)
        print(f"idle-reopen[{i}] rms={rms:.6f} peak={peak:.6f}")


if __name__ == "__main__":
    main()
