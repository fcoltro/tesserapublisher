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
//! What it reads: PSD and the large-document PSB, 8 and 16 bits a channel,
//! greyscale, duotone (whose composite is its greyscale), indexed, RGB and
//! CMYK, stored raw or run-length encoded, with the transparency Photoshop
//! writes when the layers leave some of the canvas clear. What it does not —
//! 1-bit and 32-bit files, Lab, multichannel and ZIP-compressed composites — is
//! unreadable, which preflight reports as it does any other file it cannot
//! open.

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
    at.take_sized(false)?; // image resources
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

    let colours = header.mode.colour_channels();
    if header.channels < colours {
        return Err(Unreadable::Unsupported(
            "fewer channels than its colours need",
        ));
    }
    let keep = colours + usize::from(transparent && header.channels > colours);
    let planes = decode_planes(&mut at, &header, keep, big)?;

    let pixels = header.width as usize * header.height as usize;
    let mut rgba = Vec::with_capacity(pixels * 4);
    let mut inks = (header.mode == Mode::Cmyk).then(|| Vec::with_capacity(pixels * 4));
    // Planar in the file, a plane to a channel; gathered here a pixel at a
    // time across however many planes there are.
    let across = |channel: usize, i: usize| planes[channel][i];
    for i in 0..pixels {
        let sample = |channel: usize| across(channel, i);
        let (r, g, b) = match header.mode {
            Mode::Grey => (sample(0), sample(0), sample(0)),
            Mode::Indexed => {
                let entry = usize::from(sample(0));
                // Stored as 256 reds, then 256 greens, then 256 blues.
                let at = |plane: usize| palette.get(plane * 256 + entry).copied().unwrap_or(0);
                (at(0), at(1), at(2))
            }
            Mode::Rgb => (sample(0), sample(1), sample(2)),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Grey,
    Indexed,
    Rgb,
    Cmyk,
}

impl Mode {
    fn colour_channels(self) -> usize {
        match self {
            Mode::Grey | Mode::Indexed => 1,
            Mode::Rgb => 3,
            Mode::Cmyk => 4,
        }
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
            1 | 8 => Mode::Grey,
            2 => Mode::Indexed,
            3 => Mode::Rgb,
            4 => Mode::Cmyk,
            0 => return Err(Unreadable::Unsupported("1-bit bitmap")),
            9 => return Err(Unreadable::Unsupported("Lab colour")),
            _ => return Err(Unreadable::Unsupported("this colour mode")),
        };
        if depth != 8 && depth != 16 {
            return Err(Unreadable::Unsupported("this bit depth"));
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
fn decode_planes(
    at: &mut Bytes<'_>,
    header: &Header,
    keep: usize,
    big: bool,
) -> Result<Vec<Vec<u8>>, Unreadable> {
    let width = header.width as usize;
    let height = header.height as usize;
    let row_bytes = width * usize::from(header.depth / 8);
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
                    plane.extend(high_bytes(at.take(row_bytes)?, header.depth));
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
                    plane.extend(high_bytes(&row, header.depth));
                }
                planes.push(plane);
            }
        }
        _ => return Err(Unreadable::Unsupported("a ZIP-compressed composite")),
    }
    Ok(planes)
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

    #[test]
    fn what_it_cannot_read_it_says_so() {
        assert_eq!(read(b"GIF89a").err(), Some(Unreadable::NotPhotoshop));
        let lab = Psd {
            mode: 9,
            planes: vec![vec![0; 4]; 3],
            ..Default::default()
        };
        assert!(matches!(
            read(&lab.bytes()),
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
