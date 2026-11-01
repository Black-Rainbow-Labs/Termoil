//! Keyboard, IME and mouse input for a window. Keys are checked against the bindings first (an
//! action runs through `App::run_action`), then encoded for the focused pane's keyboard mode.
//! Mouse buttons report to the program when it asked for mouse tracking (shift overrides), and
//! select text otherwise.

use std::time::{Duration, Instant};

use serde_json::Map;
use terminus_input::{
    Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind, MouseTracking, NamedKey, RawKey, encode_focus, encode_key,
    encode_mouse,
};
use terminus_vt::modes::Mode;
use terminus_vt::selection::{Selection, SelectionMode};
use winit::event::{ElementState, Ime, MouseScrollDelta};

use crate::app::App;
use crate::gui::window::WinState;
use crate::inspect;
use crate::keymap::{Bound, Lookup, event_to_chord, lookup};
use crate::model::PaneId;
use crate::view;

/// Two clicks closer than this are a double click.
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// The focused pane of a window.
pub fn focus_of(ws: &WinState, app: &App) -> Option<PaneId> {
    let wi = app.model.window_index(ws.id)?;
    Some(app.model.windows[wi].tab().focus)
}

/// Handles one raw key: bindings first, then the terminal.
pub fn raw_key(ws: &mut WinState, app: &mut App, bound: &[Bound], raw: &RawKey) {
    for ev in ws.normalizer.feed(raw) {
        if ev.kind != terminus_input::KeyEventKind::Release && binding(ws, app, bound, &ev) {
            continue;
        }
        if let Some(pane) = focus_of(ws, app) {
            send_key(app, pane, &ev);
        }
    }
}

/// Checks a press against the bindings. Returns true when the key was taken (it ran an action or
/// is part of a chained binding).
fn binding(ws: &mut WinState, app: &mut App, bound: &[Bound], ev: &KeyEvent) -> bool {
    if matches!(ev.key, Key::Named(n) if n.is_modifier_key()) {
        return false;
    }
    let Some(chord) = event_to_chord(ev) else { return false };
    ws.chords.push(chord);
    match lookup(bound, &ws.chords) {
        Lookup::Action(name, args) => {
            ws.chords.clear();
            let _ = app.run_action(&terminus_protocol::ActionCall { action: name, args });
            true
        }
        Lookup::Pending => true,
        Lookup::None => {
            ws.chords.clear();
            false
        }
    }
}

/// Encodes a key for the pane's keyboard mode and writes it, returning the view to the bottom.
pub fn send_key(app: &mut App, pane: PaneId, ev: &KeyEvent) {
    let Some(p) = app.panes.get_mut(&pane) else { return };
    let modes = inspect::keyboard_modes(&p.session.lock());
    let mut out = Vec::new();
    encode_key(ev, &modes, &mut out);
    if out.is_empty() {
        return;
    }
    if p.scroll != 0 {
        p.scroll = 0;
    }
    let _ = p.session.write(&out);
    app.redraw_pane(pane);
}

/// Handles IME events: the preedit is drawn at the cursor and never sent; a commit is sent as text.
pub fn ime(ws: &mut WinState, app: &mut App, ev: Ime) {
    match ev {
        Ime::Preedit(text, _) => ws.preedit = (!text.is_empty()).then_some(text),
        Ime::Commit(text) => {
            ws.preedit = None;
            if let Some(pane) = focus_of(ws, app)
                && let Some(p) = app.panes.get_mut(&pane)
            {
                p.scroll = 0;
                let _ = p.session.write(text.as_bytes());
            }
        }
        Ime::Disabled => ws.preedit = None,
        Ime::Enabled => {}
    }
    ws.window.request_redraw();
}

/// Window focus changed: the hollow cursor, and a focus report when the program asked for one.
pub fn focus(ws: &mut WinState, app: &mut App, focused: bool) {
    ws.focused = focused;
    ws.mods = winit::keyboard::ModifiersState::empty();
    if let Some(pane) = focus_of(ws, app)
        && let Some(p) = app.panes.get(&pane)
        && p.session.lock().mode(Mode::FocusEvents)
    {
        let mut out = Vec::new();
        encode_focus(focused, &mut out);
        let _ = p.session.write(&out);
    }
    ws.window.request_redraw();
}

/// The absolute row of a visible row of a pane.
fn absolute_row(app: &App, pane: PaneId, row: u16) -> usize {
    let p = &app.panes[&pane];
    let t = p.session.lock();
    t.screen().pages.active_y(0).saturating_sub(p.scroll) + row as usize
}

