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

use image::{ImageBuffer, Rgba};
use serde_json::{json, Value};
use windows::core::{Interface, BSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationElementArray,
    IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationValuePattern, ExpandCollapseState_Collapsed,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SetFocus,
    VIRTUAL_KEY,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, PW_RENDERFULLCONTENT, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

// Bounded walk to keep dumps reasonable on apps like Visual Studio.
const MAX_DEPTH: usize = 5;
const MAX_ELEMENTS: usize = 200;

// ---------------------------------------------------------------------------
// COM init
// ---------------------------------------------------------------------------

/// One-time COM init for the current process. Idempotent.
fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

pub fn get_automation() -> Result<IUIAutomation, String> {
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

/// Walk the UIA subtree of `root_element` and build a tree. Bounded by
/// `MAX_DEPTH` (per branch) and `MAX_ELEMENTS` (total across the whole
/// tree). The counter is shared so a wide tree gets trimmed uniformly
/// rather than producing, say, 200 nodes in the leftmost branch and
/// nothing on the right.
/// Describe one element and everything under it, returning the nodes to attach
/// at this level — usually one, none when the branch is empty, several when the
/// element itself is anonymous and its children are lifted in its place.
///
/// **Anonymous containers are traversed, not dropped.** `describe_node` returns
/// `None` for an element with neither a name nor an automationId, and this
/// function used to return `Value::Null` on the spot — cutting away the entire
/// subtree below it. Dialogs are full of such unnamed panels, so the effect was
/// brutal and silent: LibreOffice's "Enregistrer sous" dumped as three nodes,
/// two buttons and their parent, while `find_main_edit` — which enumerates with
/// `FindAll` instead of walking — found a `listview` in that very same dialog a
/// second later. The file name field and the folder list were there all along,
/// hidden under a nameless panel. The assistant was not searching badly; it was
/// being shown an empty room.
///
/// An anonymous wrapper carries no information worth a node, so its children are
/// hoisted to its parent's level. It does not consume a depth step either: it is
/// invisible in the output, and letting it eat one of the five available levels
/// would push real content out of reach for no reason.
fn build_subtree(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    depth: usize,
    counter: &mut usize,
) -> Vec<Value> {
    if *counter >= MAX_ELEMENTS || depth > MAX_DEPTH {
        return Vec::new();
    }

    let desc = describe_node(element);
    // A described node owns a level; an anonymous one is passed through at the
    // same depth as its parent.
    let profondeur_enfants = if desc.is_some() { depth + 1 } else { depth };

    let mut children: Vec<Value> = Vec::new();
    if desc.is_some() {
        *counter += 1;
    }

    if profondeur_enfants <= MAX_DEPTH && *counter < MAX_ELEMENTS {
        if let Ok(first_child) = unsafe { walker.GetFirstChildElement(element) } {
            let mut current: Option<IUIAutomationElement> = Some(first_child);
            while let Some(el) = current.take() {
                if *counter >= MAX_ELEMENTS {
                    break;
                }
                children.extend(build_subtree(walker, &el, profondeur_enfants, counter));
                current = unsafe { walker.GetNextSiblingElement(&el) }.ok();
            }
        }
    }

    match desc {
        Some(desc) => {
            // Only what carries information.
            //
            // The dump travels through the assistant's context window, where it competes
            // with the very conversation it is meant to inform: on 2026-09-06 two dumps
            // were enough to fill it and drop the thread mid-task. An empty `children`,
            // an empty `automationId` and `enabled: true` say nothing a reader could not
            // assume, and together they were about four tenths of the payload. Absence is
            // the default; only departures from it are written.
            let mut node = serde_json::Map::new();
            node.insert("name".into(), json!(desc.name));
            node.insert("type".into(), json!(desc.type_name));
            if !desc.automation_id.is_empty() {
                node.insert("automationId".into(), json!(desc.automation_id));
            }
            if !desc.enabled {
                node.insert("enabled".into(), json!(false));
            }
            if let Some(rect) = desc.rect {
                node.insert("rect".into(), json!(rect));
            }
            if !children.is_empty() {
                node.insert("children".into(), json!(children));
            }
            vec![Value::Object(node)]
        }
        // Anonymous: hand our children up, and disappear.
        None => children,
    }
}

/// Build a JSON dump for a given IUIAutomationElement (and its source HWND
/// for window-name + process). Used by `dump_active_window` and
/// `dump_window_by_title`. Returns a tree, not a flat list.
fn build_dump(automation: &IUIAutomation, hwnd: HWND, root: &IUIAutomationElement) -> Result<Value, String> {
    let window_name = unsafe { root.CurrentName() }
        .map(|b| b.to_string())
        .unwrap_or_default();
    let process = if hwnd.is_invalid() {
        String::new()
    } else {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        proc_name_for_pid(pid)
    };

    let walker = unsafe { automation.RawViewWalker() }
        .map_err(|e| format!("RawViewWalker: {e}"))?;

    let mut counter: usize = 0;
    let noeuds = build_subtree(&walker, root, 0, &mut counter);
    let node_count = counter;

    // La racine elle-meme peut etre anonyme, et rendre alors plusieurs noeuds.
    // On les regroupe pour que "tree" reste un objet, comme avant.
    let tree = match noeuds.len() {
        0 => Value::Null,
        1 => noeuds.into_iter().next().unwrap_or(Value::Null),
        _ => json!({
            "name": window_name,
            "type": "Group",
            "automationId": "",
            "enabled": true,
            "rect": Value::Null,
            "children": noeuds,
        }),
    };

    Ok(json!({
        "window": window_name,
        "process": process,
        "node_count": node_count,
        "tree": tree,
    }))
}

pub fn dump_active_window() -> Result<Value, String> {
    let automation = get_automation()?;
    let root = unsafe { automation.GetFocusedElement() }
        .or_else(|_| unsafe { automation.GetRootElement() })
        .map_err(|e| format!("GetFocused/Root: {e}"))?;
    let hwnd = unsafe { GetForegroundWindow() };
    build_dump(&automation, hwnd, &root)
}

/// Dump the UIA subtree of a specific window identified by its title. The
/// title is matched as a case-insensitive substring against the visible
/// top-level windows — same logic as `screenshot_window_by_title`.
pub fn dump_window_by_title(title: &str) -> Result<Value, String> {
    let hwnd = find_hwnd_by_title(title)?;
    let automation = get_automation()?;
    // ElementFromHandle gives us the root UI element for that HWND.
    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| format!("ElementFromHandle({title}): {e}"))?;
    build_dump(&automation, hwnd, &root)
}

/// List all visible top-level windows with their HWND and title. Hidden /
/// minimized windows are skipped. Returns Vec<(HWND, title)>.
pub fn list_visible_windows() -> Result<Vec<(HWND, String)>, String> {
    // Thread-local accumulator: the EnumWindows callback is `extern
    // "system"` and cannot capture Rust state directly.
    thread_local! {
        static FOUND: std::cell::RefCell<Vec<(HWND, String)>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn collect_proc(hwnd: HWND, _lparam: isize) -> windows::Win32::Foundation::BOOL {
        // Skip invisible / minimized windows.
        if !IsWindowVisible(hwnd).as_bool() {
            return windows::Win32::Foundation::BOOL(1);
        }
        let mut text = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut text);
        if len > 0 {
            let title = String::from_utf16_lossy(&text[..len as usize]);
            if !title.is_empty() {
                FOUND.with(|v| v.borrow_mut().push((hwnd, title)));
            }
        }
        windows::Win32::Foundation::BOOL(1) // continue
    }
    unsafe extern "system" fn collect_proc_trampoline(
        hwnd: HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        collect_proc(hwnd, lparam.0)
    }

    FOUND.with(|v| v.borrow_mut().clear());
    let result = unsafe {
        windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(collect_proc_trampoline),
            windows::Win32::Foundation::LPARAM(0),
        )
    };
    let collected = FOUND.with(|v| v.borrow_mut().drain(..).collect::<Vec<_>>());
    result.map_err(|e| format!("EnumWindows: {e}"))?;
    Ok(collected)
}

