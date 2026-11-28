use gpui::{Keystroke, TestApp, TestAppWindow};

use super::*;

/// A window with a field at its top left corner, whose text is drawn from x 8.
struct Host(Entity<TextField>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn open(text: &str) -> (TestApp, TestAppWindow<Host>, Entity<TextField>) {
    let mut app = TestApp::new();
    let mut field = None;
    let mut window = app.open_window(|_, cx| {
        let entity = cx.new(|cx| {
            let mut field = TextField::new(Role::TextInput, "Name", "Name", px(240.0), cx);
            field.text = text.to_string();
            field
        });
        field = Some(entity.clone());
        Host(entity)
    });
    let field = field.unwrap();
    window.update(|_, window, cx| {
        let focus = field.read(cx).focus.clone();
        focus.focus(window, cx)
    });
    window.draw();
    (app, window, field)
}

/// The window's place of the gap before the character at `index`, on the field's line.
fn gap(window: &TestAppWindow<Host>, field: &Entity<TextField>, index: usize) -> Point<Pixels> {
    window.read(|_, cx| {
        let field = field.read(cx);
        let layout = field.layout.as_ref().expect("the field was drawn");
        let at = layout.origin + layout.point(index);
        point(at.x, field.bounds.center().y)
    })
}

fn press<V: Render>(window: &mut TestAppWindow<V>, keys: &'static str) {
    for key in keys.split(' ') {
        let keystroke = Keystroke::parse(key).unwrap();
        window.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        window.simulate_event(gpui::KeyUpEvent { keystroke });
    }
}

fn text(window: &TestAppWindow<Host>, field: &Entity<TextField>) -> String {
    window.read(|_, cx| field.read(cx).text.clone())
}

fn selected(window: &TestAppWindow<Host>, field: &Entity<TextField>) -> Range<usize> {
    window.read(|_, cx| field.read(cx).selection())
}

/// What a field with `pastes_images` told its owner of image pastes, as (media type, base64 length).
fn watch_image_pastes(
    window: &mut TestAppWindow<Host>,
    field: &Entity<TextField>,
) -> std::rc::Rc<std::cell::RefCell<Vec<(&'static str, usize)>>> {
    let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(&'static str, usize)>::new()));
    let log = heard.clone();
    window.update(|_, _, cx| {
        cx.subscribe(field, move |_, _, pasted: &PastedImage, _| {
            log.borrow_mut().push((pasted.media_type, pasted.data.len()));
        })
        .detach();
        field.update(cx, |field, _| field.pastes_images = true);
    });
    heard
}

#[test]
fn an_image_paste_reaches_the_page_and_a_text_paste_stays_text() {
    let (_app, mut window, field) = open("");
    let heard = watch_image_pastes(&mut window, &field);
    let image = |format, len| {
        ClipboardItem::new_image(&gpui::Image::from_bytes(format, vec![7; len]))
    };
    window.update(|_, _, cx| cx.write_to_clipboard(image(ImageFormat::Png, 3)));
    press(&mut window, "cmd-v");
    assert_eq!(*heard.borrow(), vec![("image/png", 4)], "3 bytes are 4 base64 characters");
    assert_eq!(text(&window, &field), "");
    // Not an image Claude takes, and one past the cap, reach nobody.
    window.update(|_, _, cx| cx.write_to_clipboard(image(ImageFormat::Tiff, 3)));
    press(&mut window, "cmd-v");
    window.update(|_, _, cx| {
        cx.write_to_clipboard(image(ImageFormat::Png, MAX_PASTED_IMAGE_BYTES + 1))
    });
    press(&mut window, "cmd-v");
    assert_eq!(heard.borrow().len(), 1);
    window.update(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("hello".into())));
    press(&mut window, "cmd-v");
    assert_eq!(heard.borrow().len(), 1);
    assert_eq!(text(&window, &field), "hello");
}

#[test]
fn a_click_between_letters_puts_the_caret_there_and_typing_inserts_at_it() {
    let (_app, mut window, field) = open("flaoting");
    window.simulate_click(gap(&window, &field, 3), MouseButton::Left);
    assert_eq!(selected(&window, &field), 3..3);
    window.simulate_input("t");
    window.draw();
    assert_eq!(text(&window, &field), "flatoting");
    assert_eq!(selected(&window, &field), 4..4);
}

#[test]
fn tab_reaches_the_owner_only_in_a_field_that_takes_it() {
    let (_app, mut window, field) = open("hi");
    let heard = std::rc::Rc::new(std::cell::Cell::new(0));
    let log = heard.clone();
    window.update(|_, _, cx| {
        cx.subscribe(&field, move |_, _, _: &TabPressed, _| log.set(log.get() + 1)).detach();
    });
    press(&mut window, "tab");
    assert_eq!(heard.get(), 0);
    window.update(|_, _, cx| field.update(cx, |field, _| field.takes_tab = true));
    press(&mut window, "tab");
    assert_eq!(heard.get(), 1);
    press(&mut window, "shift-tab");
    assert_eq!(heard.get(), 1);
    assert_eq!(text(&window, &field), "hi");
}

#[test]
fn shift_tab_reaches_the_owner_only_in_a_field_that_takes_it_and_tab_stays_with_focus() {
    let (_app, mut window, field) = open("hi");
    let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::<bool>::new()));
    let log = heard.clone();
    window.update(|_, _, cx| {
        cx.subscribe(&field, move |_, _, tab: &TabPressed, _| log.borrow_mut().push(tab.back)).detach();
    });
    press(&mut window, "shift-tab");
    assert!(heard.borrow().is_empty());
    window.update(|_, _, cx| field.update(cx, |field, _| field.takes_shift_tab = true));
    press(&mut window, "shift-tab");
    assert_eq!(*heard.borrow(), [true]);
    // A field that takes only Shift-Tab leaves plain Tab to move focus.
    press(&mut window, "tab");
    assert_eq!(*heard.borrow(), [true]);
    assert_eq!(text(&window, &field), "hi");
}

