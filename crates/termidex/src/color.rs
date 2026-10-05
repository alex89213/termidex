//! Terminal color capability detection and the ANSI escapes for it.

use crate::data::Rgb;

/// How much color the terminal can be asked for.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ColorMode {
    /// 24-bit `38;2;r;g;b` escapes.
    Truecolor,
    /// The 256-color palette, `38;5;n`.
    Ansi256,
}

/// The six levels each channel of the xterm 6x6x6 color cube can take.
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// What the terminal supports and whether text should be colored.
#[derive(Copy, Clone, Debug)]
pub struct Caps {
    pub mode: ColorMode,
    /// False when `NO_COLOR` is set. Only affects text, never the sprite.
    pub color_text: bool,
}

impl Caps {
    /// Reads the environment to work out what the terminal can do.
    ///
    /// Truecolor is assumed when `COLORTERM` says so or when running under Windows Terminal.
    /// `NO_COLOR` turns off coloring of text such as types and stat bars, but not the sprite,
    /// because the sprite is the content rather than decoration.
    ///
    /// # Returns
    /// the detected capabilities
    pub fn detect() -> Caps {
        let truecolor = match std::env::var("COLORTERM") {
            Ok(v) => v == "truecolor" || v == "24bit",
            Err(_) => false,
        } || std::env::var_os("WT_SESSION").is_some();

        Caps {
            mode: if truecolor {
                ColorMode::Truecolor
            } else {
                ColorMode::Ansi256
            },
            color_text: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        }
    }
}

/// Appends a decimal number with no formatting machinery.
pub fn push_num(out: &mut Vec<u8>, mut n: u32) {
    if n == 0 {
        out.push(b'0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut i = digits.len();
    while n > 0 {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out.extend_from_slice(&digits[i..]);
}

/// Appends the escape that sets the foreground to a color.
pub fn set_fg(out: &mut Vec<u8>, c: Rgb, mode: ColorMode) {
    match mode {
        ColorMode::Truecolor => {
            out.extend_from_slice(b"\x1b[38;2;");
            push_rgb(out, c);
        }
        ColorMode::Ansi256 => {
            out.extend_from_slice(b"\x1b[38;5;");
            push_num(out, to_ansi256(c) as u32);
            out.push(b'm');
        }
    }
}

/// Appends the escape that sets the background to a color.
pub fn set_bg(out: &mut Vec<u8>, c: Rgb, mode: ColorMode) {
    match mode {
        ColorMode::Truecolor => {
            out.extend_from_slice(b"\x1b[48;2;");
            push_rgb(out, c);
        }
        ColorMode::Ansi256 => {
            out.extend_from_slice(b"\x1b[48;5;");
            push_num(out, to_ansi256(c) as u32);
            out.push(b'm');
        }
    }
}

/// Appends the escape that puts the background back to the terminal's own.
pub fn reset_bg(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[49m");
}

/// Appends the escape that clears every attribute.
pub fn reset(out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[0m");
}

fn push_rgb(out: &mut Vec<u8>, c: Rgb) {
    push_num(out, c.0 as u32);
    out.push(b';');
    push_num(out, c.1 as u32);
    out.push(b';');
    push_num(out, c.2 as u32);
    out.push(b'm');
}

/// Finds the closest xterm-256 palette slot to a true color.
///
/// Both the 6x6x6 cube and the 24-step grayscale ramp are tried, and whichever lands closer wins.
/// The first 16 slots are skipped because terminals remap them freely.
///
/// # Arguments
/// * `c` - the color to approximate
///
/// # Returns
/// a palette index in 16..=255
pub fn to_ansi256(c: Rgb) -> u8 {
    let Rgb(r, g, b) = c;

    let cube_index = |v: u8| -> usize {
        // pick the level with the smallest absolute difference
        let mut best = 0;
        let mut best_d = u16::MAX;
        for (i, level) in CUBE_LEVELS.iter().enumerate() {
            let d = (*level as i16 - v as i16).unsigned_abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    };
    let (ri, gi, bi) = (cube_index(r), cube_index(g), cube_index(b));
    let cube_rgb = Rgb(CUBE_LEVELS[ri], CUBE_LEVELS[gi], CUBE_LEVELS[bi]);
    let cube_slot = 16 + 36 * ri + 6 * gi + bi;

    // the gray ramp runs 8, 18, ... 238
    let luma = (r as u32 + g as u32 + b as u32) / 3;
    let gray_i = ((luma as i32 - 8).clamp(0, 238) as u32 + 5) / 10;
    let gray_i = gray_i.min(23) as usize;
    let gray_v = (8 + 10 * gray_i) as u8;
    let gray_rgb = Rgb(gray_v, gray_v, gray_v);

    if dist(c, gray_rgb) < dist(c, cube_rgb) {
        (232 + gray_i) as u8
    } else {
        cube_slot as u8
    }
}

/// Squared distance between two colors.
fn dist(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| {
        let d = x as i32 - y as i32;
        (d * d) as u32
    };
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pure black, white and mid gray should land on recognizable palette slots
    #[test]
    fn ansi256_anchors() {
        assert_eq!(to_ansi256(Rgb(0, 0, 0)), 16);
        assert_eq!(to_ansi256(Rgb(255, 255, 255)), 231);
        // a pure primary should stay in the cube rather than drifting to gray
        assert_eq!(to_ansi256(Rgb(255, 0, 0)), 196);
        assert_eq!(to_ansi256(Rgb(0, 255, 0)), 46);
        assert_eq!(to_ansi256(Rgb(0, 0, 255)), 21);
    }

    /// a near gray should prefer the gray ramp over the coarse cube
    #[test]
    fn ansi256_prefers_gray_ramp() {
        let slot = to_ansi256(Rgb(118, 118, 118));
        assert!(slot >= 232, "expected a gray ramp slot, got {slot}");
    }

    /// push_num should write plain decimal
    #[test]
    fn push_num_writes_decimal() {
        let mut out = Vec::new();
        push_num(&mut out, 0);
        push_num(&mut out, 4096);
        assert_eq!(out, b"04096");
    }
}