/// Description extracted from an IUIAutomationElement. Used by
/// `build_subtree` to attach metadata to every tree node, and by
/// `find_main_edit` to score candidates.
struct NodeDesc {
    name: String,
    type_name: &'static str,
    automation_id: String,
    enabled: bool,
    rect: Option<(i32, i32, i32, i32)>, // (x, y, w, h)
}

fn rect_tuple(el: &IUIAutomationElement) -> Option<(i32, i32, i32, i32)> {
    let r = unsafe { el.CurrentBoundingRectangle() }.ok()?;
    if r.right <= r.left || r.bottom <= r.top {
        return None;
    }
    Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
}

fn describe_node(el: &IUIAutomationElement) -> Option<NodeDesc> {
    let name = unsafe { el.CurrentName() }.ok().map(|b| b.to_string()).unwrap_or_default();
    let automation_id = unsafe { el.CurrentAutomationId() }.ok().map(|b| b.to_string()).unwrap_or_default();
    let control_type_id = unsafe { el.CurrentControlType() }.ok().map(|c| c.0).unwrap_or(0);
    let enabled = unsafe { el.CurrentIsEnabled() }.ok().map(|b| b.as_bool()).unwrap_or(false);
    let rect = rect_tuple(el);

    if name.is_empty() && automation_id.is_empty() {
        return None;
    }
    Some(NodeDesc {
        name,
        type_name: control_type_name(control_type_id),
        automation_id,
        enabled,
        rect,
    })
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
// focus_window
// ---------------------------------------------------------------------------

/// Best-effort `SetForegroundWindow`. Windows normally refuses to let a
/// background process steal focus; the canonical workaround is to attach
/// the calling thread's input state to the foreground thread's, then
/// `SetForegroundWindow`, then detach. We also `SetFocus` on the HWND for
/// keyboard input and a 200 ms sleep to let the WM_SETFOCUS settle.
pub fn focus_window(hwnd: HWND) -> Result<(), String> {
    if hwnd.is_invalid() {
        return Err("invalid HWND".into());
    }
    unsafe {
        // Restore if minimized.
        let _ = ShowWindow(hwnd, SW_RESTORE);

        let foreground_hwnd = GetForegroundWindow();
        let foreground_thread = if foreground_hwnd.is_invalid() {
            GetCurrentThreadId()
        } else {
            GetWindowThreadProcessId(foreground_hwnd, None)
        };
        let current_thread = GetCurrentThreadId();

        let attached = if foreground_thread != current_thread {
            let ok = AttachThreadInput(foreground_thread, current_thread, true).as_bool();
            let _ = SetForegroundWindow(hwnd);
            if ok {
                let _ = AttachThreadInput(foreground_thread, current_thread, false);
            }
            true
        } else {
            let _ = SetForegroundWindow(hwnd);
            false
        };

        let _ = SetFocus(hwnd);
        let _ = attached;

        // Verify, do not assume.
        //
        // Every Win32 call above is best-effort and its result was discarded,
        // so this function used to return Ok(()) whether or not the focus had
        // actually moved. Windows refuses SetForegroundWindow from a process
        // that does not already own the foreground — pc-agent never does — and
        // the AttachThreadInput workaround only covers the calling thread, so
        // it is routinely denied. The caller then typed into whatever window
        // really had focus: on 2026-09-06 the assistant reported "tape 4
        // caractere(s)" while LibreOffice stayed at "0 mot, 0 caractere",
        // because the keystrokes had gone somewhere else entirely.
        //
        // An honest failure here is worth far more than a cheerful ok: it lets
        // the caller fall back (click the window first, ask the user to focus
        // it) instead of typing blind into another application.
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if GetForegroundWindow().0 as u64 == hwnd.0 as u64 {
                return Ok(());
            }
        }

        let actual = GetForegroundWindow();
        Err(format!(
            "focus refused by Windows: foreground is still HWND {} ('{}') instead of {}. \
             A background process cannot steal focus; click the target window, or use a \
             mouse click on it, before sending keystrokes.",
            actual.0 as u64,
            window_title(actual),
            hwnd.0 as u64
        ))
    }
}

