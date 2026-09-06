//! UI Automation (UIA) — pilot Windows apps via the accessibility framework.
//!
//! Implementation note: this module uses the COM UIA API exposed by the
//! `windows` crate (v0.58). Wire-up:
//!   1. `CoInitializeEx` (apartment-threaded, idempotent)
//!   2. `CoCreateInstance(CLSID_CUIAutomation, ...)` to get `IUIAutomation`
//!   3. Walk the tree with `IUIAutomationTreeWalker` and `FindAll` + a true
//!      condition (capped at MAX_DEPTH and MAX_ELEMENTS to keep dumps small)
//!   4. For typed actions, cast the pattern `IUnknown` to the right interface
//!      (e.g. `IUIAutomationInvokePattern`) using the type's IID via
//!      `windows_core::Interface::cast`.

#![cfg(windows)]

use serde_json::{json, Value};
use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationElementArray,
    IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationValuePattern, ExpandCollapseState_Collapsed,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId,
};

// Bounded walk to keep dumps reasonable on apps like Visual Studio.
const MAX_DEPTH: usize = 5;
const MAX_ELEMENTS: usize = 100;

// ---------------------------------------------------------------------------
// COM init
// ---------------------------------------------------------------------------

/// One-time COM init for the current process. Idempotent.
fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

fn get_automation() -> Result<IUIAutomation, String> {
    init_com();
    // CLSID_CUIAutomation = {ff48dba4-60ef-4201-aa87-54103eef594e}
    let clsid = windows::core::GUID::from_u128(0xff48dba4_60ef_4201_aa87_54103eef594e);
    unsafe {
        CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("CoCreateInstance(CUIAutomation): {e}"))
    }
}

fn proc_name_for_pid(pid: u32) -> String {
    use sysinfo::{Pid, System};
    let s = System::new_all();
    s.process(Pid::from_u32(pid))
        .map(|p| p.name().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// dump_active_window
// ---------------------------------------------------------------------------

pub fn dump_active_window() -> Result<Value, String> {
    let automation = get_automation()?;
    let root = unsafe { automation.GetFocusedElement() }
        .or_else(|_| unsafe { automation.GetRootElement() })
        .map_err(|e| format!("GetFocused/Root: {e}"))?;

    // Window name via direct property. Process id: try to get from the
    // element's CurrentProcessId (we don't have a direct getter for that on
    // IUIAutomationElement in 0.58, so fall back to the foreground HWND).
    let window_name = unsafe { root.CurrentName() }
        .map(|b| b.to_string())
        .unwrap_or_default();

    let process = unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            String::new()
        } else {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            proc_name_for_pid(pid)
        }
    };

    // Bounded walk using a raw TreeWalker.
    let walker = unsafe { automation.RawViewWalker() }
        .map_err(|e| format!("RawViewWalker: {e}"))?;
    let condition = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| format!("CreateTrueCondition: {e}"))?;

    let mut controls: Vec<Value> = Vec::new();
    let _ = walk(&walker, &root, &condition, 0, &mut controls);
    let control_count = controls.len();

    Ok(json!({
        "window": window_name,
        "process": process,
        "control_count": control_count,
        "controls": controls,
    }))
}

fn walk(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
    _condition: &windows::Win32::UI::Accessibility::IUIAutomationCondition,
    depth: usize,
    out: &mut Vec<Value>,
) -> Result<(), String> {
    if depth > MAX_DEPTH || out.len() >= MAX_ELEMENTS {
        return Ok(());
    }
    // First child of `parent`.
    let child_res = unsafe { walker.GetFirstChildElement(parent) };
    match child_res {
        Ok(child) => Ok(walk_sibling_chain(walker, &child, depth, out)),
        Err(_) => Ok(()),
    }
}

