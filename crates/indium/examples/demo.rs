//! Build a small window: a label, a text field and a button, then drive it with input.
use indium::anim::{ease, tween, Ease, Spring};
use indium::widgets::{ListState, Nav};
use indium::{Action, Canvas, Dir, Event, Key, Kind, Style, Theme, Ui};

fn main() {
    // A column with a label, an editable field and a button.
    let mut ui = Ui::new(Theme::light());
    let root = ui.root();
    let column = ui
        .add(
            root,
            Kind::Box,
            Style {
                dir: Dir::Column,
                gap: 4,
                ..Style::default()
            },
        )
        .unwrap_or(root);
    let label = ui.add(column, Kind::Label("Name".to_string()), Style::default());
    let field = ui.add(
        column,
        Kind::Field("Ada".to_string()),
        Style {
            height: Some(24),
            ..Style::default()
        },
    );
    let button = ui.add(
        column,
        Kind::Button("Save".to_string()),
        Style {
            height: Some(24),
            ..Style::default()
        },
    );
    println!("nodes: root={root} label={label:?} field={field:?} button={button:?}");

    ui.layout(200, 120);
    if let Some(rect) = ui.rect(column) {
        println!(
            "column rect: x={} y={} w={} h={}",
            rect.x, rect.y, rect.w, rect.h
        );
    }
    if let Some(rect) = button.and_then(|i| ui.rect(i)) {
        println!(
            "button rect: x={} y={} w={} h={}",
            rect.x, rect.y, rect.w, rect.h
        );
        // Click the button: mouse down then up on its centre.
        let (x, y) = (rect.x + rect.w as i32 / 2, rect.y + rect.h as i32 / 2);
        let mut actions: Vec<Action> = ui.event(Event::MouseDown { x, y });
        actions.extend(ui.event(Event::MouseUp { x, y }));
        println!("click actions: {actions:?}");
    }

    // Keyboard: Tab to the field, type a character.
    ui.event(Event::Key(Key::Tab));
    println!("focus after Tab: {:?}", ui.focus());
    println!("typed actions: {:?}", ui.event(Event::Text('!')));
    println!("field text: {:?}", field.and_then(|i| ui.text(i)));

    // Software render to a 200x120 canvas and read one pixel back.
    let mut canvas = Canvas::new(200, 120, Theme::light().bg);
    ui.render(&mut canvas);
    println!(
        "canvas {}x{}, corner pixel: {:?}",
        canvas.width,
        canvas.height,
        canvas.get(0, 0)
    );

    // Pure widget state: a list of 10 rows showing 4 at a time.
    let mut list = ListState::new(10, 4);
    for _ in 0..3 {
        list.nav(Nav::Down);
    }
    println!("list selected={:?} offset={}", list.selected, list.offset);

    // Easing and tweening on integer time (ms).
    println!(
        "tween 0->100 at 50 of 100 ms (OutQuad): {}",
        tween(0, 100, 0, 100, 50, Ease::OutQuad)
    );
    println!(
        "ease OutCubic at t=500 of 1000: {}",
        ease(Ease::OutCubic, 500)
    );

    // Spring stepped at fixed 10 ms ticks for 200 steps.
    let mut spring = Spring {
        pos: 0,
        vel: 0,
        target: 1000,
        stiffness: 200,
        damping: 20,
    };
    for _ in 0..200 {
        spring.step(10);
    }
    println!(
        "spring after 200 steps: pos={} vel={} settled={}",
        spring.pos,
        spring.vel,
        spring.settled()
    );
}