/// Title of a window, or an empty string. Used to name the window that kept
/// the foreground when a focus attempt is refused.
fn window_title(hwnd: HWND) -> String {
    if hwnd.is_invalid() {
        return String::new();
    }
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// find_main_edit
// ---------------------------------------------------------------------------

/// Walk the descendants of `root` and pick the best Edit/Pane/Document to
/// type text into. Heuristic:
///   1. any `Document` (always wins — it's the main canvas in modern apps)
///   2. otherwise the largest `Pane` (likely a content area)
///   3. otherwise the largest `Edit` (a literal text box)
/// Returns `Ok(Some(json))` with `{automationId, name, type, rect}` or
/// `Ok(None)` if no candidate matches.
pub fn find_main_edit(
    automation: &IUIAutomation,
    root: &IUIAutomationElement,
) -> Result<Option<Value>, String> {
    let cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| format!("CreateTrueCondition: {e}"))?;
    let arr: IUIAutomationElementArray = unsafe {
        root.FindAll(
            windows::Win32::UI::Accessibility::TreeScope_Descendants,
            &cond,
        )
    }
    .map_err(|e| format!("FindAll: {e}"))?;
    let len_res = unsafe { arr.Length() };
    let len = match len_res {
        Ok(n) => n as usize,
        Err(_) => return Ok(None), // can't determine length → no candidates
    };

    // (score, NodeDesc). We don't keep the IUIAutomationElement since the
    // caller only needs the metadata (automationId, name, type, rect).
    let mut best: Option<(i64, NodeDesc)> = None;
    for i in 0..len.min(MAX_ELEMENTS) {
        let Ok(el) = (unsafe { arr.GetElement(i as i32) }) else { continue };
        let Some(desc) = describe_node(&el) else { continue };
        if !desc.enabled {
            continue;
        }
        let area: i64 = desc
            .rect
            .map(|(_, _, w, h)| (w as i64) * (h as i64))
            .unwrap_or(0);
        if area == 0 {
            continue;
        }
        let score: i64 = match desc.type_name {
            // Document always wins, then Pane, then Edit. Area is a
            // tie-breaker within the same type.
            "Document" => 1_000_000_000 + area,
            "Pane" => 1_000_000 + area,
            "Edit" => 1_000 + area,
            _ => continue,
        };
        if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
            best = Some((score, desc));
        }
    }

    Ok(best.map(|(_, d)| {
        let rect_arr = d.rect.map(|(x, y, w, h)| json!([x, y, w, h]));
        json!({
            "automationId": d.automation_id,
            "name": d.name,
            "type": d.type_name,
            "rect": rect_arr,
        })
    }))
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

