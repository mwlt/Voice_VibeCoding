# Research: Long-term clean swallow of AC Search / Browser Search (Windows 10/11)

**Date:** 2026-09-06  
**Scope:** Xiaomi / T1-like BLE HOGP remotes that emit Consumer Page **AC Search** (`0x0C` / `0x0221`) which Windows surfaces as **VK_BROWSER_SEARCH (`0xAA`)** and/or **APPCOMMAND_BROWSER_SEARCH (`5`)**, opening SearchHost / Start UI.  
**Product constraints:** Do not break the real PC keyboard (no ubiquitous-key suppress); do not uninstall SearchHost; do not rely on Keyboard Filter Service (Embedded/IoT lockdown); no global `RIDEV_NOLEGACY`; Esc only when Search/Start is truly foreground.

---

## Verdict (recommended architecture)

**Pick one stack: “Per-device HID strip (optional driver) + userspace dual-claim + demoted mop-up.”**

| Layer | Role | Mechanism |
|-------|------|-----------|
| **L0 (optional, true prevention)** | Strip AC Search **before** the OS consumer client acts | KMDF upper filter on **only** the remote’s Consumer Control TLC; zero Usage `0x0221` / report `02-21-02` in read completions |
| **L1 (primary userspace prevention)** | Claim both VK and shell APPCOMMAND paths | Keep `RegisterHotKey(VK_BROWSER_SEARCH)` + permanent LL gate for `0xAA`; **add** global `WH_SHELL` / `HSHELL_APPCOMMAND` return **nonzero** for `APPCOMMAND_BROWSER_SEARCH` while the T1 gate is on |
| **L2 (early arm / observe)** | Win the race before LL/hotkey | Keep Consumer Raw Input `0x0C/0x01` + `RIDEV_INPUTSINK`; on T1 `02-21-02` arm gate + kick mop-up **async** (observe only — cannot consume Consumer TLC) |
| **L3 (secondary mop-up)** | Catch residual Search UI | Keep async SearchHost dismiss + FG WinEvent watchdog; Esc **only** when Search/Start is foreground |

**Do not** treat dismiss/Esc as the primary design. Reactive mop-up will always lose some frames to SearchHost. Long-term “swallow clean” means **L0 when installed**, and **L1+L2 always**, with L3 as safety net.

---

## 1. Taxonomy of Windows paths that open Search from AC Search

Ordered by **earliness** (hardware → shell UI). Paths can run in parallel; that is why userspace alone races.

### P0 — Transport / HID report (earliest)

BLE HOGP (or USB) delivers a Consumer Control report containing Usage **AC Search = `0x0221`** on Usage Page **Consumer (`0x0C`)**, typically under TLC Consumer Control (`0x0C` / `0x01`). Report bytes often look like `02-21-02` (report-id / usage / state — device-dependent packing).

- Spec: USB-IF HID Usage Tables — Consumer Page, **AC Search `0x0221`** (“Search for documents (URLs, files, web pages, etc).”).  
  - https://usb.org/document-library/hid-usage-tables-17  
  - https://www.usb.org/sites/default/files/hut1_7.pdf  
- Windows maps the same usage for Search buttons in ACPI HID button descriptors:  
  https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/acpi-button-device

**Block at this layer:** rewrite/strip the usage in a **per-device** HID filter (L0). Userspace Raw Input **sees** the report but does **not** stop other clients.

### P1 — HIDCLASS collection + system consumer client

`hidclass.sys` exposes the Consumer Control TLC. Windows documents Consumer controls (`0x0C` / `0x01`) as **Shared** access (unlike keyboard/mouse Exclusive). Multiple clients (system consumer path, Raw Input, apps) can observe the same stream.

- https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/hid-architecture  
- Opening collections: https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/opening-hid-collections  

Keyboard path for comparison (not AC Search’s primary TLC): `kbdhid.sys` maps Keyboard Page usages → scan codes → `kbdclass`. Consumer Search is **not** that mapper.

- https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/keyboard-and-mouse-hid-client-drivers  

