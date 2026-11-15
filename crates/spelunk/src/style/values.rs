//! Parsing of single CSS declarations into typed property values.

use crate::css::{Token, Value};

use super::{Display, FlexAlign, FlexDir, FlexJustify, Len};

/// One typed value produced by a declaration; shorthands expand to several.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Prop {
    Display(Display),
    Width(Len),
    Height(Len),
    Margin(usize, Len),
    Padding(usize, i32),
    Border(usize, i32),
    Color([u8; 4]),
    Background([u8; 4]),
    FontPx(i32),
    FlexDir(FlexDir),
    FlexJustify(FlexJustify),
    FlexAlign(FlexAlign),
    FlexGrow(u32),
    Gap(i32),
}

/// Number of cascade slots: the cascade keeps one winner per slot.
pub(crate) const SLOTS: usize = 23;

impl Prop {
    /// The cascade slot this value competes in.
    pub(crate) fn slot(self) -> usize {
        match self {
            Prop::Display(_) => 0,
            Prop::Width(_) => 1,
            Prop::Height(_) => 2,
            Prop::Margin(i, _) => 3 + i,
            Prop::Padding(i, _) => 7 + i,
            Prop::Border(i, _) => 11 + i,
            Prop::Color(_) => 15,
            Prop::Background(_) => 16,
            Prop::FontPx(_) => 17,
            Prop::FlexDir(_) => 18,
            Prop::FlexJustify(_) => 19,
            Prop::FlexAlign(_) => 20,
            Prop::FlexGrow(_) => 21,
            Prop::Gap(_) => 22,
        }
    }
}

/// Parse one declaration. An empty list means the declaration is invalid and is ignored.
pub(crate) fn parse_decl(name: &str, value: &[Value]) -> Vec<Prop> {
    let toks: Vec<&Value> = value.iter().filter(|v| !is_space(v)).collect();
    parse_tokens(&name.to_ascii_lowercase(), &toks).unwrap_or_default()
}

fn is_space(v: &Value) -> bool {
    matches!(v, Value::Token(Token::Whitespace))
}

fn parse_tokens(name: &str, t: &[&Value]) -> Option<Vec<Prop>> {
    let props = match name {
        "display" => vec![Prop::Display(display(one(t)?)?)],
        "width" => vec![Prop::Width(size(one(t)?)?)],
        "height" => vec![Prop::Height(size(one(t)?)?)],
        "margin" => quad(t, len)?
            .iter()
            .enumerate()
            .map(|(i, l)| Prop::Margin(i, *l))
            .collect(),
        "padding" => quad(t, pad)?
            .iter()
            .enumerate()
            .map(|(i, p)| Prop::Padding(i, *p))
            .collect(),
        "border-width" => quad(t, pad)?
            .iter()
            .enumerate()
            .map(|(i, p)| Prop::Border(i, *p))
            .collect(),
        "color" => vec![Prop::Color(color(one(t)?)?)],
        "background-color" => vec![Prop::Background(color(one(t)?)?)],
        "font-size" => vec![Prop::FontPx(font(one(t)?)?)],
        "flex-direction" => vec![Prop::FlexDir(flex_dir(one(t)?)?)],
        "justify-content" => vec![Prop::FlexJustify(justify(one(t)?)?)],
        "align-items" => vec![Prop::FlexAlign(align(one(t)?)?)],
        "flex-grow" => vec![Prop::FlexGrow(grow(one(t)?)?)],
        "gap" => vec![Prop::Gap(pad(one(t)?)?)],
        _ => {
            let margin = name.strip_prefix("margin-").and_then(side_index);
            let padding = name.strip_prefix("padding-").and_then(side_index);
            let border = name
                .strip_prefix("border-")
                .and_then(|r| r.strip_suffix("-width"))
                .and_then(side_index);
            if let Some(side) = margin {
                vec![Prop::Margin(side, len(one(t)?)?)]
            } else if let Some(side) = padding {
                vec![Prop::Padding(side, pad(one(t)?)?)]
            } else {
                let side = border?;
                vec![Prop::Border(side, pad(one(t)?)?)]
            }
        }
    };
    Some(props)
}

fn side_index(s: &str) -> Option<usize> {
    match s {
        "top" => Some(0),
        "right" => Some(1),
        "bottom" => Some(2),
        "left" => Some(3),
        _ => None,
    }
}

/// The only value of a declaration, if it has exactly one.
fn one<'a>(t: &[&'a Value]) -> Option<&'a Value> {
    match t {
        [v] => Some(*v),
        _ => None,
    }
}