/// Find a control by the name the user sees on it, inside one window.
///
/// `automationId` is what `invoke` has always taken, and in many dialogs it is
/// an opaque ordinal: LibreOffice's "Enregistrer sous" exposes its two buttons
/// as "1" and "2". Choosing between them by position is a coin flip, and on
/// 2026-09-06 the assistant lost it — it invoked "2" and cancelled the save it
/// had been asked to perform. The name is right there in the dump, it is what
/// the user would read on screen, and it is what the caller should aim at.
///
/// The search is scoped to a window rather than the desktop root: a name like
/// "Enregistrer" or "OK" exists in a dozen places at once, and walking every
/// descendant of the root element would be both slow and ambiguous. Default is
/// the foreground window, which is the one a dialog has just taken over.
///
/// Matching goes from strict to loose — exact, then case-insensitive, then
/// substring — and prefers an enabled control at each step, because a greyed
/// out "Enregistrer" next to an active one is not the target. When nothing
/// matches, the error lists the names that do exist, so the caller can pick
/// instead of guessing again.
fn find_by_name(name: &str, title: Option<&str>) -> Result<IUIAutomationElement, String> {
    let automation = get_automation()?;

    let hwnd = match title {
        Some(t) if !t.is_empty() => find_hwnd_by_title(t)?,
        _ => unsafe { GetForegroundWindow() },
    };
    if hwnd.is_invalid() {
        return Err("no foreground window to search in".into());
    }
    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| format!("ElementFromHandle: {e}"))?;

    let cond = unsafe { automation.CreateTrueCondition() }
        .map_err(|e| format!("CreateTrueCondition: {e}"))?;
    let arr: IUIAutomationElementArray = unsafe {
        root.FindAll(
            windows::Win32::UI::Accessibility::TreeScope_Descendants,
            &cond,
        )
    }
    .map_err(|e| format!("FindAll: {e}"))?;
    let len = unsafe { arr.Length() }.map_err(|e| format!("Length: {e}"))? as usize;

    let wanted = name.trim();
    let wanted_lower = wanted.to_lowercase();

    // (element, name, enabled) for every named descendant, capped like the dumps.
    let mut candidates: Vec<(IUIAutomationElement, String, bool)> = Vec::new();
    for i in 0..len.min(MAX_ELEMENTS) {
        let Ok(el) = (unsafe { arr.GetElement(i as i32) }) else {
            continue;
        };
        let el_name = unsafe { el.CurrentName() }
            .ok()
            .map(|b| b.to_string())
            .unwrap_or_default();
        if el_name.trim().is_empty() {
            continue;
        }
        let enabled = unsafe { el.CurrentIsEnabled() }
            .ok()
            .map(|b| b.as_bool())
            .unwrap_or(false);
        candidates.push((el, el_name, enabled));
    }

    for pass in 0..3 {
        // Enabled first within each pass: a disabled twin is never the target.
        for want_enabled in [true, false] {
            for (el, el_name, enabled) in &candidates {
                if *enabled != want_enabled {
                    continue;
                }
                let trimmed = el_name.trim();
                let hit = match pass {
                    0 => trimmed == wanted,
                    1 => trimmed.eq_ignore_ascii_case(wanted),
                    _ => trimmed.to_lowercase().contains(&wanted_lower),
                };
                if hit {
                    return Ok(el.clone());
                }
            }
        }
    }

    // List what exists, but capped. In a dialog this is a handful of buttons and
    // exactly the help the caller needs; in a data-heavy window like the task
    // manager it would otherwise be two hundred table cells, drowning the answer
    // it is meant to serve.
    let mut noms: Vec<&str> = candidates.iter().map(|(_, n, _)| n.trim()).collect();
    noms.sort_unstable();
    noms.dedup();
    const MAX_NOMS: usize = 40;
    let total = noms.len();
    let listed = noms.len().min(MAX_NOMS);
    let suite = if total > listed {
        format!(" … and {} more", total - listed)
    } else {
        String::new()
    };
    Err(format!(
        "no control named '{name}' in this window. {total} named controls present: {}{suite}",
        noms[..listed].join(" | ")
    ))
}