**Block:** L0 filter above HIDCLASS on that PDO. Exclusive `CreateFile` on Consumer TLC does **not** give exclusive ownership of system handling (Shared mode).

### P2 — Raw Input `WM_INPUT` (observe / early arm only)

Apps register TLC `0x0C` / `0x01` with `RegisterRawInputDevices`. With `RIDEV_INPUTSINK`, background windows receive reports. This is **parallel notification**, not a sink that removes the report from the OS consumer path.

- Overview: https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input  
- Flags: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice  

**Critical:** `RIDEV_NOLEGACY` “prevents … legacy messages” and is documented as **only for mouse and keyboard**. It does **not** legally/semantically turn off Consumer Control → Search. Applying keyboard `RIDEV_NOLEGACY` is also **per TLC class**, not per device — it would affect **all** keyboards.

**Block:** cannot fully block Search here. Use only for earliest userspace **arm**.

### P3 — Virtual-key injection path (`VK_BROWSER_SEARCH` = `0xAA`)

Windows surfaces Browser Search as virtual-key `0xAA`:

- https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes  

That VK enters the keyboard input path (local driver or synthesis). Before it is posted to a thread queue:

### P4 — `WH_KEYBOARD_LL` (low-level keyboard hook)

Called when a keyboard input event is **about to be posted** to a thread input queue. Returning a non-zero value without calling `CallNextHookEx` eats the event.

- https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc  
- Hooks overview: https://learn.microsoft.com/en-us/windows/win32/winmsg/about-hooks  

**Limits:** no stable per-device identity; HOGP often sets `LLKHF_INJECTED` (our codebase already must not treat that as “our inject”); slow callbacks risk silent unhook; does **not** see pure APPCOMMAND that never became a VK in this process’s view.

### P5 — `RegisterHotKey(VK_BROWSER_SEARCH)`

System-wide hotkey claim: on match, OS posts `WM_HOTKEY` to the registering thread/window instead of normal key delivery.

- https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey  
- https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-hotkey  

**Reliability:** high for the **VK** path when registration succeeds. Fails if another app owns the same hotkey. Does **not** by itself document coverage of every APPCOMMAND/shell path, and races still exist if Search is opened by a parallel consumer/shell handler.

### P6 — Focused window `WM_KEY*` → `DefWindowProc` → `WM_APPCOMMAND`

`DefWindowProc` generates `WM_APPCOMMAND` when the user types an **application command key** (among other cases). `APPCOMMAND_BROWSER_SEARCH = 5` means “Open search.”

- https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-appcommand  

If child/top-level windows pass to `DefWindowProc`, the message bubbles; unhandled top-level ends in a **shell hook**.

### P7 — `WH_SHELL` / `HSHELL_APPCOMMAND` (shell default handler)

When an application did **not** handle `WM_APPCOMMAND`, the system notifies shell hooks with `HSHELL_APPCOMMAND`. **If the shell procedure handles it, it must return nonzero and must not call `CallNextHookEx`.** That is the documented way to stop the shell from acting on the appcommand.

- https://learn.microsoft.com/en-us/windows/win32/winmsg/shellproc  
- Legacy mirror: https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ms644991(v=vs.85)  

`SetWindowsHookEx` notes that `WH_SHELL` (like LL hooks) can run on the **installing thread**, so a dedicated message-loop thread (same pattern as our hotkey/LL threads) is viable without a classic inject-DLL design for this specific codepath.

- https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw  

**This is the primary userspace gap vs today’s stack** (hotkey + LL + dismiss): APPCOMMAND that reaches the shell after focus-window ignore.

### P8 — Explorer / SearchHost UI (latest / reactive)

Shell opens Windows Search / Start flyout (`SearchHost.exe`, `SearchApp.exe`, `SearchUI.exe`, `StartMenuExperienceHost.exe`, …). Closing or Esc’ing is **after** the UI appeared — mop-up, not prevention.

