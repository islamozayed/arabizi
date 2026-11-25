//! VoiceOver support. The widget is custom-drawn, so the canvas reports the
//! core's accessibility nodes as `NSAccessibilityElement` children with
//! roles, labels and states, and pressing one clicks it.

use super::{after_event, with_widget, with_widget_ro, Canvas};
use franconvert_core::ui::access::{AxNode, AxRole};
use franconvert_core::ui::Target;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification, NSAccessibilityButtonRole,
    NSAccessibilityCheckBoxRole, NSAccessibilityElement, NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey,
    NSAccessibilityRadioButtonRole, NSAccessibilitySliderRole, NSAccessibilityStaticTextRole, NSAccessibilitySwitchSubrole, NSApplication,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSPoint, NSRect, NSString, NSSize};
use std::cell::{Cell, RefCell};

pub struct AxIvars {
    target: Cell<Option<Target>>,
}

define_class!(
    #[unsafe(super(NSAccessibilityElement))]
    #[name = "FCAXElement"]
    #[ivars = AxIvars]
    pub struct AxElement;

    impl AxElement {
        #[unsafe(method(accessibilityPerformPress))]
        fn perform_press(&self) -> objc2::runtime::Bool {
            let Some(t) = self.ivars().target.get() else { return objc2::runtime::Bool::NO };
            with_widget(|w| w.ax_press(t));
            after_event();
            objc2::runtime::Bool::YES
        }
    }
);

thread_local! {
    /// Elements from the last `accessibilityChildren` call, for hit testing.
    static ELEMENTS: RefCell<Vec<(NSRect, Retained<AxElement>)>> = const { RefCell::new(Vec::new()) };
}

fn element(node: &AxNode, frame: NSRect, parent: &Canvas) -> Retained<AxElement> {
    let this = AxElement::alloc().set_ivars(AxIvars { target: Cell::new(node.target) });
    let el: Retained<AxElement> = unsafe { msg_send![super(this), init] };
    unsafe {
        let role = match node.role {
            AxRole::Button | AxRole::Option => NSAccessibilityButtonRole,
            AxRole::Radio => NSAccessibilityRadioButtonRole,
            AxRole::Switch => NSAccessibilityCheckBoxRole,
            AxRole::Slider => NSAccessibilitySliderRole,
            AxRole::Text => NSAccessibilityStaticTextRole,
        };
        let _: () = msg_send![&*el, setAccessibilityRole: role];
        if node.role == AxRole::Switch {
            let _: () = msg_send![&*el, setAccessibilitySubrole: NSAccessibilitySwitchSubrole];
        }
        let mut label = node.label.clone();
        if node.role == AxRole::Option && node.selected {
            label.push_str(", selected");
        }
        let label = NSString::from_str(&label);
        if node.role == AxRole::Text {
            let _: () = msg_send![&*el, setAccessibilityValue: &*label];
        } else {
            let _: () = msg_send![&*el, setAccessibilityLabel: &*label];
        }
        match node.role {
            AxRole::Switch => {
                let v = NSNumber::new_i32(node.checked.unwrap_or(false) as i32);
                let _: () = msg_send![&*el, setAccessibilityValue: &*v];
            }
            AxRole::Radio => {
                let v = NSNumber::new_i32(node.selected as i32);
                let _: () = msg_send![&*el, setAccessibilityValue: &*v];
            }
            _ => {}
        }
        let _: () = msg_send![&*el, setAccessibilitySelected: node.selected];
        let _: () = msg_send![&*el, setAccessibilityFrame: frame];
        let _: () = msg_send![&*el, setAccessibilityParent: parent];
    }
    el
}

/// Screen-space frame of a canvas rect (canvas coordinates are flipped).
fn screen_frame(canvas: &Canvas, r: franconvert_core::ui::geom::Rect) -> NSRect {
    let local = NSRect::new(NSPoint::new(r.x as f64, r.y as f64), NSSize::new(r.w as f64, r.h as f64));
    let in_window = canvas.convertRect_toView(local, None);
    match canvas.window() {
        Some(w) => w.convertRectToScreen(in_window),
        None => in_window,
    }
}

pub fn children(canvas: &Canvas) -> Retained<NSArray<AnyObject>> {
    let nodes = with_widget_ro(|w| w.ax_nodes()).unwrap_or_default();
    let mut objs: Vec<Retained<AnyObject>> = Vec::new();
    let mut list = Vec::new();
    for n in &nodes {
        if n.rect.w <= 0.0 || n.rect.h <= 0.0 {
            continue;
        }
        let frame = screen_frame(canvas, n.rect);
        let el = element(n, frame, canvas);
        objs.push(unsafe { Retained::cast_unchecked(el.clone()) });
        list.push((frame, el));
    }
    ELEMENTS.with(|e| *e.borrow_mut() = list);
    NSArray::from_retained_slice(&objs)
}

/// The topmost element under a screen point.
pub fn hit_test(point: NSPoint) -> Option<Retained<AnyObject>> {
    ELEMENTS.with(|e| {
        e.borrow()
            .iter()
            .rev()
            .find(|(f, el)| {
                let inside = point.x >= f.origin.x && point.x < f.origin.x + f.size.width && point.y >= f.origin.y && point.y < f.origin.y + f.size.height;
                inside && el.ivars().target.get().is_some()
            })
            .map(|(_, el)| unsafe { Retained::cast_unchecked(el.clone()) })
    })
}

/// Speaks `text` now (the highlighted suggestion while arrowing).
pub fn announce(text: &str) {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let target: &AnyObject = match app.keyWindow() {
        Some(w) => unsafe { &*(Retained::as_ptr(&w) as *const AnyObject) },
        None => unsafe { &*(Retained::as_ptr(&app) as *const AnyObject) },
    };
    let high = NSNumber::new_isize(90); // NSAccessibilityPriorityHigh
    let msg = NSString::from_str(text);
    unsafe {
        let info = NSDictionary::<NSString, AnyObject>::from_slices(
            &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
            &[&*msg as &AnyObject, &*high as &AnyObject],
        );
        NSAccessibilityPostNotificationWithUserInfo(target, NSAccessibilityAnnouncementRequestedNotification, Some(&info));
    }
}

