//! Tree, event, focus, and render tests for `glint`.
#![allow(clippy::unwrap_used)]

use glint::{Action, Canvas, Dir, Event, Key, Kind, Style, Theme, Ui};

#[test]
fn tree_operations_and_invalid_ids() {
    let mut ui = Ui::new(Theme::light());
    let root = ui.root();
    assert_eq!(root, 0);
    assert!(!ui.remove(root));
    assert!(!ui.remove(999));
    assert!(ui.children(999).is_empty());
    assert_eq!(ui.text(root), None);
    assert!(!ui.set_text(root, "nope"));
    assert_eq!(ui.rect(999), None);

    let lbl = ui
        .add(root, Kind::Label("hello".to_string()), Style::default())
        .unwrap();
    assert_eq!(ui.text(lbl), Some("hello"));
    assert!(ui.set_text(lbl, "world"));
    assert_eq!(ui.text(lbl), Some("world"));
    // Cannot add under a non-Box
    assert_eq!(ui.add(lbl, Kind::Box, Style::default()), None);

    let bx = ui.add(root, Kind::Box, Style::default()).unwrap();
    let child = ui
        .add(bx, Kind::Button("go".to_string()), Style::default())
        .unwrap();
    assert_eq!(ui.children(root), &[lbl, bx]);
    assert!(ui.remove(bx));
    assert!(!ui.remove(bx));
    assert_eq!(ui.children(root), &[lbl]);
    assert_eq!(ui.text(child), None);
    assert!(!ui.set_text(child, "x"));

    // Removed ids are never reused.
    let next = ui.add(root, Kind::Box, Style::default()).unwrap();
    assert!(next > child);
}

