//! Adobe Swatch Exchange: the `.ase` file InDesign, Illustrator and
//! Photoshop pass named colours between.
//!
//! The format is a header and a run of blocks, big-endian throughout:
//!
//! - `ASEF`, a version as two 16-bit numbers (1.0), and a 32-bit count of
//!   the blocks that follow;
//! - each block a 16-bit type, a 32-bit length and that many bytes: `0x0001`
//!   a colour, `0xC001` and `0xC002` the start and end of a group.
//!
//! A colour block is its name — a 16-bit count of UTF-16 units, the
//! terminating nul included, then the units — four bytes naming its model,
//! its numbers as 32-bit floats, and a 16-bit kind: 0 global, 1 spot,
//! 2 normal. The models are `CMYK` and `RGB ` (each number 0 to 1), `LAB `
//! (lightness 0 to 1, the two axes as they are) and `Gray` (one number, a
//! level, 1 being white).
//!
//! A group's start block carries its name, and the colours up to its end are
//! filed in it, as InDesign's colour groups are; a swatch's group is written
//! back the same way. What is written is every swatch that has a colour of
//! its own to give, which
//! [`crate::document::Document::swatches_for_exchange`] makes.

use tessera_color::Color;

use crate::nodes::Swatch;

const SIGNATURE: &[u8; 4] = b"ASEF";
const COLOUR: u16 = 0x0001;
const GROUP_START: u16 = 0xC001;
const GROUP_END: u16 = 0xC002;
const GLOBAL: u16 = 0;
const SPOT: u16 = 1;

/// Why a file could not be read as swatches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeError {
    /// It does not begin as a swatch exchange file does.
    NotSwatches,
    /// It ends in the middle of something.
    Truncated,
}

impl std::fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotSwatches => "This is not an Adobe Swatch Exchange file.",
            Self::Truncated => "The file ends part of the way through a colour.",
        })
    }
}

impl std::error::Error for ExchangeError {}

/// The swatches in an `.ase` file, in the order it lists them.
///
/// A colour in a model this does not know is passed over rather than
/// failing the file: one strange entry should not cost the other forty.
pub fn read(bytes: &[u8]) -> Result<Vec<Swatch>, ExchangeError> {
    let mut at = Reader { bytes, at: 0 };
    if at.take(4)? != SIGNATURE {
        return Err(ExchangeError::NotSwatches);
    }
    // The version and the count. The count is not trusted: some writers
    // count a group's start and end and some do not, so the blocks are
    // read until the bytes run out.
    at.take(8)?;
    let mut swatches = Vec::new();
    let mut group: Option<String> = None;
    while at.remaining() > 0 {
        let kind = at.u16()?;
        let length = at.u32()? as usize;
        let block = at.take(length)?;
        match kind {
            COLOUR => {
                if let Some(mut swatch) = colour_block(block)? {
                    swatch.group = group.clone();
                    swatches.push(swatch);
                }
            }
            // A group's start carries its name and nothing else; the colours
            // up to its end are filed in it.
            GROUP_START => {
                group = name_of(&mut Reader {
                    bytes: block,
                    at: 0,
                })
                .ok()
                .filter(|n| !n.is_empty());
            }
            GROUP_END => group = None,
            // Anything newer carries no colour this knows.
            _ => {}
        }
    }
    Ok(swatches)
}

/// One colour block's swatch, or none when its model is not one of the four.
fn colour_block(block: &[u8]) -> Result<Option<Swatch>, ExchangeError> {
    let mut at = Reader {
        bytes: block,
        at: 0,
    };
    let name = name_of(&mut at)?;
    let model = at.take(4)?;
    let mut numbers =
        |n: usize| -> Result<Vec<f32>, ExchangeError> { (0..n).map(|_| at.f32()).collect() };
    let colour = match model {
        b"CMYK" => {
            let v = numbers(4)?;
            Color::Cmyk {
                c: v[0],
                m: v[1],
                y: v[2],
                k: v[3],
                a: 1.0,
            }
        }
        b"RGB " => {
            let v = numbers(3)?;
            Color::Rgb {
                r: v[0],
                g: v[1],
                b: v[2],
                a: 1.0,
            }
        }
        b"LAB " => {
            let v = numbers(3)?;
            Color::Lab {
                l: v[0] * 100.0,
                a: v[1],
                b: v[2],
                alpha: 1.0,
            }
        }
        // A level of grey is that much short of black: printed, the black
        // ink alone.
        b"Gray" => {
            let v = numbers(1)?;
            Color::Cmyk {
                c: 0.0,
                m: 0.0,
                y: 0.0,
                k: (1.0 - v[0]).clamp(0.0, 1.0),
                a: 1.0,
            }
        }
        _ => return Ok(None),
    };
    // A block that stops before its kind is read as a process colour: the
    // colour itself arrived whole.
    let spot = at.u16().is_ok_and(|kind| kind == SPOT);
    Ok(Some(Swatch {
        name: if name.is_empty() {
            super::swatch_edit::colour_name(&colour)
        } else {
            name
        },
        colour,
        spot,
        group: None,
    }))
}