fn walk_sibling_chain(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    first: &IUIAutomationElement,
    depth: usize,
    out: &mut Vec<Value>,
) {
    let mut current: Option<IUIAutomationElement> = Some(first.clone());
    while let Some(el) = current.take() {
        if out.len() >= MAX_ELEMENTS {
            break;
        }
        if let Some(v) = describe(&el) {
            out.push(v);
        }
        // Recurse into children.
        if depth + 1 <= MAX_DEPTH {
            if let Ok(child) = unsafe { walker.GetFirstChildElement(&el) } {
                walk_sibling_chain(walker, &child, depth + 1, out);
            }
        }
        // Next sibling.
        match unsafe { walker.GetNextSiblingElement(&el) } {
            Ok(next) => current = Some(next),
            Err(_) => break,
        }
    }
}

fn describe(el: &IUIAutomationElement) -> Option<Value> {
    let name = unsafe { el.CurrentName() }.ok().map(|b| b.to_string()).unwrap_or_default();
    let automation_id = unsafe { el.CurrentAutomationId() }.ok().map(|b| b.to_string()).unwrap_or_default();
    let control_type_id = unsafe { el.CurrentControlType() }.ok().map(|c| c.0).unwrap_or(0);
    let enabled = unsafe { el.CurrentIsEnabled() }.ok().map(|b| b.as_bool()).unwrap_or(false);
    let rect = unsafe { el.CurrentBoundingRectangle() }.ok();

    if name.is_empty() && automation_id.is_empty() {
        return None;
    }
    let rect_arr = rect.map(|r| {
        if r.right > r.left && r.bottom > r.top {
            Some(json!([r.left, r.top, r.right - r.left, r.bottom - r.top]))
        } else {
            None
        }
    }).flatten();

    Some(json!({
        "name": name,
        "type": control_type_name(control_type_id),
        "automationId": automation_id,
        "enabled": enabled,
        "rect": rect_arr,
    }))
}

fn control_type_name(id: i32) -> &'static str {
    // UIA control type IDs — see <uiautomationclient.h>.
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "CheckBox",
        50003 => "ComboBox",
        50004 => "Edit",
        50005 => "Hyperlink",
        50006 => "Image",
        50007 => "ListItem",
        50008 => "List",
        50009 => "Menu",
        50010 => "MenuBar",
        50011 => "MenuItem",
        50012 => "ProgressBar",
        50013 => "RadioButton",
        50014 => "ScrollBar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "StatusBar",
        50018 => "Tab",
        50019 => "TabItem",
        50020 => "Text",
        50021 => "ToolBar",
        50022 => "ToolTip",
        50023 => "Tree",
        50024 => "TreeItem",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "DataGrid",
        50029 => "DataItem",
        50030 => "Document",
        50031 => "SplitButton",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "HeaderItem",
        50036 => "Table",
        50037 => "TitleBar",
        50038 => "Separator",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// Targeted actions
// ---------------------------------------------------------------------------

/// Find a single element anywhere under the root, by automation id. Returns
/// the first match (the first enabled one if any are enabled).
fn find_by_automation_id(automation_id: &str) -> Result<IUIAutomationElement, String> {
    let automation = get_automation()?;
    let root = unsafe { automation.GetRootElement() }
        .map_err(|e| format!("GetRootElement: {e}"))?;
    // UIA_AutomationIdPropertyId = 30011
    let cond = unsafe {
        automation.CreatePropertyCondition(
            windows::Win32::UI::Accessibility::UIA_AutomationIdPropertyId,
            &windows::core::VARIANT::from(BSTR::from(automation_id)),
        )
    }
    .map_err(|e| format!("CreatePropertyCondition: {e}"))?;

    let arr: IUIAutomationElementArray = unsafe {
        root.FindAll(
            windows::Win32::UI::Accessibility::TreeScope_Descendants,
            &cond,
        )
    }
    .map_err(|e| format!("FindAll: {e}"))?;

    let len = unsafe { arr.Length() }.map_err(|e| format!("Length: {e}"))? as usize;
    if len == 0 {
        return Err(format!("no control with automationId='{automation_id}'"));
    }
    // Prefer the first enabled, otherwise take the first.
    for i in 0..len {
        let el = unsafe { arr.GetElement(i as i32) }
            .map_err(|e| format!("GetElement({i}): {e}"))?;
        let enabled = unsafe { el.CurrentIsEnabled() }
            .ok()
            .map(|b| b.as_bool())
            .unwrap_or(false);
        if enabled {
            return Ok(el);
        }
    }
    unsafe { arr.GetElement(0) }.map_err(|e| format!("GetElement(0): {e}"))
}

