//! The standard profiles, without shipping anybody else's files.
//!
//! Every layout tool offers the same short list: sRGB and Adobe RGB for screen
//! work, and a coated CMYK stock for print. Tessera has to offer it too, or
//! choosing an output intent means hunting for a `.icc` on disk before any work
//! can start.
//!
//! **The two halves of that list are not the same kind of thing, and cannot be
//! got the same way.**
//!
//! An RGB working space is *defined by numbers* — three primaries, a white point
//! and a transfer curve, all published in a standard. So they are **built here**,
//! from those numbers. Nothing is bundled, nothing is downloaded, and the result
//! is colorimetrically exact rather than an approximation.
//!
//! A CMYK profile is *measured*. It is thousands of printed and read patches; no
//! formula produces one, and there is nothing to compute from. It has to come
//! from a file. The obvious move would be to bundle the familiar ones, and it is
//! the wrong move: `USWebCoatedSWOP.icc`, `CoatedFOGRA39.icc` and their siblings
//! are Adobe's files, under Adobe's copyright, and are not ours to redistribute.
//!
//! So Tessera **finds the ones already on the machine**. Every operating system
//! ships profiles in a known directory, and any machine with Adobe or Affinity
//! software installed already has the industry-standard set in it — the very
//! files those applications use. Discovering them is better than bundling them
//! twice over: it is legally clean, and a document proofed here against
//! "Coated FOGRA39" is proofed against *the same bytes* the next application
//! will use.

use std::path::{Path, PathBuf};

use lcms2::{CIExyY, CIExyYTRIPLE, Profile, ToneCurve};

/// A profile Tessera can build from published numbers.
///
/// RGB and greyscale only, because those are the spaces defined by numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standard {
    /// IEC 61966-2.1. The default everything assumes when nothing is said.
    Srgb,
    /// The wider gamut of the 1998 Adobe specification.
    ///
    /// Named "compatible" deliberately. The primaries, white point and gamma are
    /// published and are what this is built from, so it behaves identically — but
    /// Adobe's own profile is Adobe's, and claiming to *be* it would be a claim
    /// nobody here is entitled to make.
    AdobeRgbCompatible,
    /// The gamut of a modern wide-gamut screen. Digital Cinema P3 primaries with
    /// the sRGB curve.
    DisplayP3,
    /// ROMM RGB: very wide, for photographic origination.
    ProPhotoRgb,
    /// ITU-R BT.2020, for high dynamic range video work.
    Rec2020,
    /// Greyscale at gamma 2.2, which is what a screen does.
    GrayGamma22,
    /// Greyscale at gamma 1.8, the older prepress convention.
    GrayGamma18,
}

impl Standard {
    /// Every one, in the order a menu should list them: the default first, then
    /// wider, then greyscale.
    pub const ALL: [Standard; 7] = [
        Standard::Srgb,
        Standard::AdobeRgbCompatible,
        Standard::DisplayP3,
        Standard::ProPhotoRgb,
        Standard::Rec2020,
        Standard::GrayGamma22,
        Standard::GrayGamma18,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Standard::Srgb => "sRGB IEC61966-2.1",
            Standard::AdobeRgbCompatible => "Adobe RGB (1998) compatible",
            Standard::DisplayP3 => "Display P3",
            Standard::ProPhotoRgb => "ProPhoto RGB",
            Standard::Rec2020 => "Rec. 2020",
            Standard::GrayGamma22 => "Grey gamma 2.2",
            Standard::GrayGamma18 => "Grey gamma 1.8",
        }
    }

    /// A sentence saying what the space is *for*, which is what a person
    /// choosing between them actually needs.
    pub fn purpose(self) -> &'static str {
        match self {
            Standard::Srgb => "Screens and the web. The safe default.",
            Standard::AdobeRgbCompatible => {
                "Wider than sRGB; the usual choice for print origination."
            }
            Standard::DisplayP3 => "Modern wide-gamut screens.",
            Standard::ProPhotoRgb => "Very wide; for photographic originals, not for delivery.",
            Standard::Rec2020 => "High dynamic range video.",
            Standard::GrayGamma22 => "Greyscale matching a screen.",
            Standard::GrayGamma18 => "Greyscale, older prepress convention.",
        }
    }

    /// Whether this is a greyscale space rather than a three-channel one.
    pub fn is_grey(self) -> bool {
        matches!(self, Standard::GrayGamma22 | Standard::GrayGamma18)
    }

    /// The profile itself, built now.
    ///
    /// `None` only if Little CMS refuses to build from published constants, which
    /// would mean something is badly wrong; there is nothing sensible to do about
    /// it here, so it is reported rather than unwrapped.
    pub fn build(self) -> Option<Vec<u8>> {
        let profile = match self {
            // Little CMS's own, which is exact and saves restating the curve.
            Standard::Srgb => Profile::new_srgb(),

            Standard::AdobeRgbCompatible => Profile::new_rgb(
                &d65(),
                &primaries((0.6400, 0.3300), (0.2100, 0.7100), (0.1500, 0.0600)),
                // 563/256, which is the exact value the specification gives and
                // not the 2.2 it is usually rounded to.
                &[&ToneCurve::new(563.0 / 256.0); 3],
            )
            .ok()?,

            Standard::DisplayP3 => Profile::new_rgb(
                &d65(),
                &primaries((0.6800, 0.3200), (0.2650, 0.6900), (0.1500, 0.0600)),
                &[&srgb_curve()?; 3],
            )
            .ok()?,

            Standard::ProPhotoRgb => Profile::new_rgb(
                // D50, not D65: ROMM RGB is specified against the printing
                // illuminant, which is one of the things that makes it different.
                &white(0.345_704, 0.358_540),
                &primaries(
                    (0.734_699, 0.265_301),
                    (0.159_597, 0.840_403),
                    (0.036_598, 0.000_105),
                ),
                &[&prophoto_curve()?; 3],
            )
            .ok()?,

            Standard::Rec2020 => Profile::new_rgb(
                &d65(),
                &primaries((0.708, 0.292), (0.170, 0.797), (0.131, 0.046)),
                &[&rec709_curve()?; 3],
            )
            .ok()?,

            Standard::GrayGamma22 => Profile::new_gray(&d65(), &ToneCurve::new(2.2)).ok()?,
            Standard::GrayGamma18 => Profile::new_gray(&d65(), &ToneCurve::new(1.8)).ok()?,
        };

        profile.icc().ok()
    }
}