Undocumented/community `HKLM\...\Explorer\AppKey\5` remaps are **not** a supported primary API for Win10/11 SearchHost and must not be the product foundation.

---

## 2. Path × block method matrix

| Path | What can block | Reliability | Side effects |
|------|----------------|-------------|--------------|
| **P0 report** | Per-device HID upper filter strips `0x0221` | **Best** if filter loaded & bound | Install/signing; must **not** bind to PC keyboards; must leave volume/other consumer usages intact unless remapped by app |
| **P1 Shared consumer** | Same as P0; exclusive open **does not** steal Shared consumer from OS | Exclusive open: **poor** for prevention | Breaking remote volume if you accidentally exclusive-open and fail to re-emit |
| **P2 Raw Input** | Observe + arm only | Excellent as **sensor**, useless as **sink** for Consumer | None if INPUTSINK-only; **catastrophic** if misused as global keyboard NOLEGACY |
| **P4 LL hook `0xAA`** | Return non-zero for gate VK | High for VK path; misses pure APPCOMMAND; race vs shell | Must never suppress space/arrows/letters; never replay `0xAA` on “foreign” |
| **P5 RegisterHotKey `0xAA`** | System claims VK | High when registration wins | Steals Browser Search from **all** keyboards (rare key; acceptable under “no ubiquitous keys”) |
| **P6/P7 APPCOMMAND** | `WH_SHELL` return nonzero for cmd=5 while gate on; or L0 so APPCOMMAND never born | Medium–high; only runs if FG app did not handle `WM_APPCOMMAND` | If always-on without gate, would block PC keyboard Search APPCOMMAND too — **gate with T1 session** |
| **P8 SearchHost** | Dismiss / Esc / WinEvent | Reactive; flash possible | Esc into wrong app if FG detection wrong — keep existing FG guards |

---

## 3. Ranked long-term architectures (then the one we pick)

1. **★ Recommended: L0 optional HID strip + L1 dual-claim (HotKey+LL+Shell APPCOMMAND) + L2 Raw arm + L3 mop-up**  
   Matches a userspace product with **optional** driver (same commercial shape as WinUHid): zero-flash when filter present; still strong without it; PC keyboard untouched for ubiquitous keys.

2. **Userspace-only dual-claim + mop-up (no driver)**  
   Same as recommended without L0. Accept residual flash under load; still far better than dismiss-only once `HSHELL_APPCOMMAND` is claimed.

3. **Firmware / GATT: never expose AC Search to HOGP; voice only via ATVV `START_SEARCH`**  
   Ideal if we controlled firmware. Xiaomi/T1 HOGP often still emits `02-21-02` in parallel with ATVV — cannot assume firmware fix for all SKUs.

4. **Global keyboard `RIDEV_NOLEGACY` / wholesale media remap** — reject (below).

5. **Keyboard Filter Service / debloat SearchHost** — reject (below).

---

## 4. Explicit rejects (wrong for this product)

| Approach | Why reject |
|----------|------------|
| **Uninstall / debloat SearchHost / SearchUI** | Breaks OS Search; violates product constraint; Update restoration; not a swallow strategy |
| **Keyboard Filter Service (`Client-KeyboardFilter` / Embedded KeyboardFilter)** | Documented for **device lockdown** (IoT Enterprise / Enterprise lockdown), not consumer remotes; enables via DISM feature + reboot; Safe Mode bypass; can fight OEM keyboard filters; wrong SKU/UX for a bridge app. https://learn.microsoft.com/en-us/windows/configuration/keyboard-filter/ |
| **Global `RIDEV_NOLEGACY` on keyboard TLC** | Applies to **all** keyboards of that usage; kills legacy `WM_KEY*` for the PC keyboard; docs limit NOLEGACY to mouse/keyboard anyway — **not** a Consumer Search off-switch. https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice |
| **LL-suppress ubiquitous keys** (Space/arrows/letters) to “catch” Search | Hard product constraint; LL cannot distinguish remote vs PC keyboard reliably |
| **Esc always / blind SendInput Esc** | Focus theft into WeChat/IME; existing FG rules exist for a reason |
| **Explorer `AppKey\5` registry as primary** | Undocumented for modern SearchHost; fragile across builds; admin; global side effect |
| **Disable `HidServ` / kill consumer service** | Breaks volume/media system-wide; unsupported |
| **Rely only on dismiss SearchHost** | Reactive; known multi-path race; flash remains |