/// A block's name: a count of UTF-16 units, its nul included, and the units.
fn name_of(at: &mut Reader<'_>) -> Result<String, ExchangeError> {
    let units = at.u16()? as usize;
    let mut name = Vec::with_capacity(units);
    for _ in 0..units {
        name.push(at.u16()?);
    }
    Ok(String::from_utf16_lossy(&name)
        .trim_end_matches('\0')
        .trim()
        .to_owned())
}

fn name_bytes(name: &str) -> Vec<u8> {
    let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&(units.len() as u16).to_be_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

/// `swatches` as an `.ase` file. A swatch whose colour is not a mix of
/// its own — a tint still naming its base, a spot not yet worked out — is
/// left out; hand this what
/// [`crate::document::Document::swatches_for_exchange`] gives.
pub fn write(swatches: &[Swatch]) -> Vec<u8> {
    // Each colour, with a group's start before its first swatch and its end
    // after its last.
    let mut blocks: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut open: Option<&str> = None;
    for swatch in swatches {
        let Some(colour) = colour_bytes(swatch) else {
            continue;
        };
        let group = swatch.group.as_deref();
        if group != open {
            if open.is_some() {
                blocks.push((GROUP_END, Vec::new()));
            }
            if let Some(name) = group {
                blocks.push((GROUP_START, name_bytes(name)));
            }
            open = group;
        }
        blocks.push((COLOUR, colour));
    }
    if open.is_some() {
        blocks.push((GROUP_END, Vec::new()));
    }
    let mut out = Vec::new();
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(blocks.len() as u32).to_be_bytes());
    for (kind, block) in blocks {
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&(block.len() as u32).to_be_bytes());
        out.extend_from_slice(&block);
    }
    out
}

fn colour_bytes(swatch: &Swatch) -> Option<Vec<u8>> {
    let (model, numbers): (&[u8; 4], Vec<f32>) = match &swatch.colour {
        Color::Cmyk { c, m, y, k, .. } => (b"CMYK", vec![*c, *m, *y, *k]),
        Color::Rgb { r, g, b, .. } => (b"RGB ", vec![*r, *g, *b]),
        Color::Lab { l, a, b, .. } => (b"LAB ", vec![*l / 100.0, *a, *b]),
        // An exchange file has no word for inks over inks.
        Color::Spot { .. } | Color::Swatch { .. } | Color::Mixed { .. } => return None,
    };
    let mut out = name_bytes(&swatch.name);
    out.extend_from_slice(model);
    for n in numbers {
        out.extend_from_slice(&n.to_be_bytes());
    }
    let kind = if swatch.spot { SPOT } else { GLOBAL };
    out.extend_from_slice(&kind.to_be_bytes());
    Some(out)
}

