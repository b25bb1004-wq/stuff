//! Layout unit and property tests for `glint`.
#![allow(clippy::unwrap_used)]

use assay::forall;
use glint::{Align, Dir, Insets, Justify, Kind, Rect, Style, Theme, Ui};

fn box_style(dir: Dir) -> Style {
    Style {
        dir,
        ..Style::default()
    }
}

fn fixed_box(w: u32, h: u32) -> Style {
    Style {
        width: Some(w),
        height: Some(h),
        ..Style::default()
    }
}

#[test]
fn test_01_root_rect() {
    let mut ui = Ui::new(Theme::light());
    assert_eq!(ui.rect(ui.root()), None);
    ui.layout(800, 600);
    assert_eq!(
        ui.rect(ui.root()),
        Some(Rect {
            x: 0,
            y: 0,
            w: 800,
            h: 600
        })
    );
}

#[test]
fn test_02_column_start_stretch() {
    let mut ui = Ui::new(Theme::light());
    let a = ui.add(ui.root(), Kind::Box, fixed_box(40, 20)).unwrap();
    let b = ui.add(ui.root(), Kind::Box, fixed_box(60, 30)).unwrap();
    ui.layout(200, 100);
    assert_eq!(
        ui.rect(a),
        Some(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20
        })
    );
    assert_eq!(
        ui.rect(b),
        Some(Rect {
            x: 0,
            y: 20,
            w: 60,
            h: 30
        })
    );
}

#[test]
fn test_03_row_with_gap_and_padding() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                gap: 10,
                padding: Insets {
                    top: 5,
                    right: 6,
                    bottom: 7,
                    left: 8,
                },
                height: Some(50),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui.add(row, Kind::Box, fixed_box(20, 10)).unwrap();
    let b = ui.add(row, Kind::Box, fixed_box(30, 15)).unwrap();
    ui.layout(200, 100);
    assert_eq!(
        ui.rect(row),
        Some(Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 50
        })
    );
    assert_eq!(
        ui.rect(a),
        Some(Rect {
            x: 8,
            y: 5,
            w: 20,
            h: 10
        })
    );
    assert_eq!(
        ui.rect(b),
        Some(Rect {
            x: 38,
            y: 5,
            w: 30,
            h: 15
        })
    );
}

#[test]
fn test_04_stretch_without_explicit_cross() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                align: Align::Stretch,
                height: Some(60),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui
        .add(
            row,
            Kind::Box,
            Style {
                width: Some(30),
                ..Style::default()
            },
        )
        .unwrap();
    let b = ui.add(row, Kind::Box, fixed_box(25, 12)).unwrap();
    ui.layout(100, 100);
    assert_eq!(
        ui.rect(a),
        Some(Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 60
        })
    );
    assert_eq!(
        ui.rect(b),
        Some(Rect {
            x: 30,
            y: 0,
            w: 25,
            h: 12
        })
    );
}

#[test]
fn test_05_grow_equal_with_remainder() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                height: Some(40),
                ..Style::default()
            },
        )
        .unwrap();
    let g1 = Style {
        grow: 1,
        ..Style::default()
    };
    let a = ui.add(row, Kind::Box, g1).unwrap();
    let b = ui.add(row, Kind::Box, g1).unwrap();
    let c = ui.add(row, Kind::Box, g1).unwrap();
    ui.layout(10, 40);
    // 10 / 3 = 3 rem 1 -> first child gets 4, others 3
    assert_eq!(ui.rect(a).unwrap().w, 4);
    assert_eq!(ui.rect(b).unwrap().w, 3);
    assert_eq!(ui.rect(c).unwrap().w, 3);
    assert_eq!(ui.rect(a).unwrap().x, 0);
    assert_eq!(ui.rect(b).unwrap().x, 4);
    assert_eq!(ui.rect(c).unwrap().x, 7);
}

#[test]
fn test_06_grow_weighted_with_non_growing_child() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                gap: 5,
                height: Some(30),
                ..Style::default()
            },
        )
        .unwrap();
    let fixed = ui
        .add(
            row,
            Kind::Box,
            Style {
                width: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let g1 = ui
        .add(
            row,
            Kind::Box,
            Style {
                grow: 1,
                ..Style::default()
            },
        )
        .unwrap();
    let g2 = ui
        .add(
            row,
            Kind::Box,
            Style {
                grow: 2,
                ..Style::default()
            },
        )
        .unwrap();
    // width=50, fixed=20, gaps=2*5=10 -> free=20; 20*1/3=6, 20*2/3=13, rem=1 -> g1 gets 7, g2 gets 13
    ui.layout(50, 30);
    assert_eq!(
        ui.rect(fixed).unwrap(),
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 30
        }
    );
    assert_eq!(
        ui.rect(g1).unwrap(),
        Rect {
            x: 25,
            y: 0,
            w: 7,
            h: 30
        }
    );
    assert_eq!(
        ui.rect(g2).unwrap(),
        Rect {
            x: 37,
            y: 0,
            w: 13,
            h: 30
        }
    );
}

