//! Photoshop files, read for the picture they show.
//!
//! A PSD keeps its layers, and beside them a **composite**: the whole image
//! flattened, written so that programs which do not understand layers can still
//! show it. That composite is what a layout program places — the layers are the
//! retoucher's business, and a page prints what the retoucher saved. So this
//! reads the header, skips the sections it has no use for, and decodes the one
//! image at the end.
//!
//! Written here rather than taken from a crate because `zune-psd`, the
//! reader that would fit, treats the fourth channel of every file as alpha,
//! and in a CMYK file — the kind a press sends back — the fourth channel is
//! black.
//!
//! What it reads: PSD and the large-document PSB; 1-bit bitmaps, and 8, 16
//! and 32 bits a channel; greyscale, duotone (whose composite is its
//! greyscale), indexed, RGB, CMYK, Lab and multichannel; stored raw,
//! run-length encoded or ZIP-compressed with or without prediction; with the
//! transparency Photoshop writes when the layers leave some of the canvas
//! clear. A multichannel file is a stack of inks, each shown in the colour
//! its channel is given, printed one over another.

use std::path::Path;

/// The flattened picture a Photoshop file shows.
pub struct Composite {
    pub width: u32,
    pub height: u32,
    /// Straight RGBA, for the screen and for an RGB export.
    pub rgba: Vec<u8>,
    /// The file's own ink amounts, C, M, Y and K, a byte each and 255 for full
    /// ink, when it is a CMYK file. Kept so an export into a press's inks can
    /// write the numbers the retoucher set rather than a round trip through
    /// RGB that would move every one of them.
    pub inks: Option<Vec<u8>>,
}

/// Why a file would not read.
#[derive(Debug, PartialEq, Eq)]
pub enum Unreadable {
    NotPhotoshop,
    /// Cut off before the picture ended.
    Short,
    Unsupported(&'static str),
}

/// Whether this path is a Photoshop file, by its extension.
pub fn is_psd(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb"))
}

/// The size in pixels, from the header alone.
pub fn size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;

    let mut header = [0u8; 26];
    std::fs::File::open(path)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    let header = Header::read(&mut Bytes::new(&header)).ok()?;
    Some((header.width, header.height))
}

/// Read a Photoshop file's composite.
pub fn read(bytes: &[u8]) -> Result<Composite, Unreadable> {
    let mut at = Bytes::new(bytes);
    let header = Header::read(&mut at)?;
    let big = header.version == 2;

    let palette = at.take_sized(false)?;
    let resources = at.take_sized(false)?;
    let layers_and_masks = at.take_sized(big)?;
    // A negative layer count is how Photoshop says the first channel past the
    // colours is the composite's transparency; otherwise it is a saved
    // selection, which is not a thing that prints.
    let transparent = {
        let mut section = Bytes::new(layers_and_masks);
        section
            .take_sized(big)
            .ok()
            .is_some_and(|info| info.len() >= 2 && i16::from_be_bytes([info[0], info[1]]) < 0)
    };

    let colours = header.colours();
    if header.channels < colours {
        return Err(Unreadable::Unsupported(
            "fewer channels than its colours need",
        ));
    }
    let keep = colours + usize::from(transparent && header.channels > colours);
    let planes = decode_planes(&mut at, &header, keep, big)?;
    let multichannel =
        (header.mode == Mode::Multichannel).then(|| channel_inks(resources, colours));

    let pixels = header.width as usize * header.height as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    let mut inks = (header.mode == Mode::Cmyk).then(|| Vec::with_capacity(pixels * 4));
    // Planar in the file, a plane to a channel; gathered here a pixel at a
    // time across however many planes there are.
    let across = |channel: usize, i: usize| planes[channel][i];
    for i in 0..pixels {
        let sample = |channel: usize| across(channel, i);
        let (r, g, b) = match header.mode {
            Mode::Bitmap | Mode::Grey => (sample(0), sample(0), sample(0)),
            Mode::Indexed => {
                let entry = usize::from(sample(0));
                // Stored as 256 reds, then 256 greens, then 256 blues.
                let at = |plane: usize| palette.get(plane * 256 + entry).copied().unwrap_or(0);
                (at(0), at(1), at(2))
            }
            Mode::Rgb => (sample(0), sample(1), sample(2)),
            Mode::Lab => lab_to_srgb(sample(0), sample(1), sample(2)),
            Mode::Multichannel => {
                // Each channel is an ink, stored inverted as CMYK's are, and
                // inks print over one another: every one takes away the
                // light its colour does not reflect, in proportion to how
                // much of it there is.
                let mut light = [1.0f64; 3];
                for (channel, ink) in multichannel.iter().flatten().enumerate() {
                    let amount = f64::from(255 - sample(channel)) / 255.0;
                    for (l, i) in light.iter_mut().zip(ink) {
                        *l *= 1.0 - amount * (1.0 - f64::from(*i) / 255.0);
                    }
                }
                let byte = |v: f64| (v * 255.0).round() as u8;
                (byte(light[0]), byte(light[1]), byte(light[2]))
            }
            Mode::Cmyk => {
                // Stored inverted: 255 is no ink. What is shown is the plain
                // arithmetic, not a press's profile: a picture on screen, not
                // a proof.
                let [c, m, y, k] = [sample(0), sample(1), sample(2), sample(3)];
                if let Some(inks) = inks.as_mut() {
                    inks.extend_from_slice(&[255 - c, 255 - m, 255 - y, 255 - k]);
                }
                let mix = |v: u8| ((u16::from(v) * u16::from(k) + 127) / 255) as u8;
                (mix(c), mix(m), mix(y))
            }
        };
        let alpha = if keep > colours { sample(colours) } else { 255 };
        rgba.extend_from_slice(&unmatte([r, g, b], alpha));
    }
    if keep > colours
        && let Some(inks) = inks.as_mut()
    {
        // Ink under clear pixels is the white matte, and white is no ink:
        // the matte is taken out of the inks as it is out of the colours, and
        // with white being none of any ink, that is a division by coverage.
        for (ink, &alpha) in inks.as_chunks_mut::<4>().0.iter_mut().zip(&planes[colours]) {
            for amount in ink.iter_mut() {
                *amount = match alpha {
                    0 => 0,
                    255 => *amount,
                    a => (u16::from(*amount) * 255 / u16::from(a)).min(255) as u8,
                };
            }
        }
    }

    Ok(Composite {
        width: header.width,
        height: header.height,
        rgba,
        inks,
    })
}

