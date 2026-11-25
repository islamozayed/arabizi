//! Narrator and other screen readers (UI Automation). The widget is
//! custom-drawn, so the window answers `WM_GETOBJECT` with a provider whose
//! children are the core's accessibility nodes: buttons, list items, radio
//! buttons, toggle switches and text, with their states, and pressing one
//! clicks it. The text fields are real EDIT controls kept under the drawn
//! fields, so their text and caret come from Windows' own support.

// UI Automation's constants are matched by their Windows names.
#![allow(non_upper_case_globals)]

use super::{after_event, on_main, scale, with_widget, with_widget_ro};
use franconvert_core::ui::access::{AxNode, AxRole};
use franconvert_core::ui::Target;
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use windows::core::{implement, Error, IUnknown, Interface, Result, BOOL, BSTR, HRESULT, PCWSTR};
use windows_core::IUnknownImpl;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, SAFEARRAY};
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayPutElement};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_BSTR, VT_I4};
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::{CHILDID_SELF, OBJID_CLIENT};

const FRAMEWORK: &str = "Franconvert";

/// Identifies a node across frames: its target, or for plain text its
/// position among the text nodes.
fn key_of(node: &AxNode, text_index: usize) -> String {
    match node.target {
        Some(t) => format!("{t:?}"),
        None => format!("Text{text_index}"),
    }
}

/// The current nodes with their keys, in reading order.
fn current() -> Vec<(String, AxNode)> {
    let nodes = with_widget_ro(|w| w.ax_nodes()).unwrap_or_default();
    let mut texts = 0;
    let mut out = Vec::new();
    for n in nodes {
        if n.rect.w <= 0.0 || n.rect.h <= 0.0 {
            continue;
        }
        let key = key_of(&n, texts);
        if n.target.is_none() {
            texts += 1;
        }
        out.push((key, n));
    }
    out
}

/// What was last reported, to raise change events for what differs.
#[derive(PartialEq)]
struct Reported {
    key: String,
    label: String,
    selected: bool,
    checked: Option<bool>,
}

thread_local! {
    static ROOT: RefCell<Option<IRawElementProviderSimple>> = const { RefCell::new(None) };
    static LAST: RefCell<Vec<Reported>> = const { RefCell::new(Vec::new()) };
}

fn root(hwnd: HWND) -> IRawElementProviderSimple {
    ROOT.with(|r| r.borrow_mut().get_or_insert_with(|| Root { hwnd: hwnd.0 as isize }.into()).clone())
}

fn element(hwnd: isize, key: &str) -> IRawElementProviderFragment {
    Element { hwnd, key: key.to_string() }.into()
}

/// `WM_GETOBJECT`: hands UI Automation the window's provider.
pub fn get_object(hwnd: HWND, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    if lp.0 as i32 != UiaRootObjectId {
        return None;
    }
    let root = root(hwnd);
    Some(unsafe { UiaReturnRawElementProvider(hwnd, wp, lp, &root) })
}

/// Gives each text field a spoken name (an EDIT has none of its own).
pub fn name_field(edit: HWND, name: PCWSTR) {
    unsafe {
        let Ok(svc) = CoCreateInstance::<_, IAccPropServices>(&CLSID_AccPropServices, None, CLSCTX_INPROC_SERVER) else { return };
        let _ = svc.SetHwndPropStr(edit, OBJID_CLIENT.0 as u32, CHILDID_SELF, PROPID_ACC_NAME, name);
    }
}

/// Speaks `text` now (the highlighted suggestion while arrowing).
pub fn announce(hwnd: HWND, text: &str) {
    unsafe {
        if !UiaClientsAreListening().as_bool() {
            return;
        }
        let _ = UiaRaiseNotificationEvent(
            &root(hwnd),
            NotificationKind_ActionCompleted,
            NotificationProcessing_ImportantMostRecent,
            &BSTR::from(text),
            &BSTR::from("franconvert-announce"),
        );
    }
}