#[test]
fn test_07_justify_center() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                justify: Justify::Center,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui.add(row, Kind::Box, fixed_box(15, 10)).unwrap();
    // free = 100 - 15 = 85 -> floor(85 / 2) = 42
    ui.layout(100, 20);
    assert_eq!(ui.rect(a).unwrap().x, 42);
}

#[test]
fn test_08_justify_end() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                justify: Justify::End,
                gap: 5,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui.add(row, Kind::Box, fixed_box(10, 10)).unwrap();
    let b = ui.add(row, Kind::Box, fixed_box(20, 10)).unwrap();
    // free = 100 - 30 - 5 = 65
    ui.layout(100, 20);
    assert_eq!(ui.rect(a).unwrap().x, 65);
    assert_eq!(ui.rect(b).unwrap().x, 80);
}

#[test]
fn test_09_justify_space_between_multi() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                justify: Justify::SpaceBetween,
                gap: 2,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui.add(row, Kind::Box, fixed_box(10, 10)).unwrap();
    let b = ui.add(row, Kind::Box, fixed_box(10, 10)).unwrap();
    let c = ui.add(row, Kind::Box, fixed_box(10, 10)).unwrap();
    // free = 100 - 30 - 4 = 66; floor(66 / 2) = 33 added to gap (total step = 35)
    ui.layout(100, 20);
    assert_eq!(ui.rect(a).unwrap().x, 0);
    assert_eq!(ui.rect(b).unwrap().x, 45);
    assert_eq!(ui.rect(c).unwrap().x, 90);
}

#[test]
fn test_10_justify_space_between_single_child() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                justify: Justify::SpaceBetween,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui.add(row, Kind::Box, fixed_box(10, 10)).unwrap();
    ui.layout(100, 20);
    assert_eq!(ui.rect(a).unwrap().x, 0);
}

#[test]
fn test_11_justify_ignored_when_grow_active() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                justify: Justify::End,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui
        .add(
            row,
            Kind::Box,
            Style {
                width: Some(10),
                grow: 1,
                ..Style::default()
            },
        )
        .unwrap();
    ui.layout(100, 20);
    assert_eq!(
        ui.rect(a),
        Some(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 20
        })
    );
}

#[test]
fn test_12_align_start_center_end() {
    let mut ui = Ui::new(Theme::light());
    let c1 = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                align: Align::Start,
                height: Some(21),
                ..Style::default()
            },
        )
        .unwrap();
    let a1 = ui.add(c1, Kind::Box, fixed_box(10, 8)).unwrap();

    let c2 = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                align: Align::Center,
                height: Some(21),
                ..Style::default()
            },
        )
        .unwrap();
    let a2 = ui.add(c2, Kind::Box, fixed_box(10, 8)).unwrap();

    let c3 = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                align: Align::End,
                height: Some(21),
                ..Style::default()
            },
        )
        .unwrap();
    let a3 = ui.add(c3, Kind::Box, fixed_box(10, 8)).unwrap();

    ui.layout(100, 100);
    assert_eq!(ui.rect(a1).unwrap().y, 0);
    // (21 - 8) / 2 = 6
    assert_eq!(ui.rect(a2).unwrap().y, 21 + 6);
    // 21 - 8 = 13
    assert_eq!(ui.rect(a3).unwrap().y, 42 + 13);
}

#[test]
fn test_13_cross_overflow_starts_at_inner_edge() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                align: Align::End,
                padding: Insets {
                    top: 4,
                    right: 0,
                    bottom: 4,
                    left: 5,
                },
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let tall = ui.add(row, Kind::Box, fixed_box(10, 50)).unwrap();
    ui.layout(100, 20);
    assert_eq!(
        ui.rect(tall),
        Some(Rect {
            x: 5,
            y: 4,
            w: 10,
            h: 50
        })
    );
}

#[test]
fn test_14_main_overflow_keeps_base_size() {
    let mut ui = Ui::new(Theme::light());
    let row = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Row,
                justify: Justify::Center,
                gap: 10,
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui
        .add(
            row,
            Kind::Box,
            Style {
                width: Some(40),
                grow: 1,
                ..Style::default()
            },
        )
        .unwrap();
    let b = ui
        .add(
            row,
            Kind::Box,
            Style {
                width: Some(40),
                grow: 1,
                ..Style::default()
            },
        )
        .unwrap();
    // 40 + 10 + 40 = 90 > 50 -> free <= 0 -> keep base size, start at 0
    ui.layout(50, 20);
    assert_eq!(
        ui.rect(a).unwrap(),
        Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20
        }
    );
    assert_eq!(
        ui.rect(b).unwrap(),
        Rect {
            x: 50,
            y: 0,
            w: 40,
            h: 20
        }
    );
}