/// Photoshop flattens a transparent composite onto white, so a half-clear
/// pixel carries half white in its colour. Taking the white back out gives the
/// straight colour that blends correctly over whatever is behind the frame.
fn unmatte([r, g, b]: [u8; 3], alpha: u8) -> [u8; 4] {
    if alpha == 0 || alpha == 255 {
        return [r, g, b, alpha];
    }
    let a = f32::from(alpha) / 255.0;
    let straight = |v: u8| {
        ((f32::from(v) - 255.0 * (1.0 - a)) / a)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [straight(r), straight(g), straight(b), alpha]
}

/// A Lab sample — L from 0 to 255 for 0 to 100, a and b offset by 128 —
/// as sRGB. Photoshop's Lab is relative to D50; the matrix takes XYZ under
/// D50 to linear sRGB with the Bradford adaptation to D65 folded in.
fn lab_to_srgb(l: u8, a: u8, b: u8) -> (u8, u8, u8) {
    let l = f64::from(l) * 100.0 / 255.0;
    let a = f64::from(a) - 128.0;
    let b = f64::from(b) - 128.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inverse = |t: f64| {
        const E: f64 = 6.0 / 29.0;
        if t > E {
            t * t * t
        } else {
            3.0 * E * E * (t - 4.0 / 29.0)
        }
    };
    // D50 white.
    let (x, y, z) = (0.9642 * inverse(fx), inverse(fy), 0.8249 * inverse(fz));
    let r = 3.133_856_1 * x - 1.616_866_7 * y - 0.490_614_6 * z;
    let g = -0.978_768_4 * x + 1.916_141_5 * y + 0.033_454_0 * z;
    let bl = 0.071_945_3 * x - 0.228_991_4 * y + 1.405_242_7 * z;
    (encode(r), encode(g), encode(bl))
}

/// A linear light value, 0 to 1, as an sRGB byte.
fn encode(linear: f64) -> u8 {
    let v = linear.clamp(0.0, 1.0);
    let v = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round() as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// One bit a pixel, black and white.
    Bitmap,
    Grey,
    Indexed,
    Rgb,
    Cmyk,
    Lab,
    /// Every channel an ink of its own, spot colours or process.
    Multichannel,
}

impl Header {
    /// How many channels are colour; any past them are transparency or
    /// saved selections. A multichannel file's channels are all inks.
    fn colours(&self) -> usize {
        match self.mode {
            Mode::Bitmap | Mode::Grey | Mode::Indexed => 1,
            Mode::Rgb | Mode::Lab => 3,
            Mode::Cmyk => 4,
            Mode::Multichannel => self.channels,
        }
    }
}

/// The colour each of a multichannel file's `count` channels prints in, as
/// sRGB: from the file's display information (resource 1077, or 1007 in files
/// before Photoshop 6), and where it gives none, cyan, magenta, yellow and
/// black in turn — which is what a CMYK file converted to multichannel holds —
/// then black.
fn channel_inks(resources: &[u8], count: usize) -> Vec<[u8; 3]> {
    const PROCESS: [[u8; 3]; 4] = [[0, 255, 255], [255, 0, 255], [255, 255, 0], [0, 0, 0]];
    let given = display_info(resources);
    (0..count)
        .map(|i| {
            given
                .get(i)
                .copied()
                .flatten()
                .or_else(|| PROCESS.get(i).copied())
                .unwrap_or([0, 0, 0])
        })
        .collect()
}

/// The channels' display colours, where the resources hold them: `None` for
/// one in a colour space not read here.
fn display_info(resources: &[u8]) -> Vec<Option<[u8; 3]>> {
    let mut at = Bytes::new(resources);
    let mut older = None;
    while at.remaining() >= 12 {
        let Ok(block) = resource(&mut at) else {
            break;
        };
        match block {
            // A version, then thirteen bytes a channel.
            (1077, data) if data.len() >= 4 => {
                return data[4..].chunks_exact(13).map(display_colour).collect();
            }
            // Fourteen bytes a channel, the last padding.
            (1007, data) => older = Some(data.chunks_exact(14).map(display_colour).collect()),
            _ => {}
        }
    }
    older.unwrap_or_default()
}

/// One image resource: its id and its data.
fn resource<'a>(at: &mut Bytes<'a>) -> Result<(u16, &'a [u8]), Unreadable> {
    if at.take(4)? != b"8BIM" {
        return Err(Unreadable::Short);
    }
    let id = at.u16()?;
    // A Pascal string, padded so its length byte and it are even.
    let name = usize::from(at.take(1)?[0]);
    at.take(name + (name + 1) % 2)?;
    let size = at.u32()? as usize;
    let data = at.take(size)?;
    if size % 2 == 1 {
        at.take(1)?;
    }
    Ok((id, data))
}

