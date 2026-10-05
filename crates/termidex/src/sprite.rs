//! Decodes the baked sprite format and draws it with half-block characters.
//!
//! The format is written by `termidex-gen`: `width u8`, `height u8`, `ncolors u16` little endian,
//! `ncolors` RGB triples, then run-length-encoded palette indices where index 0 is transparent.
//! A run is one of three tokens:
//!
//! * `0iiiirrr` - index 0..15, length 1..8
//! * `10iiiiii` then `rrrrrrrr` - index 0..63, length 1..256
//! * `11000000` then `u16` index and `u16` length - anything else

use crate::color::{self, ColorMode};
use crate::data::Rgb;

/// Upper half block, used when the top pixel is the foreground.
const UPPER: &[u8] = "▀".as_bytes();
/// Lower half block, used when only the bottom pixel is opaque.
const LOWER: &[u8] = "▄".as_bytes();

/// A decoded sprite: palette indices plus the palette they point into.
pub struct Sprite<'a> {
    pub width: u32,
    pub height: u32,
    /// `ncolors` RGB triples. Palette index 1 is the first triple.
    palette: &'a [u8],
    /// One index per pixel, row major.
    indices: Vec<u16>,
}

impl<'a> Sprite<'a> {
    /// Decodes one sprite out of the blob.
    ///
    /// # Arguments
    /// * `bytes` - exactly one sprite's slice of `data::SPRITES`
    ///
    /// # Returns
    /// the decoded sprite
    ///
    /// # Panics
    /// If the blob is truncated or malformed, which can only happen if `data.rs` and
    /// `sprites.bin` came from different generator runs.
    pub fn decode(bytes: &'a [u8]) -> Sprite<'a> {
        if bytes.len() < 4 {
            return Sprite {
                width: 0,
                height: 0,
                palette: &[],
                indices: Vec::new(),
            };
        }
        let width = bytes[0] as u32;
        let height = bytes[1] as u32;
        let ncolors = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        let runs_at = 4 + ncolors * 3;
        let palette = &bytes[4..runs_at];

        let pixels = (width * height) as usize;
        let mut indices = Vec::with_capacity(pixels);
        let runs = &bytes[runs_at..];
        let mut p = 0;
        while p < runs.len() && indices.len() < pixels {
            let c = runs[p];
            let (idx, len) = if c & 0x80 == 0 {
                p += 1;
                (((c >> 3) & 0x0F) as u16, (c & 0x07) as usize + 1)
            } else if c & 0x40 == 0 {
                let len = runs[p + 1] as usize + 1;
                p += 2;
                ((c & 0x3F) as u16, len)
            } else {
                let idx = u16::from_le_bytes([runs[p + 1], runs[p + 2]]);
                let len = u16::from_le_bytes([runs[p + 3], runs[p + 4]]) as usize;
                p += 5;
                (idx, len)
            };
            let len = len.min(pixels - indices.len());
            indices.resize(indices.len() + len, idx);
        }
        // a short stream can only mean a mismatched blob, filling with transparent keeps the
        // renderer in bounds instead of panicking
        indices.resize(pixels, 0);

        Sprite {
            width,
            height,
            palette,
            indices,
        }
    }

    /// Returns the color at a pixel, or `None` where the sprite is transparent.
    fn pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        let idx = self.indices[(y * self.width + x) as usize] as usize;
        if idx == 0 {
            return None;
        }
        let o = (idx - 1) * 3;
        Some(Rgb(
            self.palette[o],
            self.palette[o + 1],
            self.palette[o + 2],
        ))
    }