#[test]
fn up_and_down_reach_the_owner_only_with_history_keys_and_only_past_the_first_and_last_line() {
    let (_app, mut window, field) = open("one\ntwo\nthree");
    let heard = std::rc::Rc::new(std::cell::RefCell::new(Vec::<bool>::new()));
    let log = heard.clone();
    window.update(|_, _, cx| {
        cx.subscribe(&field, move |_, _, key: &HistoryKey, _| log.borrow_mut().push(key.up)).detach();
        field.update(cx, |field, _| field.rows = Some(3));
    });
    window.draw();
    // Without the prop the keys move the caret as before.
    press(&mut window, "up");
    assert!(heard.borrow().is_empty());
    window.update(|_, _, cx| field.update(cx, |field, _| field.history_keys = true));
    window.draw();
    // The caret is on the middle line now: Up and Down move it, Down to the last line and then past it recalls.
    press(&mut window, "down");
    window.draw();
    assert!(heard.borrow().is_empty());
    press(&mut window, "down");
    assert_eq!(*heard.borrow(), [false]);
    press(&mut window, "up up");
    window.draw();
    assert_eq!(*heard.borrow(), [false]);
    press(&mut window, "up");
    assert_eq!(*heard.borrow(), [false, true]);
    // A modifier keeps the key for selecting.
    press(&mut window, "shift-up");
    assert_eq!(heard.borrow().len(), 2);
}

#[test]
fn a_message_box_sends_on_enter_and_starts_a_line_on_shift_enter() {
    let (_app, mut window, field) = open("hi");
    let lines = |window: &mut TestAppWindow<Host>, submits: bool| {
        window.update(|_, _, cx| {
            field.update(cx, |field, _| {
                field.rows = Some(3);
                field.enter_submits = submits;
            })
        });
    };
    lines(&mut window, true);
    press(&mut window, "enter");
    assert_eq!(text(&window, &field), "hi");
    press(&mut window, "shift-enter");
    assert_eq!(text(&window, &field), "hi\n");
    lines(&mut window, false);
    press(&mut window, "enter");
    assert_eq!(text(&window, &field), "hi\n\n");
}