/// A Photoshop colour: a colour space, then four sixteen-bit components.
fn display_colour(entry: &[u8]) -> Option<[u8; 3]> {
    let word = |i: usize| u16::from_be_bytes([entry[2 + 2 * i], entry[3 + 2 * i]]);
    let high = |v: u16| (v >> 8) as u8;
    match u16::from_be_bytes([entry[0], entry[1]]) {
        0 => Some([high(word(0)), high(word(1)), high(word(2))]),
        // CMYK, where nought is full ink: the plain arithmetic, as for a
        // CMYK file's composite.
        2 => {
            let [c, m, y, k] = [0, 1, 2, 3].map(|i| f64::from(word(i)) / 65535.0);
            let byte = |v: f64| (v * k * 255.0).round() as u8;
            Some([byte(c), byte(m), byte(y)])
        }
        // Lab: L 0 to 10000, a and b signed hundredths.
        7 => {
            let l = f64::from(word(0)) / 10000.0 * 255.0;
            let signed = |v: u16| f64::from(v as i16) / 100.0 + 128.0;
            let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
            let (r, g, b) = lab_to_srgb(byte(l), byte(signed(word(1))), byte(signed(word(2))));
            Some([r, g, b])
        }
        // Grey, 0 to 10000 of black.
        8 => {
            let v = 255 - (f64::from(word(0).min(10000)) / 10000.0 * 255.0).round() as u8;
            Some([v, v, v])
        }
        _ => None,
    }
}

struct Header {
    version: u16,
    channels: usize,
    width: u32,
    height: u32,
    depth: u16,
    mode: Mode,
}

impl Header {
    fn read(at: &mut Bytes<'_>) -> Result<Self, Unreadable> {
        if at.take(4)? != b"8BPS" {
            return Err(Unreadable::NotPhotoshop);
        }
        let version = at.u16()?;
        if version != 1 && version != 2 {
            return Err(Unreadable::NotPhotoshop);
        }
        at.take(6)?;
        let channels = usize::from(at.u16()?);
        let height = at.u32()?;
        let width = at.u32()?;
        let depth = at.u16()?;
        let mode = match at.u16()? {
            0 => Mode::Bitmap,
            1 | 8 => Mode::Grey,
            2 => Mode::Indexed,
            3 => Mode::Rgb,
            4 => Mode::Cmyk,
            7 => Mode::Multichannel,
            9 => Mode::Lab,
            _ => return Err(Unreadable::Unsupported("this colour mode")),
        };
        if !matches!(depth, 1 | 8 | 16 | 32) {
            return Err(Unreadable::Unsupported("this bit depth"));
        }
        if (mode == Mode::Bitmap) != (depth == 1) {
            return Err(Unreadable::Unsupported(
                "a bitmap's depth is one bit, and only a bitmap's",
            ));
        }
        if mode == Mode::Indexed && depth != 8 {
            return Err(Unreadable::Unsupported("indexed colour past 8 bits"));
        }
        if width == 0 || height == 0 {
            return Err(Unreadable::Unsupported("an empty canvas"));
        }
        Ok(Header {
            version,
            channels,
            width,
            height,
            depth,
            mode,
        })
    }
}

/// The first `keep` channels of the composite, a byte a sample.
///
/// Sixteen-bit samples keep their high byte: the screen and the PDF writer
/// both work in eight, and the low byte is below what either shows.
/// Thirty-two-bit samples are linear light, floats: colour is encoded as
/// sRGB, transparency kept linear, as it always is. A bitmap's bits are
/// black for one, white for nought.
fn decode_planes(
    at: &mut Bytes<'_>,
    header: &Header,
    keep: usize,
    big: bool,
) -> Result<Vec<Vec<u8>>, Unreadable> {
    let colours = header.colours();
    let raw = decode_raw(at, header, keep, big)?;
    let width = header.width as usize;
    let row_bytes = (width * usize::from(header.depth)).div_ceil(8);
    Ok(raw
        .into_iter()
        .enumerate()
        .map(|(channel, plane)| {
            plane
                .chunks(row_bytes.max(1))
                .flat_map(|row| to_eight(row, header.depth, width, channel >= colours))
                .collect()
        })
        .collect())
}

