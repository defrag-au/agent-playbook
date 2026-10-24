//! Pointer and touch input: locators first, coordinates second, with interpolated moves.
//!
//! Buttons and held modifiers are explicit, because a game distinguishes a right-click from a
//! click and a shift-drag from a drag. Drags and swipes are down, move, move, up — not a
//! `dragstart`/`drop` pair — because a canvas has pointer events, not drag-and-drop semantics.

use std::time::Duration;

use anyhow::{Context, Result};
use headless_chrome::browser::tab::point::Point;
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::Input;

use crate::probe::evaluate_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "middle" => Some(Self::Middle),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Middle => "middle",
        }
    }

    fn cdp(self) -> Input::MouseButton {
        match self {
            Self::Left => Input::MouseButton::Left,
            Self::Right => Input::MouseButton::Right,
            Self::Middle => Input::MouseButton::Middle,
        }
    }

    /// The CDP `buttons` bitmask for this button being held.
    fn mask(self) -> u32 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Click,
    Hover,
    Drag,
    Tap,
    Swipe,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "click" => Some(Self::Click),
            "hover" => Some(Self::Hover),
            "drag" => Some(Self::Drag),
            "tap" => Some(Self::Tap),
            "swipe" => Some(Self::Swipe),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Hover => "hover",
            Self::Drag => "drag",
            Self::Tap => "tap",
            Self::Swipe => "swipe",
        }
    }

    pub fn is_touch(self) -> bool {
        matches!(self, Self::Tap | Self::Swipe)
    }
}

#[derive(Debug, Clone)]
pub enum Target {
    Coordinates { x: f64, y: f64 },
    Selector(String),
}

#[derive(Debug, Clone)]
pub struct Action {
    pub kind: Kind,
    pub target: Target,
    pub button: Button,
    pub from: Option<(f64, f64)>,
}

impl Action {
    pub fn click(target: Target, button: Button) -> Self {
        Self {
            kind: Kind::Click,
            target,
            button,
            from: None,
        }
    }

    pub fn hover(target: Target) -> Self {
        Self {
            kind: Kind::Hover,
            target,
            button: Button::Left,
            from: None,
        }
    }

    pub fn drag(target: Target, from: Option<(f64, f64)>, button: Button) -> Self {
        Self {
            kind: Kind::Drag,
            target,
            button,
            from,
        }
    }

    pub fn tap(target: Target) -> Self {
        Self {
            kind: Kind::Tap,
            target,
            button: Button::Left,
            from: None,
        }
    }

    pub fn swipe(target: Target, from: Option<(f64, f64)>) -> Self {
        Self {
            kind: Kind::Swipe,
            target,
            button: Button::Left,
            from,
        }
    }
}

pub struct Pointer {
    pub x: f64,
    pub y: f64,
}