#[test]
fn shift_arrows_select_and_typing_replaces_the_selection() {
    let (_app, mut window, field) = open("hello world");
    window.simulate_click(gap(&window, &field, 5), MouseButton::Left);
    press(
        &mut window,
        "shift-right shift-right shift-right shift-right shift-right shift-right",
    );
    assert_eq!(selected(&window, &field), 5..11);
    window.simulate_input("!");
    window.draw();
    assert_eq!(text(&window, &field), "hello!");
    assert_eq!(selected(&window, &field), 6..6);
}

#[test]
fn a_double_click_selects_the_word_and_backspace_deletes_the_selection() {
    let (_app, mut window, field) = open("one two three");
    let inside = gap(&window, &field, 5);
    for click_count in [1, 2] {
        window.simulate_event(MouseDownEvent {
            position: inside,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count,
            first_mouse: false,
        });
    }
    assert_eq!(selected(&window, &field), 4..7);
    press(&mut window, "backspace");
    assert_eq!(text(&window, &field), "one  three");
    assert_eq!(selected(&window, &field), 4..4);
    // With nothing selected, Backspace takes one character.
    press(&mut window, "backspace");
    assert_eq!(text(&window, &field), "one three");
}

/// Cyclops' start and end, as a browser's `<input type="time">`: the hours, then the minutes, each a part that
/// digits fill and the arrows step, typed over what is there.
#[test]
fn a_time_fills_its_hours_then_its_minutes_from_digits_typed_over_it() {
    let (_app, mut window, field) = open("");
    window.update(|_, _, cx| field.update(cx, |field, _| field.time = true));
    window.draw();
    let value =
        |window: &TestAppWindow<Host>| window.read(|_, cx| field.read(cx).value().to_string());
    // Empty, it shows --:-- with the hours selected. A 0 waits for the hour's second digit; 9 makes 09 and moves on.
    assert_eq!(text(&window, &field), "--:--");
    assert_eq!(selected(&window, &field), 0..2);
    for (typed, shown) in [
        ("0", "00:--"),
        ("9", "09:--"),
        ("3", "09:03"),
        ("0", "09:30"),
        ("5", "09:05"),
    ] {
        window.simulate_input(typed);
        assert_eq!(text(&window, &field), shown);
    }
    assert_eq!(
        selected(&window, &field),
        3..5,
        "the minutes keep the keyboard"
    );
    assert_eq!(value(&window), "09:05");
    // Emptied, the minutes are no time.
    press(&mut window, "backspace");
    assert_eq!(text(&window, &field), "09:--");
    assert_eq!(value(&window), "", "an unfinished time is no time");
    // A digit no second can follow is the whole part: 7 is 07 and goes on to the minutes, 2 then 4 isn't 24 but 04.
    press(&mut window, "left");
    for typed in ["7", ":", "a", "6"] {
        window.simulate_input(typed);
    }
    assert_eq!(text(&window, &field), "07:06");
    press(&mut window, "left");
    for typed in ["2", "4"] {
        window.simulate_input(typed);
    }
    assert_eq!(text(&window, &field), "04:06");
    // Up and Down step the part, round the clock.
    press(&mut window, "left down down up");
    assert_eq!(text(&window, &field), "03:06");
    press(&mut window, "right down down down down down down down");
    assert_eq!(text(&window, &field), "03:59");
    // A pasted 9:45 is 09:45.
    window.update(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("9:45".into())));
    press(&mut window, "cmd-v");
    assert_eq!(text(&window, &field), "09:45");
}