/// After a redraw: tells listening screen readers what changed.
pub fn refresh(hwnd: HWND) {
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    let now: Vec<Reported> = current()
        .into_iter()
        .map(|(key, n)| Reported { key, label: n.label, selected: n.selected, checked: n.checked })
        .collect();
    let before = LAST.with(|l| std::mem::replace(&mut *l.borrow_mut(), Vec::new()));
    let id = hwnd.0 as isize;
    unsafe {
        let keys_changed = before.len() != now.len() || before.iter().zip(&now).any(|(a, b)| a.key != b.key);
        if keys_changed {
            let r = root(hwnd);
            let mut rid = [UiaAppendRuntimeId as i32, id as i32];
            let _ = UiaRaiseStructureChangedEvent(&r, StructureChangeType_ChildrenInvalidated, rid.as_mut_ptr(), rid.len() as i32);
        }
        // A new set of controls is read afresh; only report changes in place.
        for n in now.iter().filter(|_| !keys_changed) {
            let Some(old) = before.iter().find(|o| o.key == n.key) else { continue };
            if old == n {
                continue;
            }
            let Ok(el) = element(id, &n.key).cast::<IRawElementProviderSimple>() else { continue };
            if old.label != n.label {
                let _ = UiaRaiseAutomationPropertyChangedEvent(&el, UIA_NamePropertyId, &bstr_var(&old.label), &bstr_var(&n.label));
            }
            if old.checked != n.checked {
                let _ = UiaRaiseAutomationPropertyChangedEvent(
                    &el,
                    UIA_ToggleToggleStatePropertyId,
                    &int_var(toggle_state(old.checked).0),
                    &int_var(toggle_state(n.checked).0),
                );
            }
            if old.selected != n.selected {
                let _ = UiaRaiseAutomationPropertyChangedEvent(&el, UIA_SelectionItemIsSelectedPropertyId, &bool_var(old.selected), &bool_var(n.selected));
                if n.selected {
                    let _ = UiaRaiseAutomationEvent(&el, UIA_SelectionItem_ElementSelectedEventId);
                }
            }
        }
    }
    LAST.with(|l| *l.borrow_mut() = now);
}

/// Lets go of every provider (on quit).
pub fn shutdown(hwnd: HWND) {
    unsafe {
        let _ = UiaReturnRawElementProvider(hwnd, WPARAM(0), LPARAM(0), None);
        if let Some(r) = ROOT.with(|r| r.borrow_mut().take()) {
            let _ = UiaDisconnectProvider(&r);
        }
    }
}

// ── Values ───────────────────────────────────────────────────────────────

fn bstr_var(s: &str) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { bstrVal: ManuallyDrop::new(BSTR::from(s)) },
            }),
        },
    }
}

fn int_var(v: i32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 { vt: VT_I4, wReserved1: 0, wReserved2: 0, wReserved3: 0, Anonymous: VARIANT_0_0_0 { lVal: v } }),
        },
    }
}

fn bool_var(v: bool) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BOOL,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { boolVal: windows::Win32::Foundation::VARIANT_BOOL(if v { -1 } else { 0 }) },
            }),
        },
    }
}

fn toggle_state(checked: Option<bool>) -> ToggleState {
    if checked.unwrap_or(false) { ToggleState_On } else { ToggleState_Off }
}

fn runtime_id(parts: &[i32]) -> Result<*mut SAFEARRAY> {
    unsafe {
        let arr = SafeArrayCreateVector(VT_I4, 0, parts.len() as u32);
        if arr.is_null() {
            return Err(Error::from(windows::Win32::Foundation::E_OUTOFMEMORY));
        }
        for (i, v) in parts.iter().enumerate() {
            let i = i as i32;
            SafeArrayPutElement(arr, &i, (v as *const i32).cast())?;
        }
        Ok(arr)
    }
}

fn not_available() -> Error {
    Error::from(HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}

/// "Nothing": a null result with success.
fn none<T>() -> Result<T> {
    Err(Error::empty())
}

/// Screen rectangle (physical px) of a node rectangle (DIPs, client).
fn screen_rect(hwnd: isize, r: franconvert_core::ui::geom::Rect) -> UiaRect {
    let hwnd = HWND(hwnd as *mut _);
    let s = scale(hwnd) as f64;
    let mut origin = POINT::default();
    unsafe {
        let _ = ClientToScreen(hwnd, &mut origin);
    }
    UiaRect { left: origin.x as f64 + r.x as f64 * s, top: origin.y as f64 + r.y as f64 * s, width: r.w as f64 * s, height: r.h as f64 * s }
}

// ── The window's provider ────────────────────────────────────────────────

#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot, Agile = false)]
struct Root {
    hwnd: isize,
}

impl IRawElementProviderSimple_Impl for Root_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading)
    }

    fn GetPatternProvider(&self, _id: UIA_PATTERN_ID) -> Result<IUnknown> {
        none()
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        Ok(match id {
            UIA_FrameworkIdPropertyId => bstr_var(FRAMEWORK),
            _ => VARIANT::default(),
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        unsafe { UiaHostProviderFromHwnd(HWND(self.hwnd as *mut _)) }
    }
}

impl IRawElementProviderFragment_Impl for Root_Impl {
    fn Navigate(&self, dir: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let nodes = current();
        let pick = match dir {
            NavigateDirection_FirstChild => nodes.first(),
            NavigateDirection_LastChild => nodes.last(),
            _ => None,
        };
        match pick {
            Some((key, _)) => Ok(element(self.hwnd, key)),
            None => none(),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        // A window's runtime id comes from its host provider.
        Ok(std::ptr::null_mut())
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        // Also from the host provider (the window rectangle).
        Ok(UiaRect::default())
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        self.to_object().cast()
    }
}

impl IRawElementProviderFragmentRoot_Impl for Root_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let hit = current().into_iter().rev().find(|(_, n)| {
            let r = screen_rect(self.hwnd, n.rect);
            n.target.is_some() && x >= r.left && x < r.left + r.width && y >= r.top && y < r.top + r.height
        });
        match hit {
            Some((key, _)) => Ok(element(self.hwnd, &key)),
            None => self.to_object().cast(),
        }
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        // Keyboard focus is in a text field, a window of its own.
        none()
    }
}