/// Invoke a control by the name shown on it. See [`find_by_name`].
pub fn invoke_by_name(name: &str, title: Option<&str>) -> Result<Value, String> {
    let el = find_by_name(name, title)?;
    let actual = unsafe { el.CurrentName() }
        .ok()
        .map(|b| b.to_string())
        .unwrap_or_default();

    let pattern: IUIAutomationInvokePattern =
        pattern_of(&el, windows::Win32::UI::Accessibility::UIA_InvokePatternId, "Invoke")?
            .cast()
            .map_err(|e| format!("cast to IUIAutomationInvokePattern: {e}"))?;
    unsafe { pattern.Invoke() }.map_err(|e| format!("Invoke: {e}"))?;

    // Echo the name actually invoked: the match may have been loose, and the
    // caller must be able to see that it hit "Enregistrer sous..." when it
    // asked for "Enregistrer".
    Ok(json!({ "invoked": actual }))
}

/// Fetch a UIA pattern, telling "this element does not support it" apart from
/// a genuine COM failure.
///
/// `GetCurrentPattern` succeeds and hands back a NULL interface pointer when
/// the element simply does not implement the pattern. windows-rs turns that
/// null into `Err(Error::from_win32())`, which reads `GetLastError()` — and
/// since nothing failed, the code it carries is ERROR_SUCCESS. Callers were
/// therefore reporting the self-contradictory
/// `GetCurrentPattern(Value): L'opération a réussi. (0x00000000)`, which told
/// the assistant nothing and sent it looping: LibreOffice's Document element
/// does not implement ValuePattern, and that is a fact to act on (type with the
/// keyboard instead), not an error to retry.
///
/// An HRESULT that `is_ok()` is exactly that null-pointer case, so it is the
/// discriminator: anything else really did fail.
fn pattern_of(
    el: &IUIAutomationElement,
    pattern_id: windows::Win32::UI::Accessibility::UIA_PATTERN_ID,
    name: &str,
) -> Result<windows::core::IUnknown, String> {
    match unsafe { el.GetCurrentPattern(pattern_id) } {
        Ok(p) => Ok(p),
        Err(e) if e.code().is_ok() => Err(format!(
            "this element does not support the {name} pattern; \
             act on it another way — keyboard for text (focus-and-type), \
             a mouse click on its rect for a button"
        )),
        Err(e) => Err(format!("GetCurrentPattern({name}): {e}")),
    }
}