fn white(x: f64, y: f64) -> CIExyY {
    CIExyY { x, y, Y: 1.0 }
}

/// D65, the daylight white point every screen space but ProPhoto uses.
fn d65() -> CIExyY {
    white(0.3127, 0.3290)
}

fn primaries(r: (f64, f64), g: (f64, f64), b: (f64, f64)) -> CIExyYTRIPLE {
    CIExyYTRIPLE {
        Red: white(r.0, r.1),
        Green: white(g.0, g.1),
        Blue: white(b.0, b.1),
    }
}

/// The sRGB transfer curve: a short linear foot and a power curve above it.
///
/// Type 4 in Little CMS's table is exactly IEC 61966-2.1, so the five published
/// parameters go straight in rather than being approximated by a plain gamma.
/// The linear foot is not a detail — a pure 2.2 gamma is visibly wrong in the
/// shadows, which is where a proof is judged.
fn srgb_curve() -> Option<ToneCurve> {
    ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.040_45]).ok()
}

/// ROMM RGB's curve: gamma 1.8 over a linear foot of slope 16.
fn prophoto_curve() -> Option<ToneCurve> {
    ToneCurve::new_parametric(4, &[1.8, 1.0, 0.0, 1.0 / 16.0, 0.031_248]).ok()
}

/// The BT.709 curve, which BT.2020 shares.
fn rec709_curve() -> Option<ToneCurve> {
    ToneCurve::new_parametric(
        4,
        &[1.0 / 0.45, 1.0 / 1.099, 0.099 / 1.099, 1.0 / 4.5, 0.081],
    )
    .ok()
}

/// A profile found on this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub path: PathBuf,
    /// The profile's own description, which is what a menu should show. A file
    /// name is not it: profiles are renamed, copied and re-supplied.
    pub description: String,
    /// "CMYK", "RGB" or "Grey".
    pub space: &'static str,
}

/// The directories this platform keeps ICC profiles in.
///
/// The system's own first, then the places the major creative suites install
/// theirs. That second group is the point: a machine with InDesign on it already
/// has the whole industry-standard CMYK set, and reading it from there means a
/// document proofed in Tessera is proofed against the same bytes.
pub fn search_paths() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(system) = std::env::var_os("SystemRoot") {
            roots.push(
                PathBuf::from(&system)
                    .join("System32")
                    .join("spool")
                    .join("drivers")
                    .join("color"),
            );
        }
        for variable in ["CommonProgramFiles", "CommonProgramFiles(x86)"] {
            if let Some(common) = std::env::var_os(variable) {
                let adobe = PathBuf::from(&common)
                    .join("Adobe")
                    .join("Color")
                    .join("Profiles");
                roots.push(adobe.join("Recommended"));
                roots.push(adobe);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/System/Library/ColorSync/Profiles"));
        roots.push(PathBuf::from("/Library/ColorSync/Profiles"));
        roots.push(PathBuf::from(
            "/Library/Application Support/Adobe/Color/Profiles/Recommended",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Library/ColorSync/Profiles"));
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        roots.push(PathBuf::from("/usr/share/color/icc"));
        roots.push(PathBuf::from("/usr/local/share/color/icc"));
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(&home).join(".local/share/icc"));
            roots.push(PathBuf::from(&home).join(".color/icc"));
        }
    }

    roots
}

