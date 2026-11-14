//! Straight-alpha colour and the integer source-over operator.

#[cfg(test)]
mod tests;

/// Straight (not premultiplied) RGBA colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

/// Source-over of `src` onto `dst` with integer maths and straight alpha.
///
/// `oa = sa + (da*(255-sa) + 127) / 255`, and each channel is
/// `(s*sa*255 + d*da*(255-sa) + oa*255/2) / (oa*255)`. A fully transparent source returns `dst`
/// unchanged, which also keeps the colour of a transparent `dst`.
pub fn over(dst: Rgba, src: Rgba) -> Rgba {
    let sa = u32::from(src.3);
    if sa == 0 {
        return dst;
    }
    let da = u32::from(dst.3);
    let oa = sa + (da * (255 - sa) + 127) / 255;
    let mix = |s: u8, d: u8| -> u8 {
        let num = u32::from(s) * sa * 255 + u32::from(d) * da * (255 - sa) + oa * 255 / 2;
        (num / (oa * 255)).min(255) as u8
    };
    Rgba(
        mix(src.0, dst.0),
        mix(src.1, dst.1),
        mix(src.2, dst.2),
        oa as u8,
    )
}