pub fn invoke_by_id(automation_id: &str) -> Result<(), String> {
    let el = find_by_automation_id(automation_id)?;
    // UIA_InvokePatternId = 10000
    let pattern: IUIAutomationInvokePattern =
        pattern_of(&el, windows::Win32::UI::Accessibility::UIA_InvokePatternId, "Invoke")?
            .cast()
            .map_err(|e| format!("cast to IUIAutomationInvokePattern: {e}"))?;
    unsafe { pattern.Invoke() }.map_err(|e| format!("Invoke: {e}"))?;
    Ok(())
}

pub fn set_text_by_id(automation_id: &str, value: &str) -> Result<(), String> {
    let el = find_by_automation_id(automation_id)?;
    // UIA_ValuePatternId = 10002
    let pattern: IUIAutomationValuePattern =
        pattern_of(&el, windows::Win32::UI::Accessibility::UIA_ValuePatternId, "Value")?
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
            let pat: IUIAutomationSelectionItemPattern = pattern_of(
                &item,
                windows::Win32::UI::Accessibility::UIA_SelectionItemPatternId,
                "SelectionItem",
            )
            .map_err(|e| format!("{e} (on '{name}')"))?
            .cast()
            .map_err(|e| format!("cast to SelectionItem: {e}"))?;
            unsafe { pat.Select() }.map_err(|e| format!("SelectionItem.Select: {e}"))?;
            return Ok(());
        }
    }
    Err(format!("item '{value}' not found in '{automation_id}' (scanned {len} descendants)"))
}

// ---------------------------------------------------------------------------
// screenshot_window_by_title
// ---------------------------------------------------------------------------

/// Find a top-level window by exact title via FindWindowW, or by substring
/// match via EnumWindows if the exact lookup fails. Hidden / minimized
/// windows are included — the caller can check IsWindowVisible if they care.
pub fn find_hwnd_by_title(title: &str) -> Result<HWND, String> {
    // 1. Try exact match first.
    let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let exact = unsafe {
        FindWindowW(windows::core::PCWSTR::null(), windows::core::PCWSTR(title_w.as_ptr())).ok()
    };
    if let Some(h) = exact {
        return Ok(h);
    }

    // 2. Substring match: walk all top-level windows. We use a thread-local
    //    pair (term, found) because the EnumWindows callback is `extern
    //    "system"` and cannot capture Rust state directly. The callback
    //    runs on the same thread as the call, so the thread-local is
    //    safe.
    struct SearchCtx {
        term_lower: String,
        found: Option<HWND>,
    }
    thread_local! {
        static SEARCH: std::cell::RefCell<Option<SearchCtx>> = const { std::cell::RefCell::new(None) };
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: isize) -> windows::Win32::Foundation::BOOL {
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        let title = if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };
        SEARCH.with(|s| {
            let mut b = s.borrow_mut();
            if let Some(ctx) = b.as_mut() {
                if ctx.found.is_none() && title.to_lowercase().contains(&ctx.term_lower) {
                    ctx.found = Some(hwnd);
                }
            }
        });
        if SEARCH.with(|s| s.borrow().as_ref().and_then(|c| c.found).is_some()) {
            windows::Win32::Foundation::BOOL(0) // stop
        } else {
            windows::Win32::Foundation::BOOL(1) // continue
        }
    }

    unsafe extern "system" fn enum_proc_trampoline(
        hwnd: HWND,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::BOOL {
        enum_proc(hwnd, lparam.0)
    }

    // Set context, run, read.
    SEARCH.with(|s| {
        *s.borrow_mut() = Some(SearchCtx {
            term_lower: title.to_lowercase(),
            found: None,
        });
    });
    let result = unsafe {
        windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(enum_proc_trampoline),
            windows::Win32::Foundation::LPARAM(0),
        )
    };
    let found = SEARCH.with(|s| s.borrow_mut().take().and_then(|c| c.found));
    // Clear the thread-local to avoid leaking between requests on the
    // same worker thread.
    SEARCH.with(|s| *s.borrow_mut() = None);

    // A FALSE return from EnumWindows is not an error here: our callback
    // returns FALSE on purpose, to stop the walk as soon as it has a match.
    // windows-rs turns that FALSE into Err(Error::from_win32()), and since
    // nothing actually failed the code it carries is ERROR_SUCCESS — hence the
    // self-contradictory "EnumWindows: L'opération a réussi. (0x00000000)"
    // that every substring lookup used to report. Exact-title lookups were
    // spared only because FindWindowW answers before we ever get here, which
    // is why "OpenCode" worked and "LibreOffice Writer" — a substring of
    // "Sans nom 1 — LibreOffice Writer" — never did.
    //
    // So the result is only worth inspecting when we came back empty-handed:
    // that is the one case where FALSE may mean a real failure rather than a
    // deliberate early stop.
    if found.is_none() {
        result.map_err(|e| format!("EnumWindows: {e}"))?;
    }
    found.ok_or_else(|| format!("no window with title containing '{title}'"))
}