impl Default for Pointer {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

pub struct InputRecord {
    pub description: String,
    pub focus: String,
    pub scroll: String,
    pub locator: String,
}

pub fn dispatch(
    tab: &Tab,
    action: &Action,
    steps: u32,
    pointer: &mut Pointer,
    modifiers: u32,
    timeout: Duration,
) -> Result<InputRecord> {
    let (target, locator) = resolve(tab, &action.target, timeout)?;
    let steps = steps.max(1);

    let stamps = match action.kind {
        Kind::Hover => {
            move_mouse(tab, pointer, target, steps, modifiers, 0)?;
            vec![("mousemove", stamp(tab, "mousemove"))]
        }
        Kind::Click => {
            move_mouse(tab, pointer, target, steps, modifiers, 0)?;
            press(tab, target, action.button, modifiers)?;
            release(tab, target, action.button, modifiers)?;
            vec![("mousedown", stamp(tab, "mousedown"))]
        }
        Kind::Drag => {
            let start = origin(action, pointer);
            move_mouse(tab, pointer, start, 1, modifiers, 0)?;
            press(tab, start, action.button, modifiers)?;
            let down = stamp(tab, "mousedown");
            move_mouse(tab, pointer, target, steps, modifiers, action.button.mask())?;
            release(tab, target, action.button, modifiers)?;
            vec![("mousedown", down), ("mouseup", stamp(tab, "mouseup"))]
        }
        Kind::Tap => {
            touch(
                tab,
                &Input::DispatchTouchEventTypeOption::TouchStart,
                target,
                modifiers,
            )?;
            let down = stamp(tab, "touchstart");
            touch(
                tab,
                &Input::DispatchTouchEventTypeOption::TouchEnd,
                target,
                modifiers,
            )?;
            vec![("touchstart", down), ("touchend", stamp(tab, "touchend"))]
        }
        Kind::Swipe => {
            let start = origin(action, pointer);
            touch(
                tab,
                &Input::DispatchTouchEventTypeOption::TouchStart,
                start,
                modifiers,
            )?;
            let down = stamp(tab, "touchstart");
            for step in 1..=steps {
                let t = f64::from(step) / f64::from(steps);
                let x = start.x + (target.x - start.x) * t;
                let y = start.y + (target.y - start.y) * t;
                touch(
                    tab,
                    &Input::DispatchTouchEventTypeOption::TouchMove,
                    Point { x, y },
                    modifiers,
                )?;
            }
            touch(
                tab,
                &Input::DispatchTouchEventTypeOption::TouchEnd,
                target,
                modifiers,
            )?;
            vec![("touchstart", down), ("touchend", stamp(tab, "touchend"))]
        }
    };

    pointer.x = target.x;
    pointer.y = target.y;

    Ok(InputRecord {
        description: describe(action, &locator, target, steps, &stamps),
        focus: focus(tab),
        scroll: scroll(tab),
        locator,
    })
}

fn resolve(tab: &Tab, target: &Target, timeout: Duration) -> Result<(Point, String)> {
    match target {
        Target::Coordinates { x, y } => Ok((Point { x: *x, y: *y }, "coordinates".to_owned())),
        Target::Selector(selector) => Ok((centre(tab, selector, timeout)?, selector.clone())),
    }
}

fn origin(action: &Action, pointer: &Pointer) -> Point {
    action.from.map_or(
        Point {
            x: pointer.x,
            y: pointer.y,
        },
        |(x, y)| Point { x, y },
    )
}

fn move_mouse(
    tab: &Tab,
    pointer: &mut Pointer,
    target: Point,
    steps: u32,
    modifiers: u32,
    buttons: u32,
) -> Result<()> {
    for step in 1..=steps {
        let t = f64::from(step) / f64::from(steps);
        let x = pointer.x + (target.x - pointer.x) * t;
        let y = pointer.y + (target.y - pointer.y) * t;
        mouse(
            tab,
            &Input::DispatchMouseEventTypeOption::MouseMoved,
            Point { x, y },
            modifiers,
            None,
            buttons,
        )?;
    }
    pointer.x = target.x;
    pointer.y = target.y;
    Ok(())
}

fn press(tab: &Tab, at: Point, button: Button, modifiers: u32) -> Result<()> {
    mouse(
        tab,
        &Input::DispatchMouseEventTypeOption::MousePressed,
        at,
        modifiers,
        Some(button),
        button.mask(),
    )
}

fn release(tab: &Tab, at: Point, button: Button, modifiers: u32) -> Result<()> {
    mouse(
        tab,
        &Input::DispatchMouseEventTypeOption::MouseReleased,
        at,
        modifiers,
        Some(button),
        0,
    )
}

fn mouse(
    tab: &Tab,
    kind: &Input::DispatchMouseEventTypeOption,
    at: Point,
    modifiers: u32,
    button: Option<Button>,
    buttons: u32,
) -> Result<()> {
    let (button, buttons, click_count) = match kind {
        Input::DispatchMouseEventTypeOption::MousePressed => (button, Some(buttons), Some(1)),
        Input::DispatchMouseEventTypeOption::MouseReleased => (button, Some(0), Some(1)),
        _ => (None, (buttons != 0).then_some(buttons), None),
    };

    tab.call_method(Input::DispatchMouseEvent {
        Type: kind.clone(),
        x: at.x,
        y: at.y,
        modifiers: Some(modifiers),
        timestamp: None,
        button: button.map(Button::cdp),
        buttons,
        click_count,
        force: None,
        tangential_pressure: None,
        tilt_x: None,
        tilt_y: None,
        twist: None,
        delta_x: None,
        delta_y: None,
        pointer_Type: None,
    })?;
    Ok(())
}

fn touch(
    tab: &Tab,
    kind: &Input::DispatchTouchEventTypeOption,
    at: Point,
    modifiers: u32,
) -> Result<()> {
    let points = if matches!(
        kind,
        Input::DispatchTouchEventTypeOption::TouchStart
            | Input::DispatchTouchEventTypeOption::TouchMove
    ) {
        vec![Input::TouchPoint {
            x: at.x,
            y: at.y,
            radius_x: Some(1.0),
            radius_y: Some(1.0),
            rotation_angle: None,
            force: Some(1.0),
            tangential_pressure: None,
            tilt_x: None,
            tilt_y: None,
            twist: None,
            id: Some(0.0),
        }]
    } else {
        Vec::new()
    };

    tab.call_method(Input::DispatchTouchEvent {
        Type: kind.clone(),
        touch_points: points,
        modifiers: Some(modifiers),
        timestamp: None,
    })?;
    Ok(())
}

fn describe(
    action: &Action,
    locator: &str,
    target: Point,
    steps: u32,
    stamps: &[(&str, Option<f64>)],
) -> String {
    let button = if matches!(action.kind, Kind::Click | Kind::Drag) && action.button != Button::Left
    {
        format!("{} ", action.button.name())
    } else {
        String::new()
    };
    let movement = if matches!(action.kind, Kind::Drag | Kind::Swipe) {
        let from = action.from.map_or_else(
            || "last position".to_owned(),
            |(x, y)| format!("{x:.0},{y:.0}"),
        );
        format!("{from} -> {:.0},{:.0}", target.x, target.y)
    } else {
        format!("{:.0},{:.0}", target.x, target.y)
    };
    let kind = match action.kind {
        Kind::Tap => "touch tap",
        Kind::Swipe => "touch swipe",
        other => other.name(),
    };
    let stamps = stamps
        .iter()
        .filter_map(|(event, frame)| frame.map(|frame| format!("{event} @ frame {frame:.0}")))
        .collect::<Vec<_>>()
        .join(", ");
    let stamps = if stamps.is_empty() {
        String::new()
    } else {
        format!(" · {stamps}")
    };
    format!(
        "{button}{kind} ({locator}) {movement} · {steps} step{}{stamps}",
        if steps == 1 { "" } else { "s" }
    )
}

fn stamp(tab: &Tab, kind: &str) -> Option<f64> {
    let expression = format!(
        "(() => {{ const log = window.__inputLog || []; \
         for (let i = log.length - 1; i >= 0; i--) {{ \
           if (log[i].type === {kind:?}) {{ const f = log[i].frame; \
             return f === null || f === undefined ? 'null' : String(f); }} \
         }} return 'null'; }})()"
    );
    evaluate_text(tab, &expression).parse().ok()
}

fn centre(tab: &Tab, selector: &str, timeout: Duration) -> Result<Point> {
    let element = tab
        .wait_for_element_with_custom_timeout(selector, timeout)
        .with_context(|| format!("find {selector}"))?;
    let _ = element.scroll_into_view();
    let viewport = element
        .get_box_model()
        .with_context(|| format!("measure {selector}"))?
        .border_viewport();
    Ok(Point {
        x: viewport.x + viewport.width / 2.0,
        y: viewport.y + viewport.height / 2.0,
    })
}

pub fn focus(tab: &Tab) -> String {
    evaluate_text(
        tab,
        "(() => { const e = document.activeElement; if (!e) return 'none'; \
         return e.tagName.toLowerCase() + (e.id ? '#' + e.id : ''); })()",
    )
}

pub fn scroll(tab: &Tab) -> String {
    let value = evaluate_text(tab, "[window.scrollX, window.scrollY].join(',')");
    if value == "0,0" {
        "none".to_owned()
    } else {
        value
    }
}
