//! Decodes the vendored PNGs and re-encodes them into the lossless blob the runtime reads.
//!
//! The runtime must not carry an image codec, so each sprite becomes: a tiny header, a palette,
//! and run-length-encoded palette indices. Palette index 0 always means transparent. Identical
//! images are stored once and shared by every slot that points at them.
//!
//! # Layout
//! `width u8`, `height u8`, `ncolors u16` little endian, then `ncolors` RGB triples, then the runs.
//!
//! # Run encoding
//! Sprites almost always have fewer than 16 colors and lots of short runs, so a run is written
//! with a variable-length token picked from three shapes:
//!
//! * `0iiiirrr` - index 0..15, length 1..8, one byte
//! * `10iiiiii` then `rrrrrrrr` - index 0..63, length 1..256, two bytes
//! * `11000000` then `u16` index and `u16` length - anything else, five bytes

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Alpha below this counts as fully transparent, matching how the sprites are authored.
const ALPHA_CUTOFF: u8 = 128;

/// One decoded image, cropped to its alpha bounding box.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA, already cropped.
    pub pixels: Vec<[u8; 4]>,
}

/// The finished blob plus the directory the runtime indexes into.
pub struct SpriteBlob {
    pub bytes: Vec<u8>,
    /// `(offset, len)` per stored sprite.
    pub directory: Vec<(u32, u32)>,
    /// Maps a repo-relative sprite path to its index in `directory`.
    pub index_of_path: HashMap<String, u32>,
    pub max_palette: usize,
    pub decoded_count: usize,
    pub raw_png_bytes: u64,
}

impl SpriteBlob {
    pub fn new() -> SpriteBlob {
        SpriteBlob {
            bytes: Vec::new(),
            directory: Vec::new(),
            index_of_path: HashMap::new(),
            max_palette: 0,
            decoded_count: 0,
            raw_png_bytes: 0,
        }
    }

    /// Encodes one sprite file and returns its directory index, reusing an earlier entry when the
    /// file or its pixels have already been seen.
    ///
    /// # Arguments
    /// * `root` - the sprites clone root
    /// * `rel` - repo-relative path such as `sprites/pokemon/25.png`
    /// * `dedupe` - content hash to directory index, shared across the whole run
    ///
    /// # Returns
    /// the directory index, or an error naming the file that failed
    pub fn add(
        &mut self,
        root: &Path,
        rel: &str,
        dedupe: &mut HashMap<Vec<u8>, u32>,
    ) -> Result<u32, String> {
        if let Some(idx) = self.index_of_path.get(rel) {
            return Ok(*idx);
        }

        let path = root.join(rel);
        let raw = fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        self.raw_png_bytes += raw.len() as u64;
        let bitmap = decode_png(&raw).map_err(|e| format!("cannot decode {rel}: {e}"))?;
        self.decoded_count += 1;

        let (encoded, palette_len) = encode(&bitmap)?;
        self.max_palette = self.max_palette.max(palette_len);

        // the encoded bytes are the content hash: same pixels means the same encoding
        let idx = match dedupe.get(&encoded) {
            Some(idx) => *idx,
            None => {
                let offset = u32::try_from(self.bytes.len())
                    .map_err(|_| "sprite blob grew past 4 GiB".to_string())?;
                let len = u32::try_from(encoded.len()).unwrap();
                self.bytes.extend_from_slice(&encoded);
                let idx = u32::try_from(self.directory.len()).unwrap();
                self.directory.push((offset, len));
                dedupe.insert(encoded, idx);
                idx
            }
        };
        self.index_of_path.insert(rel.to_string(), idx);
        Ok(idx)
    }
}

/// Decodes a PNG into cropped RGBA.
///
/// # Arguments
/// * `raw` - the PNG file bytes
///
/// # Returns
/// the cropped bitmap, or a decode error
pub fn decode_png(raw: &[u8]) -> Result<Bitmap, String> {
    let mut decoder = png::Decoder::new(raw);
    // expand palettes and low bit depths up to plain 8-bit channels, and drop 16-bit down to 8
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return Err("palette was not expanded".to_string()),
    };
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!("unexpected bit depth {:?}", info.bit_depth));
    }

    let width = info.width as usize;
    let height = info.height as usize;
    let mut pixels = Vec::with_capacity(width * height);
    for i in 0..width * height {
        let px = &buf[i * channels..i * channels + channels];
        pixels.push(match channels {
            4 => [px[0], px[1], px[2], px[3]],
            3 => [px[0], px[1], px[2], 255],
            2 => [px[0], px[0], px[0], px[1]],
            _ => [px[0], px[0], px[0], 255],
        });
    }

    Ok(crop_to_alpha(width as u32, height as u32, &pixels))
}

