#!/usr/bin/env python3
"""Concurrent Mic Device: WASAPI hold + MME probe (IME-like)."""
import time
import numpy as np
import sounddevice as sd

devs = sd.query_devices()
wasapi = mme = None
for i, d in enumerate(devs):
    name = str(d.get("name", ""))
    host = str(sd.query_hostapis(d["hostapi"]).get("name", ""))
    if "mic device" in name.lower() and int(d.get("max_input_channels", 0) or 0) > 0:
        print(i, host, name)
        if "WASAPI" in host:
            wasapi = i
        if "MME" in host:
            mme = i

assert wasapi is not None and mme is not None
sr = int(devs[wasapi].get("default_samplerate") or 48000)
frames = {"n": 0, "rms": 0.0}


def cb(indata, frames_count, time_info, status):
    arr = np.asarray(indata, dtype=np.float32)
    frames["n"] += 1
    frames["rms"] = float(np.sqrt(np.mean(np.square(arr))))


stream = sd.InputStream(
    device=wasapi, channels=1, samplerate=sr, callback=cb, dtype="float32"
)
stream.start()
time.sleep(0.4)
print("keepalive-like open ok frames", frames["n"], "rms", round(frames["rms"], 6))
print("speak for 4s while second client records…")
t0 = time.time()
for _ in range(8):
    data = sd.rec(int(44100 * 0.5), samplerate=44100, channels=1, dtype="float32", device=mme)
    sd.wait()
    arr = np.asarray(data, dtype=np.float32)
    rms = float(np.sqrt(np.mean(np.square(arr))))
    peak = float(np.max(np.abs(arr)))
    flag = "SILENT" if rms < 1e-4 else "ok"
    print(
        f"{time.time()-t0:4.1f}s mme_rms={rms:.6f} peak={peak:.6f} "
        f"hold_rms={frames['rms']:.6f} {flag}"
    )
stream.stop()
stream.close()
print("DONE")