---

## 5. Implementation plan for this codebase

### Keep (harden, do not remove)

| Module | Keep |
|--------|------|
| **`native_suppress`** | Permanent gate for `0xAA` / `0xAC` / `0x5D`; never `allow_pass` hole for `0xAA`; never foreign-replay `0xAA`; `RegisterHotKey` for `0xAA`/`0xAC`/`0x5D`; async dismiss + FG watchdog with `should_tap_escape_for_shell` / process+class allowlists |
| **`consumer_raw_input`** | `0x0C/0x01` + `RIDEV_INPUTSINK`; T1 device filter; on `02-21-02` → `on_ac_search_hid_seen()` **before** mapping work |
| **`ble_keys` / `ble_session`** | Treat HID `hid:02-21-02` as voice **side-effect**; ATVV `START_SEARCH` as semantic voice; keep early swallow calls |
| **`ble_voice`** | Dup window (`VOICE_DUP_EVENT_MS`) **before** slow dismiss; async dismiss; inject mapped chord only once |

### Change

| Module | Change |
|--------|--------|
| **`native_suppress`** | **Add Shell APPCOMMAND claim:** dedicated thread (or extend hotkey thread) installs `SetWindowsHookEx(WH_SHELL, …, 0)` while gate on; on `HSHELL_APPCOMMAND` if `GET_APPCOMMAND_LPARAM == 5` (`APPCOMMAND_BROWSER_SEARCH`) **and** gate enabled → return **nonzero**, no `CallNextHookEx`; log + arm mop-up. Unhook on gate off. |
| **`native_suppress`** | Treat L3 dismiss as **secondary**: still call from HID/LL/hotkey/shell, but success metrics should prefer “Search never FG”, not “dismissed within N ms”. |
| **`consumer_raw_input`** | Keep observe-only; document in-module that Consumer Raw Input **cannot** NOLEGACY-swallow Search; optionally tag events with device path for future filter pairing. |
| **`ble_voice` / `ble_keys`** | Ensure **all** voice entry points call the same `on_ac_search_hid_seen` / arm helper **before** any sync UI work; never open a path that injects or replays `0xAA`. |
| **Probe script** | `scripts/probe_t1_appcommand_search.ps1` currently shifts **`6`** as “BROWSER_SEARCH”; Win32 **`APPCOMMAND_BROWSER_SEARCH = 5`** (6 is Favorites). Fix probe before relying on it. |

### New component

| Component | Purpose |
|-----------|---------|
| **`t1_hid_filter` (optional KMDF upper filter)** | INF hardware-ID match for known T1/Xiaomi HOGP Consumer Control collections only; on read completion, clear AC Search usage / zero matching report fields; pass through all other usages. Ship like WinUHid: optional install, attestation/signing story, repair script. Sample starting point: KMDF HID filter (Firefly) — https://learn.microsoft.com/en-us/samples/microsoft/windows-driver-samples/kmdf-filter-driver-for-a-hid-device/ |
| **`native_suppress::shell_appcommand` (userspace)** | Thin submodule for WH_SHELL lifecycle + cmd=5 swallow; unit-test seams for “gate on → swallow search appcommand / gate off → forward”. |

### Layering contract (runtime)

```
HOGP report 02-21-02
    ├─[L0 filter]─► usage stripped ─► OS never opens Search ─► app still sees via ATVV / optional IOCTL
    └─[no filter]
          ├─ Raw Input (L2) ─ arm + async mop-up kick
          ├─ RegisterHotKey + LL (L1) ─ eat VK 0xAA
          ├─ WH_SHELL (L1) ─ eat APPCOMMAND_BROWSER_SEARCH
          └─ L3 ─ if Search/Start FG, Esc/dismiss
ATVV START_SEARCH ─► ble_voice (dedup) ─► mapped inject (never 0xAA)
```