/// A time the page set, like an entry's 08:30: typing over it changes it, where a fifth digit was dropped.
#[test]
fn a_time_set_from_outside_takes_digits_typed_over_its_hours() {
    let (_app, mut window, field) = open("08:30");
    window.update(|_, _, cx| field.update(cx, |field, _| field.time = true));
    window.draw();
    window.simulate_input("1");
    window.simulate_input("0");
    assert_eq!(text(&window, &field), "10:30");
    // A press on the minutes picks them.
    window.simulate_click(gap(&window, &field, 4), MouseButton::Left);
    window.draw();
    assert_eq!(selected(&window, &field), 3..5);
    window.simulate_input("4");
    window.simulate_input("5");
    assert_eq!(text(&window, &field), "10:45");
}

/// A long text set from outside puts the caret at its end: the field with the keyboard scrolls to show it, and one
/// without shows the text's start.
#[test]
fn a_long_text_shows_its_start_without_the_keyboard_and_its_caret_with_it() {
    let (_app, mut window, field) = open("");
    let long = "https://github.com/acme/web/pull/482/files?diff=split&w=1#diff-0123456789abcdef";
    let scroll = |window: &mut TestAppWindow<Host>| {
        window.draw();
        window.read(|_, cx| field.read(cx).scroll)
    };
    window.update(|_, _, cx| field.update(cx, |field, _| field.set_text(long.into())));
    assert!(scroll(&mut window) > px(0.0), "with the keyboard the caret at the end is in view");
    window.update(|_, window, cx| window.blur(cx));
    assert_eq!(scroll(&mut window), px(0.0));
}

/// A piece of the window that keeps its last drawing, as the workspace's are (`workspace::parts`), holding a field,
/// and counting its renders. `covered`: it draws something of its own over the field, as a page's dialog. `fading`:
/// the field is in a box fading in that never ends.
struct Piece {
    field: Entity<TextField>,
    covered: bool,
    fading: bool,
    drawn: usize,
}

impl Render for Piece {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.drawn += 1;
        let id = window.window_handle().window_id();
        let fading = self.fading;
        let body = slots::drawing_piece("piece", id, || {
            let field = div().size_full().child(self.field.clone());
            match fading {
                true => motion::tween(field, "fading", std::time::Duration::from_secs(60), |el, t, _| el.opacity(t))
                    .into_any_element(),
                false => field.into_any_element(),
            }
        });
        slots::in_piece(Some("piece".into()), self.covered, body)
    }
}

/// The window as the workspace draws it: the piece, kept as last drawn, then the field layer over it.
struct Workspace {
    piece: Entity<Piece>,
    layer: Entity<FieldLayer>,
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        slots::start_draw(window.window_handle().window_id());
        div()
            .size_full()
            .child(self.piece.clone().cached(gpui::StyleRefinement::default().size_full()))
            .child(self.layer.clone())
    }
}

/// The field, `rows` lines tall at least, with the keyboard, in a piece of a window with a field layer.
fn open_in_piece(
    rows: Option<usize>,
    covered: bool,
    fading: bool,
) -> (TestApp, TestAppWindow<Workspace>, Entity<TextField>, Entity<Piece>) {
    let mut app = TestApp::new();
    let mut window = app.open_window(|_, cx| {
        let field = cx.new(|cx| {
            let mut field = TextField::new(Role::TextInput, "Message", "Ask", px(200.0), cx);
            field.rows = rows;
            field
        });
        Workspace {
            piece: cx.new(|_| Piece { field, covered, fading, drawn: 0 }),
            layer: cx.new(|_| FieldLayer),
        }
    });
    let piece = window.read(|workspace, _| workspace.piece.clone());
    let field = window.read(|_, cx| piece.read(cx).field.clone());
    window.update(|_, window, cx| field.read(cx).focus.clone().focus(window, cx));
    window.draw();
    // The first key after the pointer turns GPUI to keyboard input, which draws the whole window once.
    press(&mut window, "right");
    window.draw();
    (app, window, field, piece)
}

/// What the field's text was last drawn as, and the piece's renders.
fn drawn(window: &TestAppWindow<Workspace>, field: &Entity<TextField>, piece: &Entity<Piece>) -> (String, usize) {
    window.read(|_, cx| {
        let shown = field.read(cx).layout.as_ref().map(|layout| layout.doc.to_string()).unwrap_or_default();
        (shown, piece.read(cx).drawn)
    })
}