// ── One drawn control ────────────────────────────────────────────────────

#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IInvokeProvider, IToggleProvider, ISelectionItemProvider, Agile = false)]
struct Element {
    hwnd: isize,
    key: String,
}

impl Element {
    /// This element's node now, with its position in the list.
    fn node(&self) -> Result<(usize, Vec<(String, AxNode)>)> {
        let nodes = current();
        let i = nodes.iter().position(|(k, _)| *k == self.key).ok_or_else(not_available)?;
        Ok((i, nodes))
    }

    fn get(&self) -> Result<AxNode> {
        let (i, mut nodes) = self.node()?;
        Ok(nodes.swap_remove(i).1)
    }

    fn target(&self) -> Result<Target> {
        self.get()?.target.ok_or_else(|| Error::from(HRESULT(UIA_E_INVALIDOPERATION as i32)))
    }

    /// Clicks the control once the current UI Automation call has returned.
    fn press(&self) -> Result<()> {
        let t = self.target()?;
        let key = self.key.clone();
        on_main(move || {
            // Only if it's still there.
            if current().iter().any(|(k, _)| *k == key) {
                with_widget(|w| w.ax_press(t));
                after_event();
            }
        });
        Ok(())
    }
}

fn control_type(role: AxRole) -> UIA_CONTROLTYPE_ID {
    match role {
        AxRole::Button | AxRole::Switch => UIA_ButtonControlTypeId,
        AxRole::Option => UIA_ListItemControlTypeId,
        AxRole::Radio => UIA_RadioButtonControlTypeId,
        AxRole::Slider => UIA_SliderControlTypeId,
        AxRole::Text => UIA_TextControlTypeId,
    }
}

impl IRawElementProviderSimple_Impl for Element_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading)
    }

    fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
        let role = self.get()?.role;
        let supported = match id {
            UIA_InvokePatternId => matches!(role, AxRole::Button | AxRole::Option),
            UIA_TogglePatternId => role == AxRole::Switch,
            UIA_SelectionItemPatternId => matches!(role, AxRole::Option | AxRole::Radio),
            _ => false,
        };
        if supported {
            self.to_object().cast()
        } else {
            none()
        }
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let n = self.get()?;
        Ok(match id {
            UIA_ControlTypePropertyId => int_var(control_type(n.role).0),
            UIA_NamePropertyId => bstr_var(&n.label),
            UIA_AutomationIdPropertyId => bstr_var(&self.key),
            UIA_FrameworkIdPropertyId => bstr_var(FRAMEWORK),
            UIA_LocalizedControlTypePropertyId if n.role == AxRole::Switch => bstr_var("toggle switch"),
            UIA_IsEnabledPropertyId | UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => bool_var(true),
            UIA_IsKeyboardFocusablePropertyId | UIA_HasKeyboardFocusPropertyId | UIA_IsOffscreenPropertyId => bool_var(false),
            _ => VARIANT::default(),
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        none()
    }
}

impl IRawElementProviderFragment_Impl for Element_Impl {
    fn Navigate(&self, dir: NavigateDirection) -> Result<IRawElementProviderFragment> {
        if dir == NavigateDirection_Parent {
            return root(HWND(self.hwnd as *mut _)).cast();
        }
        let (i, nodes) = self.node()?;
        let j = match dir {
            NavigateDirection_NextSibling => i + 1,
            NavigateDirection_PreviousSibling if i > 0 => i - 1,
            _ => return none(),
        };
        match nodes.get(j) {
            Some((key, _)) => Ok(element(self.hwnd, key)),
            None => none(),
        }
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.key.hash(&mut h);
        runtime_id(&[UiaAppendRuntimeId as i32, h.finish() as i32])
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        Ok(screen_rect(self.hwnd, self.get()?.rect))
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        // The drawn controls aren't focusable; the text field keeps focus.
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        root(HWND(self.hwnd as *mut _)).cast()
    }
}

impl IInvokeProvider_Impl for Element_Impl {
    fn Invoke(&self) -> Result<()> {
        self.press()
    }
}

impl IToggleProvider_Impl for Element_Impl {
    fn Toggle(&self) -> Result<()> {
        self.press()
    }

    fn ToggleState(&self) -> Result<ToggleState> {
        Ok(toggle_state(self.get()?.checked))
    }
}

impl ISelectionItemProvider_Impl for Element_Impl {
    fn Select(&self) -> Result<()> {
        if self.get()?.selected {
            return Ok(());
        }
        self.press()
    }

    fn AddToSelection(&self) -> Result<()> {
        self.Select()
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        Err(Error::from(HRESULT(UIA_E_INVALIDOPERATION as i32)))
    }

    fn IsSelected(&self) -> Result<BOOL> {
        Ok(self.get()?.selected.into())
    }

    fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
        none()
    }
}
