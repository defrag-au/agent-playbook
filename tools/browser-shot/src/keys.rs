//! Keyboard input by `code`, with held modifiers and page-side event records.
//!
//! The frame, focus and scroll of an event are read **in the page** by a preloaded listener.
//! Reading them over CDP before or after the dispatch adds ~6 frames of latency per call, which
//! would make a frame-precise hold meaningless.

use anyhow::Result;
use headless_chrome::browser::tab::Tab;
use headless_chrome::protocol::cdp::Input;
use serde::Deserialize;

pub struct KeyEvent {
    pub frame: Option<f64>,
    pub focus: String,
    pub scroll: String,
}

pub fn key(tab: &Tab, code: &str, down: bool, modifiers: &mut u32) -> Result<()> {
    let info = key_info(code, down);
    if let Some(bit) = info.modifier {
        if down {
            *modifiers |= bit;
        } else {
            *modifiers &= !bit;
        }
    }

    tab.call_method(Input::DispatchKeyEvent {
        Type: if down {
            Input::DispatchKeyEventTypeOption::KeyDown
        } else {
            Input::DispatchKeyEventTypeOption::KeyUp
        },
        modifiers: Some(*modifiers),
        timestamp: None,
        text: info.text,
        unmodified_text: None,
        key_identifier: None,
        code: Some(code.to_owned()),
        key: Some(info.key),
        windows_virtual_key_code: Some(info.virtual_key),
        native_virtual_key_code: Some(info.virtual_key),
        auto_repeat: Some(false),
        is_keypad: Some(false),
        is_system_key: Some(false),
        location: None,
        commands: None,
    })?;
    Ok(())
}

pub fn last_event(tab: &Tab, code: &str, down: bool) -> Option<KeyEvent> {
    let kind = if down { "keydown" } else { "keyup" };
    let expression = format!(
        "(() => {{ const log = window.__inputLog || []; \
         for (let i = log.length - 1; i >= 0; i--) {{ \
           if (log[i].type === {kind:?} && log[i].code === {code:?}) return JSON.stringify(log[i]); \
         }} return 'null'; }})()"
    );
    let object = tab.evaluate(&expression, false).ok()?;
    let text = object.value?.as_str()?.to_owned();
    if text == "null" {
        return None;
    }
    let entry: LogEntry = serde_json::from_str(&text).ok()?;
    if entry.kind != kind || entry.code != code {
        return None;
    }

    let scroll = match entry.after_scroll_y {
        Some(after) if (after - entry.scroll_y).abs() > 0.5 => {
            format!("{} -> {after}", entry.scroll_y)
        }
        Some(after) if after.abs() < 0.5 => "none".to_owned(),
        Some(after) => format!("{after}"),
        None if entry.scroll_y.abs() < 0.5 => "none".to_owned(),
        None => format!("{}", entry.scroll_y),
    };

    Some(KeyEvent {
        frame: entry.frame,
        focus: entry.focus,
        scroll,
    })
}

#[derive(Deserialize)]
struct LogEntry {
    #[serde(rename = "type")]
    kind: String,
    code: String,
    frame: Option<f64>,
    focus: String,
    #[serde(rename = "scrollY")]
    scroll_y: f64,
    #[serde(rename = "afterScrollY")]
    after_scroll_y: Option<f64>,
}

struct KeyInfo {
    key: String,
    virtual_key: u32,
    modifier: Option<u32>,
    text: Option<String>,
}

fn key_info(code: &str, down: bool) -> KeyInfo {
    let (key, virtual_key, modifier) = match code {
        "ShiftLeft" | "ShiftRight" => ("Shift".to_owned(), 16, Some(8)),
        "ControlLeft" | "ControlRight" => ("Control".to_owned(), 17, Some(2)),
        "AltLeft" | "AltRight" => ("Alt".to_owned(), 18, Some(1)),
        "MetaLeft" | "MetaRight" => ("Meta".to_owned(), 91, Some(4)),
        "Space" => (" ".to_owned(), 32, None),
        "Enter" => ("Enter".to_owned(), 13, None),
        "Escape" => ("Escape".to_owned(), 27, None),
        "Tab" => ("Tab".to_owned(), 9, None),
        "Backspace" => ("Backspace".to_owned(), 8, None),
        "ArrowLeft" => ("ArrowLeft".to_owned(), 37, None),
        "ArrowUp" => ("ArrowUp".to_owned(), 38, None),
        "ArrowRight" => ("ArrowRight".to_owned(), 39, None),
        "ArrowDown" => ("ArrowDown".to_owned(), 40, None),
        other => {
            if let Some(letter) = other.strip_prefix("Key").filter(|letter| letter.len() == 1) {
                let letter = letter.to_ascii_lowercase();
                let virtual_key = letter.to_ascii_uppercase().as_bytes()[0] as u32;
                (letter, virtual_key, None)
            } else if let Some(digit) = other.strip_prefix("Digit").filter(|digit| digit.len() == 1)
            {
                let virtual_key = 48 + digit.parse::<u32>().unwrap_or(0);
                (digit.to_owned(), virtual_key, None)
            } else if let Some(number) = other.strip_prefix('F').and_then(|n| n.parse().ok()) {
                let number: u32 = number;
                if (1..=12).contains(&number) {
                    (other.to_owned(), 111 + number, None)
                } else {
                    (other.to_owned(), 0, None)
                }
            } else {
                (other.to_owned(), 0, None)
            }
        }
    };

    let printable = down && key.chars().count() == 1 && !key.chars().next().unwrap().is_control();
    let text = printable.then(|| key.clone());

    KeyInfo {
        key,
        virtual_key,
        modifier,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_carry_their_code_and_virtual_key() {
        let info = key_info("KeyW", true);
        assert_eq!(info.key, "w");
        assert_eq!(info.virtual_key, 87);
        assert_eq!(info.text.as_deref(), Some("w"));
        assert!(info.modifier.is_none());
    }

    #[test]
    fn modifiers_are_named_and_flagged() {
        let info = key_info("ShiftLeft", true);
        assert_eq!(info.key, "Shift");
        assert_eq!(info.virtual_key, 16);
        assert_eq!(info.modifier, Some(8));
        assert!(info.text.is_none());
    }

    #[test]
    fn arrows_are_not_printable() {
        let info = key_info("ArrowDown", true);
        assert_eq!(info.key, "ArrowDown");
        assert_eq!(info.virtual_key, 40);
        assert!(info.text.is_none());
    }
}