---

## 6. Success criteria / verification probes

### Automated seams (extend existing `t1_browser_search_swallow` style)

1. Gate on → `should_suppress_native(0xAA)` true; ubiquitous VKs false.  
2. `allow_pass_vks([0xAA])` must **not** open a hole (already asserted).  
3. New: `shell_should_swallow_appcommand(cmd, gate_on)` true iff `cmd==5 && gate_on`.  
4. Voice dup: HID + START_SEARCH within `VOICE_DUP_EVENT_MS` → single inject.  
5. Watchdog: Esc only when FG hint is Search/Start; gap ≥ `FG_WATCHDOG_ESC_GAP_MS`.

### Live probes (manual / scripted on Win10 + Win11)

| Probe | Pass |
|-------|------|
| **A. Zero-flash voice** | Hold T1 voice 20×; SearchHost/Start **never** becomes foreground (Process Explorer / `GetForegroundWindow` poll ≤16 ms). With L0 installed: zero flashes. Userspace-only: ≤ rare flash under extreme CPU load, recovered &lt;100 ms without Esc into other apps. |
| **B. APPCOMMAND** | After fixing probe to cmd=**5**, synthesize `WM_APPCOMMAND` / exercise shell path with gate on → no Search FG. Gate off → Search may open (sanity). |
| **C. HotKey ownership** | Log `RegisterHotKey` success; if fail, degrade visibly (status) and rely on LL+Shell+L3. |
| **D. PC keyboard** | Typing, Space, arrows, Enter, letters unaffected while T1 connected. Physical Browser Search key (if any) may be claimed by hotkey — acceptable; document. |
| **E. Esc discipline** | Open WeChat/IME; press voice; Esc must **not** land in WeChat unless Search was actually FG. |
| **F. Filter isolation** | With L0 installed, non-T1 USB keyboard media/search keys still work; only matched Hardware IDs are filtered. |
| **G. Disconnect** | Stop BLE / unplug → gate off → hotkey+shell unregistered; PC Browser Search restored if previously claimed. |

### Observability

- Structured log lines: `hid_seen`, `ll_swallow`, `hotkey_claim`, `shell_appcommand_swallow`, `search_fg_escape`, `filter_strip` (if L0).  
- Counters: swallows by layer; `search_fg_transitions` must trend to **0** after L1 shell claim (+ L0).

---

## Source index (primary)

| Topic | URL |
|-------|-----|
| HID Usage Tables (USB-IF) | https://usb.org/document-library/hid-usage-tables-17 |
| ACPI / AC Search `0x221` | https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/acpi-button-device |
| HID architecture / Consumer Shared | https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/hid-architecture |
| Keyboard HID clients | https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/keyboard-and-mouse-hid-client-drivers |
| Opening HID collections | https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/opening-hid-collections |
| Raw Input overview | https://learn.microsoft.com/en-us/windows/win32/inputdev/about-raw-input |
| `RAWINPUTDEVICE` / `RIDEV_NOLEGACY` | https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-rawinputdevice |
| Virtual-Key Codes (`VK_BROWSER_SEARCH`) | https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes |
| `RegisterHotKey` | https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey |
| `WM_HOTKEY` | https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-hotkey |
| `WM_APPCOMMAND` / `APPCOMMAND_BROWSER_SEARCH` | https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-appcommand |
| `ShellProc` / `HSHELL_APPCOMMAND` | https://learn.microsoft.com/en-us/windows/win32/winmsg/shellproc |
| Low-level keyboard hook | https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc |
| Hooks / `SetWindowsHookEx` | https://learn.microsoft.com/en-us/windows/win32/winmsg/about-hooks · https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowshookexw |
| Keyboard Filter (reject) | https://learn.microsoft.com/en-us/windows/configuration/keyboard-filter/ |
| KMDF HID filter sample | https://learn.microsoft.com/en-us/samples/microsoft/windows-driver-samples/kmdf-filter-driver-for-a-hid-device/ |

