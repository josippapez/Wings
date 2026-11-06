//! Mouse input: cell hits, mouse reports, selection, link clicks and the wheel.

use gpui::{
    Context, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollEvent, Window, px,
};
use wings_terminal::grid::{Scroll, SelectionType, Side, TermMode};

use super::{Drag, TerminalView, links::link_at, worker::Command};

/// A viewport cell and the half of it the pointer is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub(super) row: usize,
    pub(super) col: usize,
    pub(super) side: Side,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MouseAction {
    Press,
    Release,
    Move,
}

const NO_BUTTON: u8 = 3;
const WHEEL_UP: u8 = 64;
const WHEEL_DOWN: u8 = 65;

fn button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

/// The same reports as the desktop terminal (xterm.js 6): modes 1000/1002/1003, SGR or the default encoding.
fn mouse_report(
    code: u8,
    action: MouseAction,
    hit: Hit,
    mods: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let allowed = match action {
        MouseAction::Move if code == NO_BUTTON => mode.contains(TermMode::MOUSE_MOTION),
        MouseAction::Move => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        _ => mode.intersects(TermMode::MOUSE_MODE),
    };
    if !allowed {
        return None;
    }
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let mut cb = code
        | if mods.shift { 4 } else { 0 }
        | if mods.alt { 8 } else { 0 }
        | if mods.control { 16 } else { 0 };
    if action == MouseAction::Move {
        cb |= 32;
    } else if action == MouseAction::Release && !sgr {
        cb |= NO_BUTTON;
    }
    let (col, row) = (hit.col + 1, hit.row + 1);
    if sgr {
        let end = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{cb};{col};{row}{end}").into_bytes());
    }
    let encoded = [cb as usize + 32, col + 32, row + 32];
    if encoded.iter().any(|value| *value > 255) {
        return None;
    }
    Some([b"\x1b[M".as_slice(), &encoded.map(|value| value as u8)].concat())
}

impl TerminalView {
    /// The cell under `position`, clamped to the grid so a drag past the edge keeps selecting.
    fn hit(&self, position: Point<Pixels>) -> Option<Hit> {
        let bounds = self.bounds?;
        let (rows, cols) = (self.snapshot.rows, self.snapshot.cols);
        if rows == 0 || cols == 0 {
            return None;
        }
        let width = self.cell_width.as_f32();
        let x = (position.x - bounds.left()).as_f32().max(0.0);
        let y = (position.y - bounds.top()).as_f32().max(0.0);
        let col = ((x / width) as usize).min(cols - 1);
        Some(Hit {
            row: ((y / self.line_height) as usize).min(rows - 1),
            col,
            side: if x - col as f32 * width < width / 2.0 {
                Side::Left
            } else {
                Side::Right
            },
        })
    }

    /// The desktop terminal lets Shift force a selection over mouse reports, except on macOS.
    fn reports_mouse(&self, mods: Modifiers) -> bool {
        self.snapshot.mode.intersects(TermMode::MOUSE_MODE)
            && (cfg!(target_os = "macos") || !mods.shift)
    }

    fn report(
        &mut self,
        code: u8,
        action: MouseAction,
        hit: Hit,
        mods: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if let Some(bytes) = mouse_report(code, action, hit, mods, self.snapshot.mode) {
            self.send(Command::Input(bytes), cx);
        }
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        window.request_virtual_keyboard();
        cx.notify();
        let Some(hit) = self.hit(event.position) else {
            return;
        };
        self.last_hit = Some(hit);
        // Only a Cmd or Ctrl click opens a link, so a plain click in a mouse-mode program never does.
        if event.button == MouseButton::Left
            && (event.modifiers.platform || event.modifiers.control)
            && let Some(link) = link_at(&self.snapshot, hit.row, hit.col)
        {
            cx.open_url(&link.url);
            return;
        }
        if self.reports_mouse(event.modifiers) {
            if let Some(code) = button_code(event.button) {
                self.drag = Some(Drag::Report(code));
                self.report(code, MouseAction::Press, hit, event.modifiers, cx);
            }
        } else if event.button == MouseButton::Left {
            let ty = match event.click_count {
                0 | 1 => SelectionType::Simple,
                2 => SelectionType::Semantic,
                _ => SelectionType::Lines,
            };
            self.drag = Some(Drag::Select);
            self.send(Command::SelectFrom(hit, ty), cx);
        }
    }

