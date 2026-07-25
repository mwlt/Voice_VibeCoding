//! Phase-1 feedback loop: 一次 tap 后映射键不得仍处于按下（否则记事本会连打）。
//!
//! 运行: cargo run -p remote-bridge-hub --example diag_voice_tap
//! 退出码 1 = RED（复现到「松不开」）

use std::time::Duration;

fn key_down(vk: i32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = vk;
        false
    }
}

fn force_release_sendinput(vks: &[u16]) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
            KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
        };
        let extended = |vk: u16| matches!(vk, 0x5B | 0x5C | 0xA3 | 0xA5);
        for &vk in vks.iter().rev() {
            let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
            let mut flags = KEYEVENTF_KEYUP;
            if extended(vk) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            let inputs = [INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }];
            unsafe {
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            }
        }
        let _ = remote_bridge_hub_lib::bridges::xiaomi::hid_injector::release(vks);
    }
    #[cfg(not(windows))]
    {
        let _ = vks;
    }
}

fn check_case(name: &str, vks: &[u16], hold_ms: u64) -> bool {
    println!("--- case {name} vks={vks:?} hold_ms={hold_ms} ---");
    for &vk in vks {
        if key_down(vk as i32) {
            println!("SKIP: VK 0x{vk:02X} already down before tap (physical keyboard?)");
            return true;
        }
    }

    remote_bridge_hub_lib::bridges::xiaomi::key_mapping::tap_vks(vks, hold_ms);
    std::thread::sleep(Duration::from_millis(80));

    let mut ok = true;
    for &vk in vks {
        if key_down(vk as i32) {
            println!("RED: VK 0x{vk:02X} STILL DOWN after tap_vks — Notepad would auto-repeat");
            ok = false;
        } else {
            println!("ok: VK 0x{vk:02X} released");
        }
    }
    // Notepad F5 会插入日期时间；若误发 F5 且卡住，会刷屏日期
    if key_down(0x74) {
        println!("RED: VK_F5 still down (Notepad inserts datetime on F5)");
        ok = false;
    }

    if !ok {
        force_release_sendinput(vks);
        let _ = remote_bridge_hub_lib::bridges::xiaomi::hid_injector::release(&[0x74]);
        std::thread::sleep(Duration::from_millis(30));
    }
    ok
}

fn main() {
    env_logger::init();
    println!(
        "WinUHid available={}",
        remote_bridge_hub_lib::bridges::xiaomi::hid_injector::is_available()
    );

    let cases: &[(&str, &[u16], u64)] = &[
        ("letter_A", &[0x41], 70),
        ("letter_B", &[0x42], 70),
        ("ctrl_lwin", &[0xA2, 0x5B], 120),
        ("right_alt", &[0xA5], 70),
    ];

    let mut all_ok = true;
    for (name, vks, hold) in cases {
        if !check_case(name, vks, *hold) {
            all_ok = false;
        }
    }

    // 连续 20 次 tap：若偶发松不开或累计卡住，必红
    println!("--- burst letter_A x20 ---");
    for i in 0..20 {
        remote_bridge_hub_lib::bridges::xiaomi::key_mapping::tap_vks(&[0x41], 70);
        std::thread::sleep(Duration::from_millis(20));
        if key_down(0x41) {
            println!("RED: burst#{i} VK_A stuck");
            all_ok = false;
            force_release_sendinput(&[0x41]);
            break;
        }
    }
    if all_ok {
        println!("burst letter_A x20: keys released each time");
    }

    if all_ok {
        println!("GREEN: all tap cases released cleanly");
        std::process::exit(0);
    } else {
        println!("RED: tap left key(s) down — matches continuous Notepad input");
        std::process::exit(1);
    }
}