pub fn invoke_by_id(automation_id: &str) -> Result<(), String> {
    let el = find_by_automation_id(automation_id)?;
    // UIA_InvokePatternId = 10000
    let pattern: IUIAutomationInvokePattern = unsafe {
        el.GetCurrentPattern(windows::Win32::UI::Accessibility::UIA_InvokePatternId)
    }
    .map_err(|e| format!("GetCurrentPattern(Invoke): {e}"))?
    .cast()
    .map_err(|e| format!("cast to IUIAutomationInvokePattern: {e}"))?;
    unsafe { pattern.Invoke() }.map_err(|e| format!("Invoke: {e}"))?;
    Ok(())
}

pub fn set_text_by_id(automation_id: &str, value: &str) -> Result<(), String> {
    let el = find_by_automation_id(automation_id)?;
    // UIA_ValuePatternId = 10002
    let pattern: IUIAutomationValuePattern = unsafe {
        el.GetCurrentPattern(windows::Win32::UI::Accessibility::UIA_ValuePatternId)
    }
    .map_err(|e| format!("GetCurrentPattern(Value): {e}"))?
    .cast()
    .map_err(|e| format!("cast to IUIAutomationValuePattern: {e}"))?;
    unsafe { pattern.SetValue(&BSTR::from(value)) }
        .map_err(|e| format!("ValuePattern.SetValue: {e}"))?;
    Ok(())
}

pub fn select_by_id(automation_id: &str, value: &str) -> Result<(), String> {
    let el = find_by_automation_id(automation_id)?;
    let automation = get_automation()?;

    // 1. Try ExpandCollapse.Expand — best-effort, some ComboBoxes are
    //    auto-expanded by selection.
    if let Ok(pat) = unsafe {
        el.GetCurrentPattern(
            windows::Win32::UI::Accessibility::UIA_ExpandCollapsePatternId,
        )
    } {
        if let Ok(pat) = pat.cast::<IUIAutomationExpandCollapsePattern>() {
            let _ = unsafe { pat.Expand() };
        }
    }

    // 2. Walk the children, find one with matching name.
    let cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| format!("CreateTrueCondition: {e}"))?;
    let arr: IUIAutomationElementArray = unsafe {
        el.FindAll(
            windows::Win32::UI::Accessibility::TreeScope_Descendants,
            &cond,
        )
    }
    .map_err(|e| format!("FindAll children: {e}"))?;

    let len = unsafe { arr.Length() }.map_err(|e| format!("Length: {e}"))? as usize;
    for i in 0..len {
        let item = unsafe { arr.GetElement(i as i32) }
            .map_err(|e| format!("GetElement({i}): {e}"))?;
        let name = unsafe { item.CurrentName() }
            .ok()
            .map(|b| b.to_string())
            .unwrap_or_default();
        if name.eq_ignore_ascii_case(value) {
            // UIA_SelectionItemPatternId = 10010
            let pat: IUIAutomationSelectionItemPattern = unsafe {
                item.GetCurrentPattern(
                    windows::Win32::UI::Accessibility::UIA_SelectionItemPatternId,
                )
            }
            .map_err(|e| format!("GetCurrentPattern(SelectionItem) on '{name}': {e}"))?
            .cast()
            .map_err(|e| format!("cast to SelectionItem: {e}"))?;
            unsafe { pat.Select() }.map_err(|e| format!("SelectionItem.Select: {e}"))?;
            return Ok(());
        }
    }
    Err(format!("item '{value}' not found in '{automation_id}' (scanned {len} descendants)"))
}

// ---------------------------------------------------------------------------
// Keyboard (real — SendInput path)
// ---------------------------------------------------------------------------

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

// Touch unused imports / types so the compiler keeps them available if a
// future edit needs them.
#[allow(dead_code)]
const _TOUCH: fn() = || {
    let _ = std::mem::size_of::<BSTR>();
    let _ = ExpandCollapseState_Collapsed;
};