/// One row of samples at `depth` bits, a byte each: see [`decode_planes`].
fn to_eight(row: &[u8], depth: u16, width: usize, alpha: bool) -> Vec<u8> {
    match depth {
        1 => (0..width)
            .map(|x| {
                let bit = row.get(x / 8).is_some_and(|b| b & (0x80 >> (x % 8)) != 0);
                if bit { 0 } else { 255 }
            })
            .collect(),
        8 => row.to_vec(),
        16 => high_bytes(row, 16).collect(),
        32 => row
            .chunks_exact(4)
            .map(|b| {
                let v = f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]]));
                if alpha {
                    (v.clamp(0.0, 1.0) * 255.0).round() as u8
                } else {
                    encode(v)
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The first `keep` channels' rows as the file stores them, uncompressed.
fn decode_raw(
    at: &mut Bytes<'_>,
    header: &Header,
    keep: usize,
    big: bool,
) -> Result<Vec<Vec<u8>>, Unreadable> {
    let width = header.width as usize;
    let height = header.height as usize;
    let row_bytes = (width * usize::from(header.depth)).div_ceil(8);
    // A header can claim a canvas far bigger than the file holds. Nothing is
    // sized from it past what the file has left, so the lie is found by
    // running out of bytes rather than by asking for gigabytes first.
    let room = |at: &Bytes<'_>, wanted: usize| wanted.min(at.remaining());
    let pixels = width.saturating_mul(height);

    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(keep);
    match at.u16()? {
        0 => {
            for _ in 0..keep {
                let mut plane = Vec::with_capacity(room(at, pixels));
                for _ in 0..height {
                    plane.extend_from_slice(at.take(row_bytes)?);
                }
                planes.push(plane);
            }
        }
        1 => {
            // Every row's packed length first, for every channel, then the
            // rows themselves one after another.
            let rows = header.channels.saturating_mul(height);
            let mut lengths = Vec::with_capacity(room(at, rows));
            for _ in 0..rows {
                lengths.push(if big {
                    at.u32()? as usize
                } else {
                    usize::from(at.u16()?)
                });
            }
            // Grown as rows arrive: a packed row can claim a width its few
            // bytes never fill.
            let mut row = Vec::new();
            for channel in lengths.chunks(height).take(keep) {
                let mut plane = Vec::new();
                for &length in channel {
                    unpack(at.take(length)?, row_bytes, &mut row)?;
                    plane.extend_from_slice(&row);
                }
                planes.push(plane);
            }
        }
        // ZIP: every channel's rows in one zlib stream, and with
        // prediction each row stored as differences along it.
        compression @ (2 | 3) => {
            use std::io::Read as _;
            let plane_bytes = row_bytes.saturating_mul(height);
            let wanted = plane_bytes.saturating_mul(keep);
            let mut data = Vec::with_capacity(room(at, wanted));
            flate2::read::ZlibDecoder::new(at.take(at.remaining())?)
                .take(wanted as u64)
                .read_to_end(&mut data)
                .map_err(|_| Unreadable::Unsupported("a damaged ZIP stream"))?;
            if data.len() < wanted {
                return Err(Unreadable::Short);
            }
            if compression == 3 {
                for row in data.chunks_mut(row_bytes.max(1)) {
                    unpredict(row, header.depth);
                }
            }
            for channel in data.chunks(plane_bytes.max(1)).take(keep) {
                planes.push(channel.to_vec());
            }
        }
        _ => return Err(Unreadable::Unsupported("this compression")),
    }
    Ok(planes)
}

/// Undo Photoshop's ZIP prediction on one row: each sample stored as its
/// difference from the one before. Sixteen-bit samples are differenced as
/// numbers; thirty-two-bit rows are stored as four planes of bytes, first
/// bytes then second and so on, differenced as bytes along the whole row.
fn unpredict(row: &mut [u8], depth: u16) {
    match depth {
        16 => {
            let mut previous = 0u16;
            for pair in row.chunks_exact_mut(2) {
                let v = u16::from_be_bytes([pair[0], pair[1]]).wrapping_add(previous);
                pair.copy_from_slice(&v.to_be_bytes());
                previous = v;
            }
        }
        32 => {
            for i in 1..row.len() {
                row[i] = row[i].wrapping_add(row[i - 1]);
            }
            let n = row.len() / 4;
            let planar = row.to_vec();
            for x in 0..n {
                for byte in 0..4 {
                    row[x * 4 + byte] = planar[byte * n + x];
                }
            }
        }
        _ => {
            for i in 1..row.len() {
                row[i] = row[i].wrapping_add(row[i - 1]);
            }
        }
    }
}

fn high_bytes(row: &[u8], depth: u16) -> impl Iterator<Item = u8> + '_ {
    row.iter().step_by(usize::from(depth / 8)).copied()
}

/// One PackBits row, which must come out exactly `length` bytes long.
fn unpack(packed: &[u8], length: usize, out: &mut Vec<u8>) -> Result<(), Unreadable> {
    out.clear();
    let mut at = Bytes::new(packed);
    while out.len() < length {
        let n = at.take(1)?[0] as i8;
        match n {
            0.. => out.extend_from_slice(at.take(n as usize + 1)?),
            // A no-op byte, by the format's own definition.
            -128 => {}
            _ => {
                let value = at.take(1)?[0];
                out.resize(out.len() + (1 - n as isize) as usize, value);
            }
        }
    }
    if out.len() == length {
        Ok(())
    } else {
        Err(Unreadable::Unsupported("a row that runs past its width"))
    }
}