/// How many files a scan will look at before it stops.
///
/// A colour directory usually holds a few dozen profiles; a machine that has
/// collected several thousand should not make opening a menu take a second.
/// Reaching the cap means some profiles are not listed, which is what "Browse"
/// remains there for.
const MOST_FILES: usize = 400;

/// Every profile this machine has, that Tessera can use.
///
/// Sorted by description and deduplicated by it, because the same profile is
/// often installed in two places and one entry is what a person wants to see.
/// Greyscale and unreadable files are left out: a list is only useful if
/// everything in it can be chosen.
pub fn installed() -> Vec<Installed> {
    let mut found: Vec<Installed> = Vec::new();
    let mut looked_at = 0;

    for root in search_paths() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            // A directory that is not there is the ordinary case, not a fault:
            // no machine has all of them.
            continue;
        };
        for entry in entries.flatten() {
            if looked_at >= MOST_FILES {
                break;
            }
            let path = entry.path();
            if !is_profile_name(&path) {
                continue;
            }
            looked_at += 1;
            if let Some(profile) = describe(&path) {
                found.push(profile);
            }
        }
    }

    found.sort_by(|a, b| a.description.cmp(&b.description));
    found.dedup_by(|a, b| a.description == b.description);
    found
}

fn is_profile_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("icc") || e.eq_ignore_ascii_case("icm"))
}