/// Bytes read from the front, each read refused past the end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], ExchangeError> {
        let end = self.at.checked_add(n).ok_or(ExchangeError::Truncated)?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(ExchangeError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, ExchangeError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, ExchangeError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, ExchangeError> {
        let b = self.take(4)?;
        Ok(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Color {
        Color::Cmyk { c, m, y, k, a: 1.0 }
    }

    /// A colour block as another program writes one, by hand.
    fn block(name: &str, model: &[u8; 4], numbers: &[f32], kind: u16) -> Vec<u8> {
        let mut data = Vec::new();
        let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
        data.extend_from_slice(&(units.len() as u16).to_be_bytes());
        for u in units {
            data.extend_from_slice(&u.to_be_bytes());
        }
        data.extend_from_slice(model);
        for n in numbers {
            data.extend_from_slice(&n.to_be_bytes());
        }
        data.extend_from_slice(&kind.to_be_bytes());
        let mut out = COLOUR.to_be_bytes().to_vec();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(&data);
        out
    }

    fn file(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"ASEF".to_vec();
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(&(blocks.len() as u32).to_be_bytes());
        for b in blocks {
            out.extend_from_slice(b);
        }
        out
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let swatches = vec![
            Swatch::new("Brand red", cmyk(0.0, 0.91, 0.76, 0.0)),
            Swatch {
                name: "PANTONE 286 C".into(),
                colour: cmyk(1.0, 0.75, 0.0, 0.02),
                spot: true,
                group: None,
            },
            Swatch::new(
                "Électric sky ☀",
                Color::Rgb {
                    r: 0.0,
                    g: 0.78,
                    b: 1.0,
                    a: 1.0,
                },
            ),
            Swatch::new(
                "Forest",
                Color::Lab {
                    l: 42.0,
                    a: -38.0,
                    b: 22.0,
                    alpha: 1.0,
                },
            ),
        ];
        let back = read(&write(&swatches)).unwrap();
        assert_eq!(back.len(), 4);
        assert_eq!(back[0], swatches[0]);
        assert_eq!(back[1], swatches[1], "a spot stays a spot");
        assert_eq!(back[2], swatches[2], "a name outside Latin-1 survives");
        let Color::Lab { l, a, b, .. } = back[3].colour else {
            panic!("{:?}", back[3].colour)
        };
        assert!((l - 42.0).abs() < 1e-4 && a == -38.0 && b == 22.0);
    }

    #[test]
    fn a_file_as_another_program_writes_it_is_read() {
        // Written byte by byte, independently of `write`: a group round two
        // colours, a grey, and a model nobody knows.
        let group = |kind: u16, name: &str| {
            let mut data = Vec::new();
            if !name.is_empty() {
                let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
                data.extend_from_slice(&(units.len() as u16).to_be_bytes());
                for u in units {
                    data.extend_from_slice(&u.to_be_bytes());
                }
            }
            let mut out = kind.to_be_bytes().to_vec();
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(&data);
            out
        };
        let bytes = file(&[
            group(GROUP_START, "Spring"),
            block("Leaf", b"RGB ", &[0.2, 0.6, 0.1], 2),
            block("Ink", b"CMYK", &[0.0, 0.0, 0.0, 1.0], 1),
            group(GROUP_END, ""),
            block("Grey 30", b"Gray", &[0.7], 0),
            block("Odd", b"HSB ", &[0.1, 0.2, 0.3], 0),
        ]);
        let swatches = read(&bytes).unwrap();
        let names: Vec<&str> = swatches.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Leaf", "Ink", "Grey 30"]);
        assert!(!swatches[0].spot && swatches[1].spot);
        let groups: Vec<Option<&str>> = swatches.iter().map(|s| s.group.as_deref()).collect();
        assert_eq!(groups, [Some("Spring"), Some("Spring"), None]);
        let Color::Cmyk { k, .. } = swatches[2].colour else {
            panic!()
        };
        assert!((k - 0.3).abs() < 1e-6, "a level of 0.7 is 30% black");
    }

    #[test]
    fn colour_groups_are_written_and_read_back() {
        let mut leaf = Swatch::new(
            "Leaf",
            Color::Rgb {
                r: 0.2,
                g: 0.6,
                b: 0.1,
                a: 1.0,
            },
        );
        leaf.group = Some("Spring".into());
        let mut moss = leaf.clone();
        moss.name = "Moss".into();
        let plain = Swatch::new("Plain", Color::BLACK_INK);
        let back = read(&write(&[leaf, moss, plain])).unwrap();
        let groups: Vec<Option<&str>> = back.iter().map(|s| s.group.as_deref()).collect();
        assert_eq!(groups, [Some("Spring"), Some("Spring"), None]);
    }

    #[test]
    fn a_file_that_is_not_swatches_or_stops_short_is_refused() {
        assert_eq!(read(b"%PDF-1.7"), Err(ExchangeError::NotSwatches));
        assert_eq!(read(b"AS"), Err(ExchangeError::Truncated));
        let mut bytes = file(&[block("Leaf", b"RGB ", &[0.2, 0.6, 0.1], 2)]);
        bytes.truncate(bytes.len() - 6);
        assert_eq!(read(&bytes), Err(ExchangeError::Truncated));
    }

    #[test]
    fn a_colour_with_no_colour_of_its_own_is_not_written() {
        let swatches = vec![
            Swatch::new(
                "Brand red 40%",
                Color::Swatch {
                    name: "Brand red".into(),
                    tint: 0.4,
                },
            ),
            Swatch::new("Black", cmyk(0.0, 0.0, 0.0, 1.0)),
        ];
        let back = read(&write(&swatches)).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "Black");
    }

    #[test]
    fn a_colour_with_no_name_is_named_by_its_numbers() {
        let bytes = file(&[block("", b"CMYK", &[0.0, 0.5, 1.0, 0.0], 0)]);
        assert_eq!(read(&bytes).unwrap()[0].name, "C=0 M=50 Y=100 K=0");
    }
}