#[test]
fn hit_testing_deepest_and_later_sibling_on_top() {
    let mut ui = Ui::new(Theme::light());
    let b1 = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                width: Some(50),
                height: Some(50),
                ..Style::default()
            },
        )
        .unwrap();
    let b1_child = ui
        .add(
            b1,
            Kind::Button("A".to_string()),
            Style {
                width: Some(40),
                height: Some(40),
                ..Style::default()
            },
        )
        .unwrap();
    ui.layout(100, 100);

    assert_eq!(ui.hit(-1, 0), None);
    assert_eq!(ui.hit(100, 0), None);
    assert_eq!(ui.hit(0, 100), None);
    assert_eq!(ui.hit(10, 10), Some(b1_child));
    assert_eq!(ui.hit(45, 45), Some(b1));
    assert_eq!(ui.hit(80, 80), Some(ui.root()));

    // Overlapping later sibling via overflow on parent:
    let mut ui2 = Ui::new(Theme::light());
    let row = ui2
        .add(
            ui2.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let first_box = ui2
        .add(
            row,
            Kind::Box,
            Style {
                width: Some(10),
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let _overflowing_child = ui2
        .add(
            first_box,
            Kind::Button("wide".to_string()),
            Style {
                width: Some(50),
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let second_box = ui2
        .add(
            row,
            Kind::Button("B".to_string()),
            Style {
                width: Some(20),
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    ui2.layout(100, 100);
    // At x=15, both _overflowing_child (under first_box) and second_box cover (15, 5).
    // Later sibling (second_box) must win.
    assert_eq!(ui2.hit(15, 5), Some(second_box));
}

#[test]
fn tab_ring_and_escape() {
    let mut ui = Ui::new(Theme::light());
    // No focusables: Tab and ShiftTab do nothing.
    assert!(ui.event(Event::Key(Key::Tab)).is_empty());
    assert!(ui.event(Event::Key(Key::ShiftTab)).is_empty());
    assert!(ui.event(Event::Key(Key::Escape)).is_empty());

    let b1 = ui
        .add(ui.root(), Kind::Button("One".to_string()), Style::default())
        .unwrap();
    let _lbl = ui
        .add(ui.root(), Kind::Label("Skip".to_string()), Style::default())
        .unwrap();
    let nested = ui.add(ui.root(), Kind::Box, Style::default()).unwrap();
    let f1 = ui
        .add(nested, Kind::Field(String::new()), Style::default())
        .unwrap();
    let b2 = ui
        .add(ui.root(), Kind::Button("Two".to_string()), Style::default())
        .unwrap();

    // Tab from None -> first (b1) -> f1 -> b2 -> wrap to b1
    assert_eq!(
        ui.event(Event::Key(Key::Tab)),
        vec![Action::Focused(Some(b1))]
    );
    assert_eq!(ui.focus(), Some(b1));
    assert_eq!(
        ui.event(Event::Key(Key::Tab)),
        vec![Action::Focused(Some(f1))]
    );
    assert_eq!(
        ui.event(Event::Key(Key::Tab)),
        vec![Action::Focused(Some(b2))]
    );
    assert_eq!(
        ui.event(Event::Key(Key::Tab)),
        vec![Action::Focused(Some(b1))]
    );

    // Escape clears focus; second Escape does nothing.
    assert_eq!(
        ui.event(Event::Key(Key::Escape)),
        vec![Action::Focused(None)]
    );
    assert_eq!(ui.focus(), None);
    assert!(ui.event(Event::Key(Key::Escape)).is_empty());

    // ShiftTab from None -> last (b2) -> f1 -> b1 -> wrap to b2
    assert_eq!(
        ui.event(Event::Key(Key::ShiftTab)),
        vec![Action::Focused(Some(b2))]
    );
    assert_eq!(
        ui.event(Event::Key(Key::ShiftTab)),
        vec![Action::Focused(Some(f1))]
    );
    assert_eq!(
        ui.event(Event::Key(Key::ShiftTab)),
        vec![Action::Focused(Some(b1))]
    );
    assert_eq!(
        ui.event(Event::Key(Key::ShiftTab)),
        vec![Action::Focused(Some(b2))]
    );
}

#[test]
fn mouse_click_hover_and_focus_rules() {
    let mut ui = Ui::new(Theme::light());
    let btn = ui
        .add(
            ui.root(),
            Kind::Button("Click".to_string()),
            Style {
                width: Some(60),
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let fld = ui
        .add(ui.root(), Kind::Field("abc".to_string()), Style::default())
        .unwrap();
    ui.layout(200, 200);

    // MouseMove over btn sets hover (no actions).
    assert!(ui.event(Event::MouseMove { x: 10, y: 10 }).is_empty());

    // Press and release on btn -> Focused(btn), then Clicked(btn).
    assert_eq!(
        ui.event(Event::MouseDown { x: 10, y: 10 }),
        vec![Action::Focused(Some(btn))]
    );
    assert_eq!(
        ui.event(Event::MouseUp { x: 10, y: 10 }),
        vec![Action::Clicked(btn)]
    );

    // Press on btn, release outside -> no Clicked.
    assert!(ui.event(Event::MouseDown { x: 10, y: 10 }).is_empty());
    assert!(ui.event(Event::MouseUp { x: 150, y: 150 }).is_empty());

    // Click on field focuses it.
    assert_eq!(
        ui.event(Event::MouseDown { x: 10, y: 25 }),
        vec![Action::Focused(Some(fld))]
    );
    assert!(ui.event(Event::MouseUp { x: 10, y: 25 }).is_empty());

    // Click on root background clears focus.
    assert_eq!(
        ui.event(Event::MouseDown { x: 150, y: 150 }),
        vec![Action::Focused(None)]
    );
}

#[test]
fn field_editing_and_button_keyboard_activation() {
    let mut ui = Ui::new(Theme::light());
    let btn = ui
        .add(ui.root(), Kind::Button("Go".to_string()), Style::default())
        .unwrap();
    let fld = ui
        .add(ui.root(), Kind::Field(String::new()), Style::default())
        .unwrap();

    // Focus btn
    ui.event(Event::Key(Key::Tab));
    assert_eq!(ui.event(Event::Key(Key::Enter)), vec![Action::Clicked(btn)]);
    assert_eq!(ui.event(Event::Key(Key::Space)), vec![Action::Clicked(btn)]);

    // Focus fld
    ui.event(Event::Key(Key::Tab));
    assert_eq!(ui.focus(), Some(fld));
    // Backspace on empty field does nothing.
    assert!(ui.event(Event::Key(Key::Backspace)).is_empty());
    assert_eq!(ui.event(Event::Text('a')), vec![Action::Changed(fld)]);
    assert_eq!(ui.event(Event::Text('b')), vec![Action::Changed(fld)]);
    assert_eq!(ui.text(fld), Some("ab"));
    assert_eq!(
        ui.event(Event::Key(Key::Backspace)),
        vec![Action::Changed(fld)]
    );
    assert_eq!(ui.text(fld), Some("a"));
    assert_eq!(
        ui.event(Event::Key(Key::Enter)),
        vec![Action::Submitted(fld)]
    );
}

#[test]
fn removing_focused_pressed_hovered_clears_silently() {
    let mut ui = Ui::new(Theme::light());
    let btn = ui
        .add(
            ui.root(),
            Kind::Button("Del".to_string()),
            Style {
                width: Some(40),
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    ui.layout(100, 100);
    ui.event(Event::MouseMove { x: 5, y: 5 });
    ui.event(Event::MouseDown { x: 5, y: 5 });
    assert_eq!(ui.focus(), Some(btn));

    assert!(ui.remove(btn));
    assert_eq!(ui.focus(), None);
    // MouseUp after removal must not emit Clicked.
    assert!(ui.event(Event::MouseUp { x: 5, y: 5 }).is_empty());
}

#[test]
fn render_pixels_light_and_dark_themes() {
    for theme in [Theme::light(), Theme::dark()] {
        assert_eq!(theme.bg.3, 255);
        assert_eq!(theme.fg.3, 255);
        assert_eq!(theme.button.3, 255);
        assert_eq!(theme.button_hover.3, 255);
        assert_eq!(theme.button_pressed.3, 255);
        assert_eq!(theme.field_bg.3, 255);
        assert_eq!(theme.focus_ring.3, 255);
        assert!(theme.text_scale >= 1);

        let mut ui = Ui::new(theme);
        let btn = ui
            .add(
                ui.root(),
                Kind::Button("A".to_string()),
                Style {
                    width: Some(40),
                    height: Some(20),
                    ..Style::default()
                },
            )
            .unwrap();
        let fld = ui
            .add(ui.root(), Kind::Field("".to_string()), Style::default())
            .unwrap();
        ui.layout(120, 80);

        let mut canvas = Canvas::new(120, 80, theme.bg);
        ui.render(&mut canvas);

        // Background pixel outside widgets
        assert_eq!(canvas.get(110, 70), Some(theme.bg));
        // Normal button interior
        assert_eq!(canvas.get(30, 10), Some(theme.button));
        // Field interior
        assert_eq!(canvas.get(10, 25), Some(theme.field_bg));

        // Hover button
        ui.event(Event::MouseMove { x: 30, y: 10 });
        ui.render(&mut canvas);
        assert_eq!(canvas.get(30, 10), Some(theme.button_hover));

        // Press & focus button
        ui.event(Event::MouseDown { x: 30, y: 10 });
        ui.render(&mut canvas);
        assert_eq!(canvas.get(30, 10), Some(theme.button_pressed));
        // Focus ring on button border
        assert_eq!(canvas.get(0, 0), Some(theme.focus_ring));
        assert_eq!(canvas.get(39, 19), Some(theme.focus_ring));

        // Focus field instead and move mouse off button
        ui.event(Event::MouseUp { x: 110, y: 70 });
        ui.event(Event::MouseMove { x: 10, y: 25 });
        ui.event(Event::MouseDown { x: 10, y: 25 });
        assert_eq!(ui.focus(), Some(fld));
        ui.render(&mut canvas);
        let fr = ui.rect(fld).unwrap();
        assert_eq!(canvas.get(fr.x, fr.y), Some(theme.focus_ring));
        // Button no longer pressed
        assert_eq!(canvas.get(30, 10), Some(theme.button));
        let _ = btn;
    }
}