#[test]
fn test_15_min_sizes_override_content_and_explicit() {
    let mut ui = Ui::new(Theme::light());
    let col = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Column,
                align: Align::Start,
                ..Style::default()
            },
        )
        .unwrap();
    let a = ui
        .add(
            col,
            Kind::Box,
            Style {
                width: Some(10),
                height: Some(10),
                min_w: 35,
                min_h: 25,
                ..Style::default()
            },
        )
        .unwrap();
    let b = ui
        .add(
            col,
            Kind::Label("hi".to_string()),
            Style {
                min_w: 50,
                min_h: 30,
                ..Style::default()
            },
        )
        .unwrap();
    ui.layout(200, 200);
    assert_eq!(
        ui.rect(a),
        Some(Rect {
            x: 0,
            y: 0,
            w: 35,
            h: 25
        })
    );
    assert_eq!(
        ui.rect(b),
        Some(Rect {
            x: 0,
            y: 25,
            w: 50,
            h: 30
        })
    );
}

#[test]
fn test_16_label_button_field_content_sizes() {
    let mut ui = Ui::new(Theme::light());
    ui.remove(0); // no-op, root stays
    let col = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Column,
                align: Align::Start,
                ..Style::default()
            },
        )
        .unwrap();
    let lbl = ui
        .add(
            col,
            Kind::Label("abc".to_string()),
            Style {
                padding: Insets {
                    top: 1,
                    right: 2,
                    bottom: 3,
                    left: 4,
                },
                ..Style::default()
            },
        )
        .unwrap();
    let btn = ui
        .add(col, Kind::Button("OK".to_string()), Style::default())
        .unwrap();
    let fld = ui
        .add(col, Kind::Field("x".to_string()), Style::default())
        .unwrap();

    ui.layout(300, 300);
    let (lw, lh) = pigment::measure("abc", 1);
    assert_eq!(
        ui.rect(lbl),
        Some(Rect {
            x: 0,
            y: 0,
            w: lw + 6,
            h: lh + 4
        })
    );
    let (bw, bh) = pigment::measure("OK", 1);
    assert_eq!(
        ui.rect(btn),
        Some(Rect {
            x: 0,
            y: (lh + 4) as i32,
            w: bw,
            h: bh
        })
    );
    assert_eq!(
        ui.rect(fld),
        Some(Rect {
            x: 0,
            y: (lh + 4 + bh) as i32,
            w: 80,
            h: 16
        })
    );
}

#[test]
fn test_17_field_wide_text_and_scale_2() {
    let mut theme = Theme::dark();
    theme.text_scale = 2;
    let mut ui = Ui::new(theme);
    let col = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Column,
                align: Align::Start,
                ..Style::default()
            },
        )
        .unwrap();
    let short_fld = ui
        .add(col, Kind::Field("hi".to_string()), Style::default())
        .unwrap();
    let long_txt = "0123456789abcdef";
    let long_fld = ui
        .add(col, Kind::Field(long_txt.to_string()), Style::default())
        .unwrap();

    ui.layout(500, 500);
    assert_eq!(ui.rect(short_fld).unwrap().w, 160);
    assert_eq!(ui.rect(short_fld).unwrap().h, 32);
    let (mw, _) = pigment::measure(long_txt, 2);
    assert_eq!(ui.rect(long_fld).unwrap().w, mw.max(160));
    assert_eq!(ui.rect(long_fld).unwrap().h, 32);
}

#[test]
fn test_18_nested_box_content_size_propagates() {
    let mut ui = Ui::new(Theme::light());
    let outer = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Column,
                align: Align::Start,
                ..Style::default()
            },
        )
        .unwrap();
    let inner = ui
        .add(
            outer,
            Kind::Box,
            Style {
                dir: Dir::Row,
                gap: 6,
                padding: Insets {
                    top: 2,
                    right: 3,
                    bottom: 4,
                    left: 5,
                },
                ..Style::default()
            },
        )
        .unwrap();
    let _c1 = ui.add(inner, Kind::Box, fixed_box(10, 12)).unwrap();
    let _c2 = ui.add(inner, Kind::Box, fixed_box(20, 18)).unwrap();
    let after = ui.add(outer, Kind::Box, fixed_box(5, 5)).unwrap();

    ui.layout(200, 200);
    // inner: w = 10 + 20 + 6 + 5 + 3 = 44, h = 18 + 2 + 4 = 24
    assert_eq!(
        ui.rect(inner),
        Some(Rect {
            x: 0,
            y: 0,
            w: 44,
            h: 24
        })
    );
    assert_eq!(
        ui.rect(after),
        Some(Rect {
            x: 0,
            y: 24,
            w: 5,
            h: 5
        })
    );
}