/// Expand a 1 to 4 value shorthand into top, right, bottom, left.
fn quad<T: Copy>(t: &[&Value], f: impl Fn(&Value) -> Option<T>) -> Option<[T; 4]> {
    let vals = t.iter().map(|v| f(v)).collect::<Option<Vec<T>>>()?;
    match vals.as_slice() {
        [a] => Some([*a; 4]),
        [a, b] => Some([*a, *b, *a, *b]),
        [a, b, c] => Some([*a, *b, *c, *b]),
        [a, b, c, d] => Some([*a, *b, *c, *d]),
        _ => None,
    }
}

fn ident(v: &Value) -> Option<&str> {
    match v {
        Value::Token(Token::Ident(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn display(v: &Value) -> Option<Display> {
    match ident(v)?.to_ascii_lowercase().as_str() {
        "block" => Some(Display::Block),
        "inline" => Some(Display::Inline),
        "none" => Some(Display::None),
        "flex" => Some(Display::Flex),
        _ => None,
    }
}

fn keyword(v: &Value) -> Option<String> {
    ident(v).map(str::to_ascii_lowercase)
}

fn flex_dir(v: &Value) -> Option<FlexDir> {
    match keyword(v)?.as_str() {
        "row" => Some(FlexDir::Row),
        "column" => Some(FlexDir::Column),
        _ => None,
    }
}

fn justify(v: &Value) -> Option<FlexJustify> {
    match keyword(v)?.as_str() {
        "flex-start" => Some(FlexJustify::Start),
        "center" => Some(FlexJustify::Center),
        "flex-end" => Some(FlexJustify::End),
        "space-between" => Some(FlexJustify::SpaceBetween),
        _ => None,
    }
}

fn align(v: &Value) -> Option<FlexAlign> {
    match keyword(v)?.as_str() {
        "flex-start" => Some(FlexAlign::Start),
        "center" => Some(FlexAlign::Center),
        "flex-end" => Some(FlexAlign::End),
        "stretch" => Some(FlexAlign::Stretch),
        _ => None,
    }
}

/// `flex-grow`: a non-negative whole number.
fn grow(v: &Value) -> Option<u32> {
    match v {
        Value::Token(Token::Number { value, .. })
            if *value >= 0.0 && value.fract() == 0.0 && *value <= f64::from(u32::MAX) =>
        {
            Some(*value as u32)
        }
        _ => None,
    }
}

/// A pixel length; a dimension in `px` or a bare `0`.
fn length(v: &Value) -> Option<i32> {
    match v {
        Value::Token(Token::Dimension { value, unit, .. }) if unit.eq_ignore_ascii_case("px") => {
            Some(px(*value))
        }
        Value::Token(Token::Number { value, .. }) if *value == 0.0 => Some(0),
        _ => None,
    }
}

/// Round a pixel value; float-to-int casts saturate and map NaN to 0.
fn px(v: f64) -> i32 {
    v.round() as i32
}

fn len(v: &Value) -> Option<Len> {
    if ident(v).is_some_and(|s| s.eq_ignore_ascii_case("auto")) {
        Some(Len::Auto)
    } else {
        length(v).map(Len::Px)
    }
}

fn size(v: &Value) -> Option<Len> {
    match len(v)? {
        Len::Px(p) if p < 0 => None,
        l => Some(l),
    }
}

/// Padding and border widths: negative values clamp to 0.
fn pad(v: &Value) -> Option<i32> {
    length(v).map(|p| p.max(0))
}

fn font(v: &Value) -> Option<i32> {
    length(v).filter(|p| *p >= 0)
}

fn color(v: &Value) -> Option<[u8; 4]> {
    match v {
        Value::Token(Token::Hash { value, .. }) => hex(value),
        Value::Token(Token::Ident(s)) => named(&s.to_ascii_lowercase()),
        _ => None,
    }
}

fn named(s: &str) -> Option<[u8; 4]> {
    Some(match s {
        "transparent" => [0, 0, 0, 0],
        "black" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "gray" => [128, 128, 128, 255],
        _ => return None,
    })
}

/// `#rgb` or `#rrggbb` (the hash text without `#`).
fn hex(s: &str) -> Option<[u8; 4]> {
    let d: Vec<u8> = s
        .chars()
        .map(|c| c.to_digit(16).map(|x| x as u8))
        .collect::<Option<_>>()?;
    match d.as_slice() {
        [r, g, b] => Some([r * 17, g * 17, b * 17, 255]),
        [r1, r2, g1, g2, b1, b2] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2, 255]),
        _ => None,
    }
}
