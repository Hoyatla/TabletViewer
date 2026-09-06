//! UI Automation (UIA) — pilot Windows apps via the accessibility framework.
//!
//! ## Implementation status (2026-09-06)
//!
//! The first iteration of this module was wired against the COM UIA API
//! exposed by the `windows` crate, but the actual type layouts in
//! `windows = 0.58` differ from the public docs in ways that need a
//! reference build to settle (VARIANT field access, `IUnknown` → pattern
//! downcast, `IUIAutomationElementArray::Length`/`GetElement`).
//!
//! Rather than ship a half-broken build, this file ships a **stub**:
//! every function exists with the right signature, the module compiles,
//! and calls fail with a clear "not yet implemented on this build" error.
//! The 5 HTTP routes still authenticate and respond — they just return
//! `ok: false` with a descriptive error until the UIA logic is filled in.
//!
//! TODO: replace these bodies with real COM UIA calls once we can
//! iterate against a reference build of `windows = 0.58`.

#![cfg(windows)]

use serde_json::Value;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};

/// Renvoie l'arbre UIA de la fenêtre au premier plan en JSON.
///
/// Status: STUB. Returns a clear error to the caller.
pub fn dump_active_window() -> Result<Value, String> {
    Err("uia::dump_active_window: not yet implemented (COM UIA bindings pending for windows 0.58). The route is wired and authenticated; only the inner UIA walk is missing.".into())
}

/// Invoque un contrôle identifié par `automation_id`. STUB.
pub fn invoke_by_id(_automation_id: &str) -> Result<(), String> {
    Err("uia::invoke_by_id: not yet implemented (UIA InvokePattern downcast pending).".into())
}

/// Écrit du texte dans un contrôle Edit identifié par `automation_id`. STUB.
pub fn set_text_by_id(_automation_id: &str, _value: &str) -> Result<(), String> {
    Err("uia::set_text_by_id: not yet implemented (UIA ValuePattern downcast pending).".into())
}

/// Sélectionne un item dans ComboBox/ListBox identifié par `automation_id`. STUB.
pub fn select_by_id(_automation_id: &str, _value: &str) -> Result<(), String> {
    Err("uia::select_by_id: not yet implemented (UIA ExpandCollapse + SelectionItem patterns pending).".into())
}

/// Envoie une combinaison de touches. THIS ONE IS REAL — SendInput is well-supported.
pub fn press_keys(keys: &str) -> Result<(), String> {
    let parts: Vec<&str> = keys.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("empty keys".into());
    }

    let main_vk = parse_key(parts[parts.len() - 1])
        .ok_or_else(|| format!("unknown key: '{}'", parts[parts.len() - 1]))?;
    let modifiers: Vec<VIRTUAL_KEY> = parts[..parts.len() - 1]
        .iter()
        .map(|m| parse_modifier(m))
        .collect::<Result<_, _>>()?;

    for &vk in &modifiers {
        send_key(vk, false);
    }
    send_key(main_vk, false);
    send_key(main_vk, true);
    for &vk in modifiers.iter().rev() {
        send_key(vk, true);
    }
    Ok(())
}

fn send_key(vk: VIRTUAL_KEY, key_up: bool) {
    let flags = if key_up { KEYEVENTF_KEYUP } else { Default::default() };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let _ = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn parse_modifier(s: &str) -> Result<VIRTUAL_KEY, String> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => VIRTUAL_KEY(0x11),
        "shift" => VIRTUAL_KEY(0x10),
        "alt" | "menu" => VIRTUAL_KEY(0x12),
        "win" | "meta" | "super" => VIRTUAL_KEY(0x5B),
        other => return Err(format!("unknown modifier: {other}")),
    })
}

fn parse_key(s: &str) -> Option<VIRTUAL_KEY> {
    let k = match s.to_ascii_lowercase().as_str() {
        "return" | "enter" | "cr" => 0x0D,
        "escape" | "esc" => 0x1B,
        "tab" => 0x09,
        "space" | "spc" => 0x20,
        "backspace" | "bs" => 0x08,
        "delete" | "del" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "f1" => 0x70, "f2" => 0x71, "f3" => 0x72, "f4" => 0x73,
        "f5" => 0x74, "f6" => 0x75, "f7" => 0x76, "f8" => 0x77,
        "f9" => 0x78, "f10" => 0x79, "f11" => 0x7A, "f12" => 0x7B,
        other if other.len() == 1 => {
            let c = other.chars().next().unwrap();
            let upper = c.to_ascii_uppercase() as u16;
            if upper <= 0x7F && (upper as u8 as char).is_ascii_alphanumeric() {
                upper
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(VIRTUAL_KEY(k))
}