/// True when the program wants mouse reports and shift is not held.
fn tracking(ws: &WinState, app: &App, pane: PaneId) -> Option<terminus_input::MouseModes> {
    if ws.mods.shift_key() {
        return None;
    }
    let m = inspect::mouse_modes(&app.panes.get(&pane)?.session.lock());
    (m.tracking != MouseTracking::Off).then_some(m)
}

/// Sends a mouse report for a pane; returns whether the program took it.
fn report(
    ws: &mut WinState, app: &App, pane: PaneId, kind: MouseEventKind, button: MouseButton, col: u16, row: u16,
    px: (u32, u32),
) -> bool {
    let Some(modes) = tracking(ws, app, pane) else { return false };
    let ev = MouseEvent {
        kind,
        button,
        col: col.into(),
        row: row.into(),
        pixel_x: px.0,
        pixel_y: px.1,
        mods: crate::gui::input::modifiers(ws.mods),
    };
    let mut out = Vec::new();
    if encode_mouse(&ev, &modes, ws.mouse.last_report, &mut out) {
        ws.mouse.last_report = Some((col.into(), row.into()));
        let _ = app.panes[&pane].session.write(&out);
    }
    true
}

/// The pointer moved: extend a selection being dragged, or report motion.
pub fn cursor_moved(ws: &mut WinState, app: &mut App, x: f64, y: f64) {
    ws.mouse.pos = (x, y);
    let Some((pane, col, row, px)) = ws.pane_at(x, y) else { return };
    if let Some(sel_pane) = ws.mouse.selecting {
        if sel_pane == pane {
            let ay = absolute_row(app, pane, row);
            let p = app.panes.get_mut(&pane).expect("pane under pointer");
            if let Some(sel) = &p.selection {
                sel.update(&mut p.session.lock().screen_mut().pages, ay, col);
            }
            ws.window.request_redraw();
        }
        return;
    }
    let button = if ws.mouse.buttons & 1 != 0 {
        MouseButton::Left
    } else if ws.mouse.buttons & 2 != 0 {
        MouseButton::Middle
    } else if ws.mouse.buttons & 4 != 0 {
        MouseButton::Right
    } else {
        MouseButton::None
    };
    report(ws, app, pane, MouseEventKind::Motion, button, col, row, px);
}

/// A mouse button went down or up.
pub fn button(ws: &mut WinState, app: &mut App, state: ElementState, b: winit::event::MouseButton) {
    let (x, y) = ws.mouse.pos;
    if state == ElementState::Pressed && y < ws.tab_bar_h as f64 {
        click_tab(ws, app, x);
        return;
    }
    let Some((pane, col, row, px)) = ws.pane_at(x, y) else { return };
    let (mb, bit) = match b {
        winit::event::MouseButton::Left => (MouseButton::Left, 1),
        winit::event::MouseButton::Middle => (MouseButton::Middle, 2),
        winit::event::MouseButton::Right => (MouseButton::Right, 4),
        winit::event::MouseButton::Back => (MouseButton::Back, 8),
        winit::event::MouseButton::Forward => (MouseButton::Forward, 16),
        winit::event::MouseButton::Other(_) => return,
    };
    let pressed = state == ElementState::Pressed;
    if pressed {
        ws.mouse.buttons |= bit;
        if focus_of(ws, app) != Some(pane) {
            let _ = crate::actions::focus_pane(app, pane);
        }
    } else {
        ws.mouse.buttons &= !bit;
    }
    let kind = if pressed { MouseEventKind::Press } else { MouseEventKind::Release };
    if report(ws, app, pane, kind, mb, col, row, px) {
        return;
    }
    match (mb, pressed) {
        (MouseButton::Left, true) => start_selection(ws, app, pane, col, row),
        (MouseButton::Left, false) => finish_selection(ws, app),
        (MouseButton::Right, true) => right_click(app, pane),
        (MouseButton::Middle, true) if !cfg!(windows) => {
            let _ = app.run_action(&terminus_protocol::ActionCall { action: "paste".into(), args: Map::new() });
        }
        _ => {}
    }
}