/// Trims fully transparent rows and columns off every side.
fn crop_to_alpha(width: u32, height: u32, pixels: &[[u8; 4]]) -> Bitmap {
    let opaque = |x: u32, y: u32| pixels[(y * width + x) as usize][3] >= ALPHA_CUTOFF;

    let mut min_x = width;
    let mut max_x = 0u32;
    let mut min_y = height;
    let mut max_y = 0u32;
    for y in 0..height {
        for x in 0..width {
            if opaque(x, y) {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if min_x > max_x {
        return Bitmap {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
    }

    let cw = max_x - min_x + 1;
    let ch = max_y - min_y + 1;
    let mut out = Vec::with_capacity((cw * ch) as usize);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = pixels[(y * width + x) as usize];
            // normalize anything under the cutoff to a single transparent value so near-identical
            // sprites still dedupe
            out.push(if px[3] >= ALPHA_CUTOFF {
                [px[0], px[1], px[2], 255]
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    Bitmap {
        width: cw,
        height: ch,
        pixels: out,
    }
}

/// Encodes a cropped bitmap into the runtime's sprite format.
///
/// # Arguments
/// * `bitmap` - the cropped image
///
/// # Returns
/// the encoded bytes and the palette length including the transparent slot
fn encode(bitmap: &Bitmap) -> Result<(Vec<u8>, usize), String> {
    if bitmap.width > 255 || bitmap.height > 255 {
        return Err(format!(
            "sprite is {}x{}, the format stores size in one byte each",
            bitmap.width, bitmap.height
        ));
    }

    // index 0 is transparent, opaque colors get 1.. in first-seen order
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut lookup: HashMap<[u8; 3], u32> = HashMap::new();
    let mut indices = Vec::with_capacity(bitmap.pixels.len());
    for px in &bitmap.pixels {
        if px[3] < ALPHA_CUTOFF {
            indices.push(0u32);
            continue;
        }
        let rgb = [px[0], px[1], px[2]];
        let next = u32::try_from(palette.len()).unwrap() + 1;
        let idx = *lookup.entry(rgb).or_insert_with(|| {
            palette.push(rgb);
            next
        });
        indices.push(idx);
    }

    let mut out = Vec::with_capacity(indices.len() / 2 + palette.len() * 3 + 8);
    out.push(bitmap.width as u8);
    out.push(bitmap.height as u8);
    let ncolors =
        u16::try_from(palette.len()).map_err(|_| "palette over 65535 colors".to_string())?;
    out.extend_from_slice(&ncolors.to_le_bytes());
    for rgb in &palette {
        out.extend_from_slice(rgb);
    }

    let mut i = 0;
    while i < indices.len() {
        let idx = indices[i];
        let mut run = 1usize;
        while i + run < indices.len() && indices[i + run] == idx {
            run += 1;
        }
        i += run;
        write_runs(&mut out, idx, run);
    }

    Ok((out, palette.len() + 1))
}

/// Writes one logical run, splitting it into as many tokens as the format needs.
fn write_runs(out: &mut Vec<u8>, idx: u32, mut run: usize) {
    while run > 0 {
        if idx < 16 && run <= 8 {
            out.push(((idx as u8) << 3) | (run as u8 - 1));
            run = 0;
        } else if idx < 64 {
            let take = run.min(256);
            out.push(0x80 | idx as u8);
            out.push((take - 1) as u8);
            run -= take;
        } else {
            let take = run.min(u16::MAX as usize);
            out.push(0xC0);
            out.extend_from_slice(&(idx as u16).to_le_bytes());
            out.extend_from_slice(&(take as u16).to_le_bytes());
            run -= take;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// round tripping the encoder through the runtime's decoder should give the pixels back
    #[test]
    fn encode_round_trips() {
        let mut pixels = Vec::new();
        for y in 0..7u32 {
            for x in 0..5u32 {
                pixels.push(if (x + y) % 3 == 0 {
                    [0, 0, 0, 0]
                } else {
                    [(x * 40) as u8, (y * 30) as u8, 9, 255]
                });
            }
        }
        let bitmap = Bitmap {
            width: 5,
            height: 7,
            pixels: pixels.clone(),
        };
        let (bytes, _) = encode(&bitmap).unwrap();

        // mirror of the runtime decoder, kept here so a format change breaks a test
        assert_eq!(bytes[0], 5);
        assert_eq!(bytes[1], 7);
        let ncolors = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        let pal_end = 4 + ncolors * 3;
        let mut got = Vec::new();
        let mut p = pal_end;
        while p < bytes.len() {
            let c = bytes[p];
            let (idx, len) = if c & 0x80 == 0 {
                p += 1;
                (((c >> 3) & 0x0F) as usize, (c & 0x07) as usize + 1)
            } else if c & 0x40 == 0 {
                let len = bytes[p + 1] as usize + 1;
                p += 2;
                ((c & 0x3F) as usize, len)
            } else {
                let idx = u16::from_le_bytes([bytes[p + 1], bytes[p + 2]]) as usize;
                let len = u16::from_le_bytes([bytes[p + 3], bytes[p + 4]]) as usize;
                p += 5;
                (idx, len)
            };
            for _ in 0..len {
                got.push(if idx == 0 {
                    [0, 0, 0, 0]
                } else {
                    let o = 4 + (idx - 1) * 3;
                    [bytes[o], bytes[o + 1], bytes[o + 2], 255]
                });
            }
        }
        assert_eq!(got, pixels);
    }

    /// a run longer than 8 with a low index should use the two byte token
    #[test]
    fn long_runs_split() {
        let mut out = Vec::new();
        write_runs(&mut out, 1, 300);
        assert_eq!(out, vec![0x81, 255, 0x81, 43]);
    }
}