    /// Draws the sprite into `out` at its native size.
    ///
    /// Each terminal cell carries two vertical pixels. When only one half is opaque the cell uses
    /// a foreground-only half block so the terminal's own background shows through, which keeps
    /// the sprite looking right on light and dark themes alike. Trailing transparent cells are
    /// never written.
    ///
    /// # Arguments
    /// * `out` - the output buffer to append to
    /// * `mode` - the terminal's color depth
    pub fn render(&self, out: &mut Vec<u8>, mode: ColorMode) {
        if self.width == 0 || self.height == 0 {
            return;
        }

        for y in (0..self.height).step_by(2) {
            let mut fg: Option<Rgb> = None;
            let mut bg: Option<Rgb> = None;
            let mut pending_spaces = 0u32;

            for x in 0..self.width {
                let top = self.pixel(x, y);
                let bottom = if y + 1 < self.height {
                    self.pixel(x, y + 1)
                } else {
                    None
                };

                let (cell_fg, cell_bg, glyph) = match (top, bottom) {
                    (None, None) => {
                        pending_spaces += 1;
                        continue;
                    }
                    (Some(t), Some(b)) => (t, Some(b), UPPER),
                    (Some(t), None) => (t, None, UPPER),
                    (None, Some(b)) => (b, None, LOWER),
                };

                if pending_spaces > 0 {
                    // a space would be painted in whatever background is current, so clear it
                    if bg.is_some() {
                        color::reset_bg(out);
                        bg = None;
                    }
                    for _ in 0..pending_spaces {
                        out.push(b' ');
                    }
                    pending_spaces = 0;
                }

                if fg != Some(cell_fg) {
                    color::set_fg(out, cell_fg, mode);
                    fg = Some(cell_fg);
                }
                if bg != cell_bg {
                    match cell_bg {
                        Some(c) => color::set_bg(out, c, mode),
                        None => color::reset_bg(out),
                    }
                    bg = cell_bg;
                }
                out.extend_from_slice(glyph);
            }

            // pending_spaces holds only trailing transparent cells at this point, so drop them
            color::reset(out);
            out.push(b'\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// build the encoded form of a tiny sprite by hand so the decoder is tested on its own
    fn encoded(width: u8, height: u8, palette: &[[u8; 3]], runs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = vec![width, height];
        out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
        for rgb in palette {
            out.extend_from_slice(rgb);
        }
        for (idx, len) in runs {
            if *idx < 16 && *len <= 8 {
                out.push(((*idx as u8) << 3) | (*len as u8 - 1));
            } else if *idx < 64 && *len <= 256 {
                out.push(0x80 | *idx as u8);
                out.push((*len - 1) as u8);
            } else {
                out.push(0xC0);
                out.extend_from_slice(&idx.to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
            }
        }
        out
    }

    /// decode should give back the exact indices that were encoded
    #[test]
    fn decode_expands_runs() {
        let bytes = encoded(
            4,
            2,
            &[[10, 20, 30], [40, 50, 60]],
            &[(0, 3), (1, 2), (2, 3)],
        );
        let sprite = Sprite::decode(&bytes);
        assert_eq!((sprite.width, sprite.height), (4, 2));
        assert_eq!(sprite.pixel(0, 0), None);
        assert_eq!(sprite.pixel(3, 0), Some(Rgb(10, 20, 30)));
        assert_eq!(sprite.pixel(1, 1), Some(Rgb(40, 50, 60)));
    }

    /// a fully transparent row should render as an empty line, not a run of spaces
    #[test]
    fn render_trims_trailing_transparency() {
        let bytes = encoded(4, 2, &[[1, 2, 3]], &[(1, 1), (0, 7)]);
        let sprite = Sprite::decode(&bytes);
        let mut out = Vec::new();
        sprite.render(&mut out, ColorMode::Truecolor);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "\x1b[38;2;1;2;3m▀\x1b[0m\n");
    }

    /// a cell with both halves opaque should set a background as well as a foreground
    #[test]
    fn render_uses_background_for_full_cells() {
        let bytes = encoded(1, 2, &[[1, 1, 1], [9, 9, 9]], &[(1, 1), (2, 1)]);
        let sprite = Sprite::decode(&bytes);
        let mut out = Vec::new();
        sprite.render(&mut out, ColorMode::Truecolor);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, "\x1b[38;2;1;1;1m\x1b[48;2;9;9;9m▀\x1b[0m\n");
    }

    /// an odd number of rows should still emit the last half row
    #[test]
    fn render_handles_odd_height() {
        let bytes = encoded(1, 3, &[[1, 1, 1]], &[(1, 3)]);
        let sprite = Sprite::decode(&bytes);
        let mut out = Vec::new();
        sprite.render(&mut out, ColorMode::Truecolor);
        assert_eq!(String::from_utf8(out).unwrap().lines().count(), 2);
    }
}