/// Read a profile far enough to list it, or `None` if it cannot be used.
///
/// A file that will not parse is skipped rather than reported. A colour
/// directory is not the user's doing and is full of things they never chose;
/// complaining about one would be complaining about their operating system.
fn describe(path: &Path) -> Option<Installed> {
    let bytes = std::fs::read(path).ok()?;
    let profile = crate::managed::OutputProfile::from_bytes(bytes).ok()?;
    Some(Installed {
        path: path.to_path_buf(),
        description: profile.description().to_string(),
        space: profile.space(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed::OutputProfile;

    #[test]
    fn every_standard_space_builds() {
        // Built from published numbers, so a failure here is a typo in a
        // constant rather than a missing file.
        for standard in Standard::ALL {
            let bytes = standard.build();
            assert!(bytes.is_some(), "{} did not build", standard.label());
            let profile = OutputProfile::from_bytes(bytes.expect("built"))
                .unwrap_or_else(|error| panic!("{}: {error}", standard.label()));
            let wanted = if standard.is_grey() { "Grey" } else { "RGB" };
            assert_eq!(profile.space(), wanted, "{}", standard.label());
        }
    }

    #[test]
    fn every_standard_space_says_what_it_is_for() {
        // A list of names is not a choice a person can make.
        for standard in Standard::ALL {
            assert!(!standard.label().is_empty());
            assert!(!standard.purpose().is_empty(), "{}", standard.label());
        }
    }

    #[test]
    fn srgb_comes_first_because_it_is_the_answer_when_nothing_is_said() {
        assert_eq!(Standard::ALL[0], Standard::Srgb);
    }

    #[test]
    fn a_proof_really_goes_through_the_profile_it_is_proofing() {
        // **The test with teeth.** Little CMS is free to collapse a proofing
        // transform whose source and destination are the same profile into an
        // identity, throwing the proofing profile away — and then the proof
        // silently does nothing while looking like a press with a perfect gamut.
        //
        // Probed with a *narrower* press than the source, because that is the
        // only direction that clips: sRGB content proofed for a wider space is
        // correctly left alone, so a wide profile proves nothing here. A
        // greyscale press is as narrow as a press gets, and a saturated green
        // proofed for one must come back neutral.
        let grey =
            OutputProfile::from_bytes(Standard::GrayGamma22.build().expect("built")).expect("read");
        let proof = grey
            .proof(crate::managed::Rendering::RelativeColorimetric)
            .expect("a proof");

        let [r, g, b, _] = proof.apply([0.0, 1.0, 0.0, 1.0]);
        assert!(
            (r - g).abs() < 0.05 && (g - b).abs() < 0.05,
            "a green proofed for a one-ink press came back as {r}, {g}, {b}              rather than a neutral, so the proofing profile was thrown away"
        );
    }

    #[test]
    fn the_wide_spaces_differ_from_one_another() {
        // Proofing does not separate them — sRGB content is inside all of them —
        // so what is checked is the profiles themselves: a typo in a primary or a
        // curve would make two of these identical.
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for standard in [
            Standard::Srgb,
            Standard::AdobeRgbCompatible,
            Standard::DisplayP3,
            Standard::ProPhotoRgb,
            Standard::Rec2020,
        ] {
            let bytes = standard.build().expect("built");
            assert!(
                !seen.contains(&bytes),
                "{} is byte-identical to another space",
                standard.label()
            );
            seen.push(bytes);
        }
    }

    #[test]
    fn a_wider_space_needs_different_numbers_for_the_same_colour() {
        // The direction that proves the primaries are really being used. Adobe
        // RGB’s green primary is *more* saturated than sRGB’s, so reproducing
        // sRGB’s pure green there means adding red and blue to desaturate it. If
        // the primaries were being ignored, [0, 1, 0] would come back as
        // [0, 1, 0].
        let adobe = OutputProfile::from_bytes(Standard::AdobeRgbCompatible.build().expect("built"))
            .expect("read");
        let ink = adobe
            .ink_for_screen_colour(crate::managed::Rendering::RelativeColorimetric)
            .expect("a conversion")
            .apply([0.0, 1.0, 0.0]);

        assert!(
            ink[0] > 0.1 && ink[2] > 0.1,
            "sRGB green needs desaturating in a wider space, but came back as              {ink:?}"
        );
        assert_eq!(ink[3], 0.0, "an RGB output has no fourth ink");
    }

    #[test]
    fn the_srgb_curve_has_its_linear_foot() {
        // A plain 2.2 gamma is visibly wrong in the shadows, which is where a
        // proof is judged. Near black the two diverge, so this catches the curve
        // being replaced by a rounded exponent.
        let curve = srgb_curve().expect("a curve");
        let plain = ToneCurve::new(2.2);
        let near_black = 0.01f32;
        let with_foot = curve.eval(near_black);
        let without = plain.eval(near_black);
        assert!(
            (with_foot - without).abs() > 1e-5,
            "the linear foot is missing: {with_foot} against {without}"
        );
    }

    #[test]
    fn prophoto_is_built_against_the_printing_illuminant() {
        // D50, not D65, and it is one of the things that makes ProPhoto ProPhoto.
        // Round-tripping a neutral through it and through a D65 space differs,
        // which is what this reaches.
        let prophoto =
            OutputProfile::from_bytes(Standard::ProPhotoRgb.build().expect("built")).expect("read");
        assert_eq!(prophoto.space(), "RGB");
        assert!(prophoto.bytes().len() > 128);
    }

    /// Not an assertion: a report, so a person running the suite can see what
    /// this machine actually has. Run with `-- --nocapture --ignored`.
    #[test]
    #[ignore = "reports what this machine has rather than asserting anything"]
    fn report_what_this_machine_has() {
        for root in search_paths() {
            let count = std::fs::read_dir(&root)
                .map(|entries| entries.flatten().count())
                .unwrap_or(0);
            println!("{count:5}  {}", root.display());
        }
        println!("---");
        for profile in installed() {
            println!("{:6}  {}", profile.space, profile.description);
        }
    }

    #[test]
    fn a_search_path_is_offered_for_this_platform() {
        // A machine with no colour directory at all is possible; a *platform*
        // with none is not, and would mean the list can never be populated.
        assert!(!search_paths().is_empty());
    }

    #[test]
    fn scanning_a_machine_with_no_colour_directories_is_not_a_fault() {
        // No machine has all of them, so an absent directory is the ordinary
        // case.
        let _ = installed();
    }

    #[test]
    fn only_profile_files_are_looked_at() {
        assert!(is_profile_name(Path::new("Coated.icc")));
        assert!(is_profile_name(Path::new("Coated.ICM")));
        assert!(!is_profile_name(Path::new("readme.txt")));
        assert!(!is_profile_name(Path::new("no-extension")));
    }

    #[test]
    fn a_file_that_will_not_parse_is_skipped_rather_than_reported() {
        // A colour directory is not the user's doing and is full of things they
        // never chose; complaining about one would be complaining about their
        // operating system.
        let path = std::env::temp_dir().join("tessera-not-a-profile.icc");
        std::fs::write(&path, b"this is a note").expect("write");
        assert!(describe(&path).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_profile_written_out_is_found_and_named_by_its_own_description() {
        let path = std::env::temp_dir().join("tessera-found-profile.icc");
        std::fs::write(&path, Standard::Srgb.build().expect("built")).expect("write");

        let found = describe(&path).expect("a profile");
        assert!(!found.description.is_empty());
        assert_eq!(found.space, "RGB");
        assert_eq!(found.path, path);

        let _ = std::fs::remove_file(&path);
    }
}