    /// Registered on the window, so a drag keeps going outside the terminal.
    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(hit) = self.hit(event.position) else {
            return;
        };
        if self.last_hit == Some(hit) {
            return;
        }
        match self.drag {
            Some(Drag::Select) if event.dragging() => {
                self.last_hit = Some(hit);
                self.send(Command::SelectTo(hit), cx);
            }
            Some(Drag::Report(code)) => {
                self.last_hit = Some(hit);
                self.report(code, MouseAction::Move, hit, event.modifiers, cx);
            }
            None => {
                self.last_hit = Some(hit);
                let inside = self
                    .bounds
                    .is_some_and(|bounds| bounds.contains(&event.position));
                let link = inside
                    .then(|| link_at(&self.snapshot, hit.row, hit.col))
                    .flatten();
                if link != self.link {
                    self.link = link;
                    cx.notify();
                }
                if inside && self.reports_mouse(event.modifiers) {
                    self.report(NO_BUTTON, MouseAction::Move, hit, event.modifiers, cx);
                }
            }
            _ => {}
        }
    }

    pub(super) fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        match self.drag.take() {
            Some(Drag::Report(code)) => {
                if let Some(hit) = self.hit(event.position) {
                    self.report(code, MouseAction::Release, hit, event.modifiers, cx);
                }
            }
            Some(Drag::Select) if super::look(cx).copy_on_select => self.send(Command::Copy, cx),
            _ => {}
        }
    }

    pub(super) fn scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let lines = self.scroll_rest
            + event.delta.pixel_delta(px(self.line_height)).y.as_f32() / self.line_height;
        let whole = lines.trunc();
        self.scroll_rest = lines - whole;
        let whole = whole as i32;
        cx.stop_propagation();
        if whole == 0 {
            return;
        }
        let report = self.reports_mouse(event.modifiers);
        if let Some(hit) = self.hit(event.position)
            && let Some(command) =
                wheel_command(whole, hit, event.modifiers, self.snapshot.mode, report)
        {
            self.send(command, cx);
        }
    }
}

/// `lines` > 0 is towards older output. The wheel goes to a program that asked for mouse reports,
/// moves through an alternate-screen program with arrow keys, or else scrolls the history.
fn wheel_command(
    lines: i32,
    hit: Hit,
    mods: Modifiers,
    mode: TermMode,
    report: bool,
) -> Option<Command> {
    let count = lines.unsigned_abs() as usize;
    if report {
        let code = if lines > 0 { WHEEL_UP } else { WHEEL_DOWN };
        mouse_report(code, MouseAction::Press, hit, mods, mode)
            .map(|report| Command::Input(report.repeat(count)))
    } else if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
        let prefix = if mode.contains(TermMode::APP_CURSOR) {
            "\x1bO"
        } else {
            "\x1b["
        };
        let arrow = format!("{prefix}{}", if lines > 0 { 'A' } else { 'B' });
        Some(Command::Input(arrow.repeat(count).into_bytes()))
    } else {
        Some(Command::Scroll(Scroll::Delta(lines)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Hit = Hit {
        row: 4,
        col: 9,
        side: Side::Left,
    };

    #[test]
    fn mouse_reports_match_the_desktop_terminal() {
        use MouseAction::*;
        let none = Modifiers::default();
        let ctrl = Modifiers {
            control: true,
            ..Default::default()
        };
        let click = TermMode::MOUSE_REPORT_CLICK;
        let sgr = click | TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report(0, Press, AT, none, sgr),
            Some(b"\x1b[<0;10;5M".to_vec())
        );
        assert_eq!(
            mouse_report(0, Release, AT, none, sgr),
            Some(b"\x1b[<0;10;5m".to_vec())
        );
        assert_eq!(
            mouse_report(WHEEL_UP, Press, AT, ctrl, sgr),
            Some(b"\x1b[<80;10;5M".to_vec())
        );
        assert_eq!(
            mouse_report(2, Press, AT, none, click),
            Some(vec![0x1b, b'[', b'M', 32 + 2, 32 + 10, 32 + 5])
        );
        assert_eq!(
            mouse_report(2, Release, AT, none, click),
            Some(vec![0x1b, b'[', b'M', 32 + 3, 32 + 10, 32 + 5])
        );
        let far = Hit { col: 223, ..AT };
        assert_eq!(mouse_report(0, Press, far, none, click), None);
        assert!(mouse_report(0, Press, far, none, sgr).is_some());
        assert_eq!(mouse_report(0, Move, AT, none, sgr), None);
        assert_eq!(
            mouse_report(0, Move, AT, none, sgr | TermMode::MOUSE_DRAG),
            Some(b"\x1b[<32;10;5M".to_vec())
        );
        assert_eq!(
            mouse_report(NO_BUTTON, Move, AT, none, sgr | TermMode::MOUSE_DRAG),
            None
        );
        assert_eq!(
            mouse_report(
                NO_BUTTON,
                Move,
                AT,
                none,
                TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE
            ),
            Some(b"\x1b[<35;10;5M".to_vec())
        );
        assert_eq!(mouse_report(0, Press, AT, none, TermMode::SGR_MOUSE), None);
    }

    #[test]
    fn wheel_reports_moves_alternate_screens_or_scrolls_history() {
        let none = Modifiers::default();
        let input = |command: Option<Command>| match command {
            Some(Command::Input(bytes)) => bytes,
            _ => panic!("expected input"),
        };
        let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            input(wheel_command(-2, AT, none, sgr, true)),
            b"\x1b[<65;10;5M\x1b[<65;10;5M"
        );
        let alt = TermMode::default() | TermMode::ALT_SCREEN;
        assert_eq!(input(wheel_command(1, AT, none, alt, false)), b"\x1b[A");
        assert_eq!(
            input(wheel_command(
                -2,
                AT,
                none,
                alt | TermMode::APP_CURSOR,
                false
            )),
            b"\x1bOB\x1bOB"
        );
        assert!(matches!(
            wheel_command(3, AT, none, TermMode::default(), false),
            Some(Command::Scroll(Scroll::Delta(3)))
        ));
    }
}