/// A key typed in a long chat's message box drew the whole chat, and the main column around it, again: GPUI draws a
/// view's ancestors with it, and draws a piece kept as last drawn again whole. The field's text is drawn in the
/// window's layer instead, and a key draws that alone.
#[test]
fn a_key_in_a_field_in_a_piece_draws_the_layer_and_not_the_piece() {
    let (_app, mut window, field, piece) = open_in_piece(Some(2), false, false);
    let before = drawn(&window, &field, &piece).1;
    for key in ["a", "b", "c"] {
        window.simulate_input(key);
        window.draw();
    }
    press(&mut window, "left shift-left");
    window.draw();
    let (shown, drawn) = drawn(&window, &field, &piece);
    assert_eq!(drawn - before, 0, "the piece kept its drawing");
    assert_eq!(shown, "abc", "the layer drew what was typed");
    assert_eq!(window.read(|_, cx| field.read(cx).selection()), 1..2);
}

/// Under something its piece draws over it, as a page's own dialog, the field draws its text in place, under that.
/// So does one fading in, whose text the layer would draw at full strength.
#[test]
fn a_field_its_piece_draws_over_or_fading_in_draws_in_place() {
    for (covered, fading) in [(true, false), (false, true)] {
        let (_app, mut window, field, piece) = open_in_piece(None, covered, fading);
        let before = drawn(&window, &field, &piece).1;
        for key in ["a", "b", "c"] {
            window.simulate_input(key);
            window.draw();
        }
        let (shown, drawn) = drawn(&window, &field, &piece);
        assert_eq!(drawn - before, 3, "covered {covered}, fading {fading}");
        assert_eq!(shown, "abc");
    }
}

/// A line more makes the field taller, which the piece lays out again; typing on that line doesn't.
#[test]
fn a_field_that_grows_a_line_draws_its_piece_again() {
    let (_app, mut window, field, piece) = open_in_piece(Some(1), false, false);
    window.simulate_input("a");
    window.draw();
    let (before, height) = (drawn(&window, &field, &piece).1, window.read(|_, cx| field.read(cx).bounds.size.height));
    press(&mut window, "enter");
    window.draw();
    let grown = window.read(|_, cx| field.read(cx).bounds.size.height);
    assert_eq!(drawn(&window, &field, &piece).1 - before, 1);
    assert!(grown > height, "{grown:?} after {height:?}");
    window.simulate_input("b");
    window.draw();
    assert_eq!(drawn(&window, &field, &piece), ("a\nb".to_owned(), before + 1));
}

/// Cyclops' API token: a browser's `<input type="password">` shows a dot a character, and copy and cut take nothing.
#[test]
fn a_password_shows_dots_edits_by_character_and_is_never_copied() {
    let (_app, mut window, field) = open("cyc_é1");
    window.update(|_, _, cx| field.update(cx, |field, _| field.secret = true));
    window.draw();
    window.read(|_, cx| {
        let layout = field.read(cx).layout.as_ref().unwrap();
        let shown: String = layout
            .lines
            .iter()
            .map(|line| line.text.to_string())
            .collect();
        assert_eq!(shown, "••••••");
    });
    // A click between the third and fourth dots is between the third and fourth characters.
    window.simulate_click(gap(&window, &field, 3), MouseButton::Left);
    assert_eq!(selected(&window, &field), 3..3);
    window.simulate_input("x");
    window.draw();
    assert_eq!(text(&window, &field), "cycx_é1");
    // Past the two-byte é, the caret moves a character at a time.
    press(&mut window, "end left");
    assert_eq!(selected(&window, &field), 7..7);
    window.update(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string("before".into())));
    press(&mut window, "cmd-a cmd-c cmd-x");
    assert_eq!(text(&window, &field), "cycx_é1", "cut leaves the text");
    let clipboard =
        window.update(|_, _, cx| cx.read_from_clipboard().and_then(|item| item.text()));
    assert_eq!(clipboard.as_deref(), Some("before"));
}