---

## Bottom line

Userspace today (LL + `RegisterHotKey` + Raw arm + dismiss) correctly attacks the **VK** path and mops the **UI**, but **APPCOMMAND → shell** and **Shared Consumer HID** remain earlier/parallel leaks.  

**Long-term optimal for this product:** optional **per-device HID strip of `0x0221`**, plus userspace **dual-claim of `0xAA` and `HSHELL_APPCOMMAND(5)`**, with SearchHost dismiss demoted to a watchdog — never SearchHost removal, never Embedded Keyboard Filter, never global `RIDEV_NOLEGACY`.

---

## Implementation progress (据实标记，2026-09-06)

| 步骤 | 内容 | 状态 | 证据 |
|------|------|------|------|
| **1 L1 策略缝** | `shell_should_swallow_appcommand` / `appcommand_from_lparam` / `APPCOMMAND_BROWSER_SEARCH=5` | **完成** | `t1_browser_search_swallow` 含 `shell_appcommand_*`；全量缝测 **21 passed** |
| **1 L1 ShellHook** | `RegisterShellHookWindow` + wndproc 处理 `HSHELL_APPCOMMAND` → `on_shell_appcommand_seen` | **完成** | 日志 `T1 ShellHook window registered msg=0xC029`（17:23:13） |
| **1 L1 WH_SHELL** | `SetWindowsHookEx(WH_SHELL)` + `t1_shell_hook.dll` 硬拦 cmd=5 | **完成（据实）** | 缝测 `wh_shell_dll_can_arm_hook` ok；`scripts/probe_t1_wh_shell_hardblock.ps1` → **PASS** + shared map `gate=[1,enter,swallow,0]`（DefWindowProc→HSHELL_APPCOMMAND）。注意：直发 `Shell_TrayWnd` 的旧探针不走 WH_SHELL，仍可能 WARN |
| **2 探针** | `probe_t1_appcommand_search.ps1` 改为 cmd=**5**；新增 WH_SHELL 硬拦探针 | **完成** | 硬拦探针 PASS；tray/广播探针仍测 mop-up（路径不同） |
| **2 AA 探针** | `probe_t1_aa_swallow.ps1` | **完成（重试 PASS）** | 闸门就绪后 `PASS` + `LOG swallow=True` |
| **3 入口统一** | HID / START_SEARCH / HotKey / LL 语音均走 `on_ac_search_hid_seen` 或 `on_browser_search_ll_swallowed` | **完成** | `ble_keys` / `ble_session` / hotkey 已改；`consumer_raw_input` 模块注释标明不能 NOLEGACY 吞 |
| **4 可观测** | `shell_appcommand_swallow_count` / `hid_ac_search_seen_count` / `search_fg_escape_count` + 结构化日志；DLL map 计数 | **完成（计数器+日志）** | 未接前端面板（无现有 UI 位；后续可接） |
| **5 L0 切断** | 禁用 T1 BLE **Consumer Control** HID（无自签 `.sys`） | **已切换** | Secure Boot 友好；`install-t1-hid-filter.ps1` 仅 disable `UP:000C`；自签 KMDF 方案已放弃 |

### 已知缺口（勿标完成）

1. **L0 Consumer 禁用** 已落地：不装自签驱动、不改 BIOS；自动修复即 `pnputil` 禁用 T1 Consumer Control。精细 Usage 剥离需日后 Attestation 签名驱动。
2. 前端无「吞键层健康度」展示。
3. `--lib` 单测在本仓会 `STATUS_ENTRYPOINT_NOT_FOUND`（DLL）；以 `--test t1_browser_search_swallow` 为准（**21 passed**，含 WH_SHELL 装载缝）。
4. `npm run tauri:dev` / `tauri:build` 会先 `cargo build -p t1_shell_hook`，确保旁路 DLL 存在。