/// Capture a specific window by its title into a PNG file. The result is a
/// JSON object with the saved path and the captured dimensions.
pub fn screenshot_window_by_title(title: &str) -> Result<Value, String> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
        ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};

    let hwnd = find_hwnd_by_title(title)?;

    // Get window rect (screen coordinates).
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect).map_err(|e| format!("GetWindowRect: {e}"))?; }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
        return Err(format!(
            "invalid rect for '{title}': {}x{} (window minimized or off-screen?)",
            width, height
        ));
    }

    let captures_dir = r"C:\Program Files\SenSÉ\Captures";
    if let Err(e) = std::fs::create_dir_all(captures_dir) {
        return Err(format!("create_dir({captures_dir}): {e}"));
    }

    // Device contexts.
    let hdc_screen = unsafe { GetDC(hwnd) };
    let hdc_mem = unsafe { CreateCompatibleDC(hdc_screen) };
    let hbm = unsafe { CreateCompatibleBitmap(hdc_screen, width, height) };

    unsafe {
        let _ = SelectObject(hdc_mem, hbm);
        // PW_RENDERFULLCONTENT (0x2) captures the DWM-composed window,
        // which is what you see on screen (not just the legacy GDI bits).
        let _ = PrintWindow(hwnd, hdc_mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT));
    }

    // Extract pixels.
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // negative = top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [Default::default(); 1],
    };

    let mut buffer: Vec<u8> = vec![0u8; (width as usize) * (height as usize) * 4];
    let scan_lines = unsafe {
        GetDIBits(
            hdc_mem,
            hbm.clone(),
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    if scan_lines == 0 {
        return Err("GetDIBits returned 0 scanlines".into());
    }

    // GDI gives us BGRA; the `image` crate wants RGBA. Swap B and R.
    for chunk in buffer.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    // Build image + save PNG.
    let img_buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width as u32, height as u32, buffer)
            .ok_or_else(|| "ImageBuffer::from_raw returned None".to_string())?;
    let ts = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S").to_string();
    let path = format!(r"{}\screenshot-{}.png", captures_dir, ts);
    img_buf.save(&path).map_err(|e| format!("save PNG: {e}"))?;

    // Cleanup GDI handles. `hbm` was moved into `SelectObject` above, but
    // `SelectObject` returns the *previous* handle — so on most windows
    // `hbm` is still ours and should be deleted. We do a defensive `is_err`
    // check via the BOOL return to avoid double-free if the bitmap was
    // discarded internally.
    unsafe {
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(hwnd, hdc_screen);
    }

    Ok(json!({
        "path": path,
        "width": width,
        "height": height,
        "title": title,
        "hwnd": hwnd.0 as u64,
        "visible": unsafe { IsWindowVisible(hwnd).as_bool() },
    }))
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
        .ok_or_else(|| format!("unknown key: '{}'. {}", parts[parts.len() - 1], known_key_hints()))?;
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

/// Send one UTF-16 code unit as a synthetic key press, character by character.
///
/// `KEYEVENTF_UNICODE` bypasses the keyboard layout entirely: the scan code
/// carries the character itself, so accents and symbols land the same way on
/// AZERTY, QWERTY or anything else. Surrogate pairs work because each unit is
/// sent on its own, which is what Windows expects.
fn send_unicode(unit: u16) {
    for key_up in [false, true] {
        let flags = if key_up {
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
        } else {
            KEYEVENTF_UNICODE
        };
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
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
}

/// Type a string into whatever currently has keyboard focus. Returns the
/// number of UTF-16 units sent.
pub fn type_text(text: &str) -> usize {
    let mut sent = 0usize;
    for unit in text.encode_utf16() {
        send_unicode(unit);
        sent += 1;
    }
    sent
}

/// Focus a window and type into it, in one call and on one thread.
///
/// This exists because splitting the two across processes cannot be made
/// reliable. Windows ties the foreground and the input queue to a thread:
/// `AttachThreadInput` only lends the calling thread the right to redirect
/// focus, and only for as long as it holds it. When focus came from pc-agent
/// and the keystrokes came from a second process a moment later, nothing tied
/// the two together — and the text landed in whichever window really had the
/// foreground. Doing both here, synchronously, is what makes the guarantee
/// meaningful.
///
/// `focus_window` now fails loudly when the focus is refused, so this returns
/// an error rather than typing into the wrong application. The reply carries
/// what the caller needs to decide what to do next, without a second round
/// trip: the window it typed into, and how much it sent.
pub fn focus_and_type(hwnd: HWND, text: &str) -> Result<Value, String> {
    focus_window(hwnd)?;

    let sent = type_text(text);
    let foreground = unsafe { GetForegroundWindow() };

    Ok(json!({
        "hwnd": hwnd.0 as u64,
        "title": window_title(hwnd),
        "units_sent": sent,
        // Re-read after typing: a window can lose the foreground mid-sequence
        // (a notification, another app stealing it), and the caller should be
        // told rather than left to assume the whole string landed.
        "still_foreground": foreground.0 as u64 == hwnd.0 as u64,
    }))
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
        // Return / Enter
        "return" | "enter" | "cr" | "retour" => 0x0D,
        // Escape
        "escape" | "esc" | "echap" => 0x1B,
        // Tab
        "tab" | "tabulation" => 0x09,
        // Space
        "space" | "spc" | "espace" => 0x20,
        // Backspace
        "backspace" | "bs" | "retour arriere" | "retourchariot" => 0x08,
        // Delete
        "delete" | "del" | "supprimer" | "suppr" => 0x2E,
        // Insert
        "insert" | "ins" | "inserer" => 0x2D,
        // Print Screen / Impr ecran
        "print" | "printscreen" | "print screen" | "prtsc" | "prtscr"
            | "snapshot" | "impr" | "impr ecran" | "imprecran" => 0x2C,
        // Home / End
        "home" | "debut" => 0x24,
        "end" | "fin" => 0x23,
        // Page Up / Page Down
        "pageup" | "pgup" | "pg prec" | "pguprec" | "page prec" => 0x21,
        "pagedown" | "pgdn" | "pg suiv" | "pgsuiv" | "page suiv" => 0x22,
        // Arrow keys
        "left" | "gauche" | "fleche gauche" | "flechegauche" => 0x25,
        "up" | "haut" | "fleche haut" | "flechehaut" => 0x26,
        "right" | "droite" | "fleche droite" | "flechedroite" => 0x27,
        "down" | "bas" | "fleche bas" | "flechebas" => 0x28,
        // Function keys
        "f1" => 0x70, "f2" => 0x71, "f3" => 0x72, "f4" => 0x73,
        "f5" => 0x74, "f6" => 0x75, "f7" => 0x76, "f8" => 0x77,
        "f9" => 0x78, "f10" => 0x79, "f11" => 0x7A, "f12" => 0x7B,
        // Lock keys
        "capslock" | "caps lock" | "verrouillage maj" => 0x14,
        "numlock" | "num lock" | "verrouillage num" => 0x90,
        "scrolllock" | "scroll lock" | "verrouillage defil" => 0x91,
        // Single alphanumeric: A=0x41, 0=0x30
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

/// Hint returned when a key name doesn't match. Lists a sample of the
/// most common English + French aliases so the caller has a fighting chance
/// of guessing the right name without reading the source.
fn known_key_hints() -> &'static str {
    "known keys: a-z, 0-9, F1-F12, Return/Enter/Retour, Escape/Echap/Esc, \
     Tab/Tabulation, Space/Espace, Backspace/Retour, Delete/Supprimer/Suppr, \
     Insert/Ins, PrintScreen/Impr/PrtSc/Snapshot, Home/Debut, End/Fin, \
     PageUp/PageDown/PgUp/PgDn, Left/Right/Up/Down/Gauche/Droite/Haut/Bas; \
     modifiers: Ctrl/Control, Shift, Alt/Menu, Win/Super/Meta"
}

// Touch unused imports / types so the compiler keeps them available if a
// future edit needs them.
#[allow(dead_code)]
const _TOUCH: fn() = || {
    let _ = std::mem::size_of::<BSTR>();
    let _ = ExpandCollapseState_Collapsed;
};