#[test]
fn test_19_padding_larger_than_parent_saturates_to_zero() {
    let mut ui = Ui::new(Theme::light());
    let bx = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                padding: Insets {
                    top: 50,
                    right: 50,
                    bottom: 50,
                    left: 50,
                },
                height: Some(20),
                ..Style::default()
            },
        )
        .unwrap();
    let child = ui
        .add(
            bx,
            Kind::Box,
            Style {
                grow: 1,
                ..Style::default()
            },
        )
        .unwrap();
    ui.layout(30, 20);
    assert_eq!(
        ui.rect(child),
        Some(Rect {
            x: 50,
            y: 50,
            w: 0,
            h: 0
        })
    );
}

#[test]
fn test_20_empty_box_size_with_padding() {
    let mut ui = Ui::new(Theme::light());
    let col = ui
        .add(
            ui.root(),
            Kind::Box,
            Style {
                dir: Dir::Column,
                align: Align::Start,
                ..Style::default()
            },
        )
        .unwrap();
    let empty = ui
        .add(
            col,
            Kind::Box,
            Style {
                padding: Insets {
                    top: 3,
                    right: 4,
                    bottom: 5,
                    left: 6,
                },
                ..Style::default()
            },
        )
        .unwrap();
    ui.layout(100, 100);
    assert_eq!(
        ui.rect(empty),
        Some(Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 8
        })
    );
}

#[test]
fn prop_layout_never_panics_and_non_overflowing_children_inside_parent() {
    forall(200, 0xA11C_E001, |rng| {
        let mut theme = Theme::light();
        theme.text_scale = rng.range(0, 4) as u32;
        let mut ui = Ui::new(theme);
        let mut boxes = vec![ui.root()];
        let count = rng.range(1, 35);
        for i in 0..count {
            let parent = *rng.pick(&boxes);
            let dir = if rng.chance(1, 2) {
                Dir::Row
            } else {
                Dir::Column
            };
            let justify = match rng.range(0, 4) {
                0 => Justify::Start,
                1 => Justify::Center,
                2 => Justify::End,
                _ => Justify::SpaceBetween,
            };
            let align = match rng.range(0, 4) {
                0 => Align::Start,
                1 => Align::Center,
                2 => Align::End,
                _ => Align::Stretch,
            };
            let style = Style {
                dir,
                justify,
                align,
                gap: rng.range(0, 12) as u32,
                padding: Insets {
                    top: rng.range(0, 8) as u32,
                    right: rng.range(0, 8) as u32,
                    bottom: rng.range(0, 8) as u32,
                    left: rng.range(0, 8) as u32,
                },
                grow: rng.range(0, 4) as u32,
                width: if rng.chance(1, 3) {
                    Some(rng.range(0, 60) as u32)
                } else {
                    None
                },
                height: if rng.chance(1, 3) {
                    Some(rng.range(0, 60) as u32)
                } else {
                    None
                },
                min_w: rng.range(0, 20) as u32,
                min_h: rng.range(0, 20) as u32,
            };
            let kind = match rng.range(0, 4) {
                0 => Kind::Box,
                1 => Kind::Label(format!("L{i}")),
                2 => Kind::Button(format!("B{i}")),
                _ => Kind::Field(format!("F{i}")),
            };
            if let Some(id) = ui.add(parent, kind.clone(), style) {
                if matches!(kind, Kind::Box) {
                    boxes.push(id);
                }
            }
        }

        let w = rng.range(0, 800) as u32;
        let h = rng.range(0, 800) as u32;
        ui.layout(w, h);

        // Also check non-overflowing Box invariant on a clean large canvas.
        let big_w = 5000u32;
        let big_h = 5000u32;
        let mut clean_ui = Ui::new(Theme::light());
        let row = clean_ui
            .add(clean_ui.root(), Kind::Box, box_style(Dir::Row))
            .unwrap();
        for _ in 0..5 {
            let cw = rng.range(10, 100) as u32;
            let ch = rng.range(10, 100) as u32;
            clean_ui.add(row, Kind::Box, fixed_box(cw, ch));
        }
        clean_ui.layout(big_w, big_h);
        let pr = clean_ui.rect(row).unwrap();
        for &cid in clean_ui.children(row) {
            let cr = clean_ui.rect(cid).unwrap();
            assert!(cr.x >= pr.x);
            assert!(cr.y >= pr.y);
            assert!(i64::from(cr.x) + i64::from(cr.w) <= i64::from(pr.x) + i64::from(pr.w));
            assert!(i64::from(cr.y) + i64::from(cr.h) <= i64::from(pr.y) + i64::from(pr.h));
        }
    });
}