/// Starts a selection: one click by cell, two by word, three by line, alt for a rectangle.
fn start_selection(ws: &mut WinState, app: &mut App, pane: PaneId, col: u16, row: u16) {
    let ay = absolute_row(app, pane, row);
    let count = match ws.mouse.last_click {
        Some((at, cell, n)) if at.elapsed() < MULTI_CLICK && cell == (ay, col) => n % 3 + 1,
        _ => 1,
    };
    ws.mouse.last_click = Some((Instant::now(), (ay, col), count));
    let mode = match count {
        _ if ws.mods.alt_key() => SelectionMode::Block,
        2 => SelectionMode::Word,
        3 => SelectionMode::Line,
        _ => SelectionMode::Char,
    };
    let p = app.panes.get_mut(&pane).expect("pane under pointer");
    view::clear_selection(p);
    let sel = Selection::new(&mut p.session.lock().screen_mut().pages, mode, ay, col);
    p.selection = Some(sel);
    p.selection_empty = count == 1 && mode == SelectionMode::Char;
    ws.mouse.selecting = Some(pane);
    ws.window.request_redraw();
}

/// Ends a drag: a click without movement leaves no selection; otherwise copy when the
/// configuration asks for copy on select.
fn finish_selection(ws: &mut WinState, app: &mut App) {
    let Some(pane) = ws.mouse.selecting.take() else { return };
    let moved = {
        let Some(p) = app.panes.get(&pane) else { return };
        let t = p.session.lock();
        p.selection.as_ref().and_then(|s| s.range(&t.screen().pages, "")).is_some_and(|r| r.start != r.end)
    };
    let p = app.panes.get_mut(&pane).expect("pane checked");
    if p.selection_empty && !moved {
        view::clear_selection(p);
    }
    p.selection_empty = false;
    if app.config.clipboard.copy_on_select
        && let Some(text) = view::selected_text(app, pane)
    {
        app.clipboard.set(&text);
    }
    ws.window.request_redraw();
}

/// Right click copies the selection when there is one, and pastes otherwise.
fn right_click(app: &mut App, pane: PaneId) {
    let name = if view::selected_text(app, pane).is_some() { "copy" } else { "paste" };
    let mut args = Map::new();
    args.insert("pane".into(), pane.into());
    let _ = app.run_action(&terminus_protocol::ActionCall { action: name.into(), args: args.clone() });
    if name == "copy" {
        let _ = app.run_action(&terminus_protocol::ActionCall { action: "clear-selection".into(), args });
    }
}

/// A click in the tab bar switches to the tab under it.
fn click_tab(ws: &mut WinState, app: &mut App, x: f64) {
    let (cw, _) = ws.cell();
    let col = (x / cw as f64) as u16;
    let Some(i) = ws.tab_spans.iter().position(|s| col >= s.start && col < s.end) else { return };
    let Some(wi) = app.model.window_index(ws.id) else { return };
    let focus = app.model.windows[wi].tabs[i].focus;
    let _ = crate::actions::focus_pane(app, focus);
}

/// The wheel: reports to a tracking program, arrow keys on the alternate screen (mode 1007), and
/// scrollback otherwise.
pub fn wheel(ws: &mut WinState, app: &mut App, delta: MouseScrollDelta) {
    let (x, y) = ws.mouse.pos;
    let Some((pane, col, row, px)) = ws.pane_at(x, y) else { return };
    let (_, ch) = ws.cell();
    let lines = match delta {
        MouseScrollDelta::LineDelta(_, dy) => (dy * app.config.mouse.scroll_multiplier as f32).round() as i32,
        MouseScrollDelta::PixelDelta(p) => (p.y / ch as f64).round() as i32,
    };
    if lines == 0 {
        return;
    }
    let b = if lines > 0 { MouseButton::WheelUp } else { MouseButton::WheelDown };
    if tracking(ws, app, pane).is_some() {
        for _ in 0..lines.unsigned_abs().min(10) {
            ws.mouse.last_report = None;
            report(ws, app, pane, MouseEventKind::Press, b, col, row, px);
        }
        return;
    }
    let alt_arrows = app.panes.get(&pane).is_some_and(|p| {
        let t = p.session.lock();
        t.is_alt_screen() && t.mode(Mode::AlternateScroll)
    });
    if alt_arrows {
        let key = if lines > 0 { NamedKey::Up } else { NamedKey::Down };
        for _ in 0..lines.unsigned_abs() {
            send_key(app, pane, &KeyEvent::press(Key::Named(key), terminus_input::Modifiers::NONE));
        }
        return;
    }
    view::scroll_by(app, pane, lines as isize);
}