/// A cursor over the file, in big-endian, that says so when it runs out.
struct Bytes<'a> {
    data: &'a [u8],
}

impl<'a> Bytes<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bytes { data }
    }

    fn remaining(&self) -> usize {
        self.data.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Unreadable> {
        if n > self.data.len() {
            return Err(Unreadable::Short);
        }
        let (head, rest) = self.data.split_at(n);
        self.data = rest;
        Ok(head)
    }

    fn u16(&mut self) -> Result<u16, Unreadable> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, Unreadable> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A section that starts with its own length: four bytes, or eight in a
    /// large document where `big` says the section may be that long.
    fn take_sized(&mut self, big: bool) -> Result<&'a [u8], Unreadable> {
        let length = if big {
            let b = self.take(8)?;
            u64::from_be_bytes(b.try_into().expect("eight bytes"))
        } else {
            u64::from(self.u32()?)
        };
        self.take(usize::try_from(length).map_err(|_| Unreadable::Short)?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A Photoshop file with the given planes, written by hand.
    ///
    /// `packed` run-length encodes every row the way Photoshop does, with a
    /// run wherever three bytes or more repeat, so both kinds of PackBits
    /// record are read back.
    pub(crate) struct Psd<'a> {
        pub version: u16,
        pub mode: u16,
        pub depth: u16,
        pub width: u32,
        pub height: u32,
        pub planes: Vec<Vec<u8>>,
        pub palette: &'a [u8],
        pub transparent: bool,
        pub packed: bool,
    }

    impl Default for Psd<'_> {
        fn default() -> Self {
            Psd {
                version: 1,
                mode: 3,
                depth: 8,
                width: 2,
                height: 2,
                planes: Vec::new(),
                palette: &[],
                transparent: false,
                packed: false,
            }
        }
    }

    impl Psd<'_> {
        pub(crate) fn bytes(&self) -> Vec<u8> {
            let big = self.version == 2;
            let mut out = b"8BPS".to_vec();
            out.extend(self.version.to_be_bytes());
            out.extend([0; 6]);
            out.extend((self.planes.len() as u16).to_be_bytes());
            out.extend(self.height.to_be_bytes());
            out.extend(self.width.to_be_bytes());
            out.extend(self.depth.to_be_bytes());
            out.extend(self.mode.to_be_bytes());
            out.extend((self.palette.len() as u32).to_be_bytes());
            out.extend(self.palette);
            // Image resources: one block, the print resolution, which is
            // skipped — a placed picture is measured a pixel to the point.
            let mut resolution = b"8BIM".to_vec();
            resolution.extend(1005u16.to_be_bytes());
            resolution.extend([0, 0]); // an empty name, padded to even
            resolution.extend(16u32.to_be_bytes());
            resolution.extend([0; 16]);
            out.extend((resolution.len() as u32).to_be_bytes());
            out.extend(resolution);
            // Layer and mask information, whose layer info says whether the
            // composite's first extra channel is its transparency.
            let count: i16 = if self.transparent { -1 } else { 1 };
            let mut info = count.to_be_bytes().to_vec();
            info.extend([0; 4]);
            let length = |n: usize| -> Vec<u8> {
                if big {
                    (n as u64).to_be_bytes().to_vec()
                } else {
                    (n as u32).to_be_bytes().to_vec()
                }
            };
            let mut section = length(info.len());
            section.extend(&info);
            out.extend(length(section.len()));
            out.extend(section);

            let row = self.width as usize * usize::from(self.depth / 8);
            out.extend(u16::from(self.packed).to_be_bytes());
            if !self.packed {
                for plane in &self.planes {
                    out.extend(plane);
                }
                return out;
            }
            let rows: Vec<Vec<u8>> = self
                .planes
                .iter()
                .flat_map(|plane| plane.chunks(row).map(pack))
                .collect();
            for packed in &rows {
                if big {
                    out.extend((packed.len() as u32).to_be_bytes());
                } else {
                    out.extend((packed.len() as u16).to_be_bytes());
                }
            }
            for packed in rows {
                out.extend(packed);
            }
            out
        }
    }

    fn pack(row: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < row.len() {
            let run = row[i..]
                .iter()
                .take_while(|&&b| b == row[i])
                .count()
                .min(128);
            if run >= 3 {
                out.push((1 - run as isize) as i8 as u8);
                out.push(row[i]);
                i += run;
            } else {
                let start = i;
                while i < row.len()
                    && i - start < 128
                    && row[i..].iter().take_while(|&&b| b == row[i]).count() < 3
                {
                    i += 1;
                }
                out.push((i - start - 1) as u8);
                out.extend(&row[start..i]);
            }
        }
        out
    }

    fn pixel(c: &Composite, i: usize) -> [u8; 4] {
        c.rgba[i * 4..i * 4 + 4].try_into().unwrap()
    }

    #[test]
    fn an_rgb_file_reads_raw_and_packed_alike() {
        let planes = vec![
            vec![255, 0, 0, 10, 10, 10, 10, 10],
            vec![0, 255, 0, 20, 20, 20, 20, 20],
            vec![0, 0, 255, 30, 30, 30, 30, 30],
        ];
        for packed in [false, true] {
            let file = Psd {
                width: 4,
                planes: planes.clone(),
                packed,
                ..Default::default()
            };
            let c = read(&file.bytes()).expect("reads");
            assert_eq!((c.width, c.height), (4, 2));
            assert_eq!(pixel(&c, 0), [255, 0, 0, 255], "packed: {packed}");
            assert_eq!(pixel(&c, 1), [0, 255, 0, 255], "packed: {packed}");
            assert_eq!(pixel(&c, 2), [0, 0, 255, 255], "packed: {packed}");
            assert_eq!(pixel(&c, 7), [10, 20, 30, 255], "packed: {packed}");
            assert!(c.inks.is_none());
        }
    }

    #[test]
    fn a_large_document_reads_like_any_other() {
        // PSB: the same file with eight-byte section lengths and four-byte
        // row lengths, for canvases past thirty thousand pixels.
        let file = Psd {
            version: 2,
            width: 4,
            planes: vec![vec![1; 8], vec![2; 8], vec![3; 8]],
            packed: true,
            ..Default::default()
        };
        let c = read(&file.bytes()).expect("reads");
        assert_eq!(pixel(&c, 5), [1, 2, 3, 255]);
    }

    #[test]
    fn a_cmyk_file_keeps_its_inks_and_its_black() {
        // The case the crates get wrong: the fourth channel is black, not
        // alpha. Stored inverted, so 0 is full ink.
        let file = Psd {
            mode: 4,
            planes: vec![
                vec![0, 255, 255, 255],   // cyan
                vec![255, 255, 255, 255], // magenta
                vec![255, 255, 255, 255], // yellow
                vec![255, 0, 128, 255],   // black
            ],
            ..Default::default()
        };
        let c = read(&file.bytes()).expect("reads");
        assert_eq!(pixel(&c, 0), [0, 255, 255, 255], "full cyan is cyan");
        assert_eq!(
            pixel(&c, 1),
            [0, 0, 0, 255],
            "full black is black, not clear"
        );
        assert_eq!(pixel(&c, 2), [128, 128, 128, 255], "half black is grey");
        assert_eq!(pixel(&c, 3), [255, 255, 255, 255], "no ink is paper");
        let inks = c.inks.expect("the inks are kept");
        assert_eq!(&inks[..4], &[255, 0, 0, 0]);
        assert_eq!(&inks[4..8], &[0, 0, 0, 255]);
    }

    #[test]
    fn transparency_is_read_only_when_the_file_says_it_is_there() {
        // A fourth channel on an RGB file is a saved selection unless the
        // layer count is negative; a selection shown as transparency would
        // punch holes in a photograph.
        let planes = vec![vec![200; 4], vec![100; 4], vec![0; 4], vec![0, 255, 0, 255]];
        let selection = Psd {
            planes: planes.clone(),
            ..Default::default()
        };
        let c = read(&selection.bytes()).expect("reads");
        assert_eq!(pixel(&c, 0), [200, 100, 0, 255]);

        let clear = Psd {
            planes,
            transparent: true,
            ..Default::default()
        };
        let c = read(&clear.bytes()).expect("reads");
        assert_eq!(pixel(&c, 0)[3], 0);
        assert_eq!(pixel(&c, 1), [200, 100, 0, 255]);
    }

    #[test]
    fn a_half_clear_pixel_loses_the_white_it_was_flattened_on() {
        // Photoshop writes the composite over white: a half-clear red is
        // stored as pink, green and blue at the half of 255 the white gave. Shown as pink over a dark page it would glow.
        let file = Psd {
            width: 1,
            height: 1,
            planes: vec![vec![255], vec![127], vec![127], vec![128]],
            transparent: true,
            ..Default::default()
        };
        let c = read(&file.bytes()).expect("reads");
        let [r, g, b, a] = pixel(&c, 0);
        assert_eq!((r, a), (255, 128));
        assert!(g <= 1 && b <= 1, "the white stayed in: {g} {b}");

        // The same in inks: half-clear full cyan is stored as half cyan, and
        // whatever is under a wholly clear pixel is no ink at all.
        let file = Psd {
            mode: 4,
            width: 2,
            height: 1,
            planes: vec![
                vec![127, 0],
                vec![255, 255],
                vec![255, 255],
                vec![255, 255],
                vec![128, 0],
            ],
            transparent: true,
            ..Default::default()
        };
        let inks = read(&file.bytes()).expect("reads").inks.expect("inks");
        assert_eq!(&inks[..4], &[255, 0, 0, 0], "the white stayed in the cyan");
        assert_eq!(&inks[4..], &[0, 0, 0, 0], "a clear pixel prints ink");
    }

    #[test]
    fn grey_sixteen_bit_and_indexed_files_read() {
        let grey = Psd {
            mode: 1,
            depth: 16,
            width: 1,
            height: 2,
            planes: vec![vec![0x80, 0xFF, 0x20, 0x01]],
            ..Default::default()
        };
        let c = read(&grey.bytes()).expect("reads");
        assert_eq!(
            pixel(&c, 0),
            [0x80, 0x80, 0x80, 255],
            "the high byte is kept"
        );
        assert_eq!(pixel(&c, 1), [0x20, 0x20, 0x20, 255]);

        // A duotone's composite is the greyscale its inks are laid over.
        let duotone = Psd {
            mode: 8,
            width: 1,
            height: 1,
            planes: vec![vec![0x44]],
            ..Default::default()
        };
        assert_eq!(
            pixel(&read(&duotone.bytes()).expect("reads"), 0),
            [0x44, 0x44, 0x44, 255]
        );

        let mut palette = vec![0u8; 768];
        palette[3] = 250; // entry 3's red
        palette[256 + 3] = 150; // green
        palette[512 + 3] = 50; // blue
        let indexed = Psd {
            mode: 2,
            width: 1,
            height: 1,
            planes: vec![vec![3]],
            palette: &palette,
            ..Default::default()
        };
        let c = read(&indexed.bytes()).expect("reads");
        assert_eq!(pixel(&c, 0), [250, 150, 50, 255]);
    }

    /// The same file with its composite ZIP-compressed: the raw planes
    /// after the compression word, deflated, with or without prediction.
    fn zipped(raw: &Psd<'_>, predict: bool) -> Vec<u8> {
        use std::io::Write as _;
        let plain = raw.bytes();
        let data: usize = raw.planes.iter().map(Vec::len).sum();
        let head = plain.len() - data - 2;
        let mut planes: Vec<u8> = raw.planes.concat();
        let row = (raw.width as usize * usize::from(raw.depth)).div_ceil(8);
        if predict {
            for r in planes.chunks_mut(row) {
                match raw.depth {
                    16 => {
                        let mut previous = 0u16;
                        for pair in r.chunks_exact_mut(2) {
                            let v = u16::from_be_bytes([pair[0], pair[1]]);
                            pair.copy_from_slice(&v.wrapping_sub(previous).to_be_bytes());
                            previous = v;
                        }
                    }
                    32 => {
                        let n = r.len() / 4;
                        let pixels = r.to_vec();
                        for x in 0..n {
                            for byte in 0..4 {
                                r[byte * n + x] = pixels[x * 4 + byte];
                            }
                        }
                        for i in (1..r.len()).rev() {
                            r[i] = r[i].wrapping_sub(r[i - 1]);
                        }
                    }
                    _ => {
                        for i in (1..r.len()).rev() {
                            r[i] = r[i].wrapping_sub(r[i - 1]);
                        }
                    }
                }
            }
        }
        let mut out = plain[..head].to_vec();
        out.extend(if predict { 3u16 } else { 2u16 }.to_be_bytes());
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&planes).unwrap();
        out.extend(z.finish().unwrap());
        out
    }

    #[test]
    fn a_zip_compressed_composite_reads_with_and_without_prediction() {
        let rgb = Psd {
            width: 3,
            height: 2,
            planes: vec![
                vec![10, 20, 30, 40, 50, 60],
                vec![200, 190, 180, 170, 160, 150],
                vec![0, 255, 0, 255, 0, 255],
            ],
            ..Default::default()
        };
        let plain = read(&rgb.bytes()).expect("raw");
        for predict in [false, true] {
            let zip = read(&zipped(&rgb, predict)).expect("zipped");
            assert_eq!(zip.rgba, plain.rgba, "predicted: {predict}");
        }
        // Sixteen bits, predicted as numbers: the high bytes come back.
        let deep = Psd {
            mode: 1,
            depth: 16,
            width: 2,
            height: 1,
            planes: vec![vec![0x12, 0x34, 0xAB, 0xCD]],
            ..Default::default()
        };
        let c = read(&zipped(&deep, true)).expect("zipped");
        assert_eq!((pixel(&c, 0)[0], pixel(&c, 1)[0]), (0x12, 0xAB));
    }

    #[test]
    fn thirty_two_bit_light_is_encoded_as_srgb() {
        // Linear 0, 0.2140 (sRGB 128) and 1, one pixel each, in grey; the
        // same file ZIP-compressed with its byte planes predicted.
        let floats = [0.0f32, 0.214_041_14, 1.0];
        let grey = Psd {
            mode: 1,
            depth: 32,
            width: 3,
            height: 1,
            planes: vec![floats.iter().flat_map(|f| f.to_be_bytes()).collect()],
            ..Default::default()
        };
        for bytes in [grey.bytes(), zipped(&grey, true)] {
            let c = read(&bytes).expect("reads");
            assert_eq!(pixel(&c, 0), [0, 0, 0, 255]);
            assert_eq!(pixel(&c, 1)[0], 128);
            assert_eq!(pixel(&c, 2), [255, 255, 255, 255]);
        }
    }

    #[test]
    fn a_bitmap_is_black_for_one() {
        // Ten pixels a row, so the row is two bytes and the last six bits
        // are padding: 1010000000 on one row.
        let bitmap = Psd {
            mode: 0,
            depth: 1,
            width: 10,
            height: 1,
            planes: vec![vec![0b1010_0000, 0b0000_0000]],
            ..Default::default()
        };
        let c = read(&bitmap.bytes()).expect("reads");
        assert_eq!(c.rgba.len(), 10 * 4);
        assert_eq!(pixel(&c, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&c, 1), [255, 255, 255, 255]);
        assert_eq!(pixel(&c, 2), [0, 0, 0, 255]);
        assert_eq!(pixel(&c, 9), [255, 255, 255, 255]);
        // A bitmap claiming eight bits is not one.
        let wrong = Psd { depth: 8, ..bitmap };
        assert!(read(&wrong.bytes()).is_err());
    }

    #[test]
    fn multichannel_without_colours_is_cyan_magenta_yellow_black() {
        // Stored inverted: 0 is full ink. Pixel 0 no ink, 1 full cyan, 2 full
        // cyan and yellow, 3 full black.
        let multichannel = Psd {
            mode: 7,
            planes: vec![
                vec![255, 0, 0, 255],
                vec![255, 255, 255, 255],
                vec![255, 255, 0, 255],
                vec![255, 255, 255, 0],
            ],
            ..Default::default()
        };
        let c = read(&multichannel.bytes()).expect("reads");
        assert_eq!(pixel(&c, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(&c, 1), [0, 255, 255, 255]);
        assert_eq!(pixel(&c, 2), [0, 255, 0, 255]);
        assert_eq!(pixel(&c, 3), [0, 0, 0, 255]);
        assert!(c.inks.is_none(), "not a CMYK file's own numbers");
    }

    #[test]
    fn multichannel_inks_take_the_colours_the_file_gives() {
        // Display info (1077): version 1, then an RGB orange at half-strength
        // coverage in one pixel.
        let mut info = 1u32.to_be_bytes().to_vec();
        info.extend(0u16.to_be_bytes()); // RGB
        for v in [65535u16, 32896, 0, 0] {
            info.extend(v.to_be_bytes());
        }
        info.extend(100u16.to_be_bytes()); // solidity
        info.push(2); // a spot channel
        let mut resources = b"8BIM".to_vec();
        resources.extend(1077u16.to_be_bytes());
        resources.extend([0, 0]);
        resources.extend((info.len() as u32).to_be_bytes());
        resources.extend(&info);
        resources.push(0); // padded to even
        assert_eq!(
            channel_inks(&resources, 2),
            vec![[255, 128, 0], [255, 0, 255]]
        );
        // Where the display info stops, the process inks carry on.
    }

    #[test]
    fn lab_is_shown_as_srgb() {
        // White, black, and a strong red: L 54, a +81, b +70 is sRGB red.
        let lab = Psd {
            mode: 9,
            width: 3,
            height: 1,
            planes: vec![
                vec![255, 0, (53.24 * 2.55f64).round() as u8],
                vec![128, 128, 128 + 80],
                vec![128, 128, 128 + 67],
            ],
            ..Default::default()
        };
        let c = read(&lab.bytes()).expect("reads");
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 3);
        assert!(
            near(pixel(&c, 0), [255, 255, 255, 255]),
            "{:?}",
            pixel(&c, 0)
        );
        assert!(near(pixel(&c, 1), [0, 0, 0, 255]), "{:?}", pixel(&c, 1));
        let red = pixel(&c, 2);
        assert!(red[0] > 240 && red[1] < 30 && red[2] < 30, "{red:?}");
    }

    #[test]
    fn what_it_cannot_read_it_says_so() {
        assert_eq!(read(b"GIF89a").err(), Some(Unreadable::NotPhotoshop));
        let unknown = Psd {
            mode: 5,
            planes: vec![vec![0; 4]; 3],
            ..Default::default()
        };
        assert!(matches!(
            read(&unknown.bytes()),
            Err(Unreadable::Unsupported(_))
        ));

        let whole = Psd {
            planes: vec![vec![7; 4]; 3],
            packed: true,
            ..Default::default()
        }
        .bytes();
        for cut in [10, 30, whole.len() - 1] {
            assert!(read(&whole[..cut]).is_err(), "cut at {cut}");
        }

        // A header that claims far more canvas than the file holds.
        let mut liar = Psd {
            planes: vec![vec![7; 4]; 3],
            ..Default::default()
        }
        .bytes();
        liar[14..18].copy_from_slice(&60_000u32.to_be_bytes());
        liar[18..22].copy_from_slice(&60_000u32.to_be_bytes());
        assert_eq!(read(&liar).err(), Some(Unreadable::Short));
    }

    #[test]
    fn a_packed_row_reads_its_runs_its_copies_and_its_no_ops() {
        let mut row = Vec::new();
        // Two copied bytes, a no-op, then a run of three.
        unpack(&[1, 7, 8, 0x80, (-2i8) as u8, 9], 5, &mut row).expect("unpacks");
        assert_eq!(row, [7, 8, 9, 9, 9]);
        assert_eq!(
            unpack(&[(-3i8) as u8, 1], 3, &mut row).err(),
            Some(Unreadable::Unsupported("a row that runs past its width"))
        );
        assert_eq!(
            unpack(&[4, 1, 2], 5, &mut row).err(),
            Some(Unreadable::Short)
        );
    }

    #[test]
    fn the_size_comes_from_the_header() {
        let file = Psd {
            width: 3,
            height: 5,
            planes: vec![vec![0; 15]; 3],
            ..Default::default()
        };
        let path = std::env::temp_dir().join("tessera-psd-size.psd");
        std::fs::write(&path, file.bytes()).unwrap();
        assert_eq!(size(&path), Some((3, 5)));
        assert!(is_psd(&path) && is_psd(Path::new("big.PSB")));
        assert!(!is_psd(Path::new("photo.tif")));
    }
}
