//! Pointer input: locators first, coordinates second, with interpolated moves.

use std::time::Duration;

use anyhow::{Context, Result};
use headless_chrome::browser::tab::point::Point;
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::Input;

use crate::probe::evaluate_text;

#[derive(Debug, Clone)]
pub enum Action {
    Click { x: f64, y: f64 },
    Hover { x: f64, y: f64 },
    ClickSelector(String),
    HoverSelector(String),
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
    timeout: Duration,
) -> Result<InputRecord> {
    let (kind, target, locator) = match action {
        Action::Click { x, y } => ("click", Point { x: *x, y: *y }, "coordinates".to_owned()),
        Action::Hover { x, y } => ("hover", Point { x: *x, y: *y }, "coordinates".to_owned()),
        Action::ClickSelector(selector) => {
            ("click", centre(tab, selector, timeout)?, selector.clone())
        }
        Action::HoverSelector(selector) => {
            ("hover", centre(tab, selector, timeout)?, selector.clone())
        }
    };

    let steps = steps.max(1);
    for step in 1..=steps {
        let t = f64::from(step) / f64::from(steps);
        let x = pointer.x + (target.x - pointer.x) * t;
        let y = pointer.y + (target.y - pointer.y) * t;
        mouse(tab, &Input::DispatchMouseEventTypeOption::MouseMoved, x, y)?;
    }
    pointer.x = target.x;
    pointer.y = target.y;

    if kind == "click" {
        mouse(
            tab,
            &Input::DispatchMouseEventTypeOption::MousePressed,
            target.x,
            target.y,
        )?;
        mouse(
            tab,
            &Input::DispatchMouseEventTypeOption::MouseReleased,
            target.x,
            target.y,
        )?;
    }

    Ok(InputRecord {
        description: format!(
            "{kind} ({locator}) at {:.0},{:.0} · {steps} step{}",
            target.x,
            target.y,
            if steps == 1 { "" } else { "s" }
        ),
        focus: focus(tab),
        scroll: scroll(tab),
        locator,
    })
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

fn mouse(tab: &Tab, kind: &Input::DispatchMouseEventTypeOption, x: f64, y: f64) -> Result<()> {
    let (button, buttons, click_count) = match kind {
        Input::DispatchMouseEventTypeOption::MousePressed => {
            (Some(Input::MouseButton::Left), Some(1), Some(1))
        }
        Input::DispatchMouseEventTypeOption::MouseReleased => {
            (Some(Input::MouseButton::Left), Some(0), Some(1))
        }
        _ => (None, None, None),
    };

    tab.call_method(Input::DispatchMouseEvent {
        Type: kind.clone(),
        x,
        y,
        modifiers: None,
        timestamp: None,
        button,
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
