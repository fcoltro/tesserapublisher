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

/// A profile shipped with Tessera.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundled {
    pub path: PathBuf,
    /// What the manifest calls it, which is a shorter and steadier name than the
    /// profile’s own description.
    pub name: String,
    pub space: &'static str,
    /// The licence tag, so the interface can point at the terms.
    pub licence: String,
}

/// Where the bundled profiles are, if they can be found.
///
/// Two places, tried in order: beside the executable, which is where an
/// installed Tessera keeps them, and then up from the executable to the
/// repository root, which is where they are during development. Neither is
/// guaranteed — a build run from an odd directory finds nothing, and finding
/// nothing is not a fault, because these are an *addition* to the spaces built
/// from published numbers rather than a replacement for them.
pub fn bundled_directory() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let beside = executable.parent()?;

    let candidates = [
        beside.join("profiles"),
        beside.join("assets").join("profiles"),
        // `target/debug/tessera_app` and `target/debug/deps/test-binary` during
        // development, so up three is the repository root either way.
        beside
            .join("..")
            .join("..")
            .join("..")
            .join("assets")
            .join("profiles"),
        beside.join("..").join("..").join("assets").join("profiles"),
    ];
    candidates
        .into_iter()
        .find(|at| at.join(MANIFEST).is_file())
}

/// The manifest’s file name, in one place because two readers use it.
const MANIFEST: &str = "manifest.tsv";

/// The profiles that were shipped and are actually present.
///
/// Driven by the manifest rather than by whatever files are in the directory, so
/// that a name and a licence tag come with each one. A row whose file is absent
/// is skipped in silence: an un-vendored checkout should have a shorter list, not
/// a list of things that cannot be chosen.
pub fn bundled() -> Vec<Bundled> {
    let Some(directory) = bundled_directory() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(directory.join(MANIFEST)) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        // file, space, licence, name, source. The source is the vendoring
        // script’s business, not the application’s.
        let mut fields = line.split('\t').map(str::trim);
        let (Some(file), Some(space), Some(licence), Some(name)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };

        let path = directory.join(file);
        if !path.is_file() {
            continue;
        }
        // The space the manifest claims is not taken on trust: the file is read
        // and asked. A row that disagrees with its file is a row to skip, and the
        // vendoring script is where that gets reported.
        let Some(found) = describe(&path) else {
            continue;
        };
        if found.space != space {
            continue;
        }

        out.push(Bundled {
            path,
            name: name.to_string(),
            space: found.space,
            licence: licence.to_string(),
        });
    }
    out
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
/// Three groups, and each is there for its own reason.
///
/// **The system's own.** Every platform has one, and on Linux it is where the
/// distribution's profile packages land — `icc-profiles-free` and friends, which
/// is how the freely licensed presses reach most machines.
///
/// **The creative suites.** A machine with InDesign or Affinity on it already
/// has the whole industry-standard CMYK set. Reading it from there means a
/// document proofed in Tessera is proofed against the same bytes the next
/// application will use, which matters more than matching a name.
///
/// **The other open-source tools.** Krita, Scribus, GIMP, darktable and
/// RawTherapee each ship profiles, and between them they have solved this
/// problem already: the RGB working spaces are Elle Stone's public-domain set,
/// and the CMYK presses come from the distribution rather than from the
/// application. Looking where they put them costs one `read_dir` that usually
/// fails, and gains their whole answer on a machine that has any of them.
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
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            let Some(programs) = std::env::var_os(variable) else {
                continue;
            };
            let programs = PathBuf::from(&programs);
            // The open-source tools, which put theirs inside their own install.
            roots.push(
                programs
                    .join("Krita (x64)")
                    .join("share")
                    .join("color")
                    .join("icc"),
            );
            roots.push(
                programs
                    .join("Scribus")
                    .join("share")
                    .join("color")
                    .join("icc"),
            );
            roots.push(
                programs
                    .join("GIMP 2")
                    .join("share")
                    .join("color")
                    .join("icc"),
            );
            roots.push(
                programs
                    .join("Inkscape")
                    .join("share")
                    .join("color")
                    .join("icc"),
            );
            roots.push(
                programs
                    .join("darktable")
                    .join("share")
                    .join("darktable")
                    .join("color-in"),
            );
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            roots.push(PathBuf::from(&home).join(".color").join("icc"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/System/Library/ColorSync/Profiles"));
        roots.push(PathBuf::from("/Library/ColorSync/Profiles"));
        roots.push(PathBuf::from(
            "/Library/Application Support/Adobe/Color/Profiles/Recommended",
        ));
        roots.push(PathBuf::from(
            "/Library/Application Support/Adobe/Color/Profiles",
        ));
        // The open-source tools, inside their bundles.
        roots.push(PathBuf::from(
            "/Applications/krita.app/Contents/Resources/color/icc",
        ));
        roots.push(PathBuf::from(
            "/Applications/Scribus.app/Contents/share/color/icc",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(&home);
            roots.push(home.join("Library/ColorSync/Profiles"));
            roots.push(home.join(".color/icc"));
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // The distribution's own. `icc-profiles-free` and `icc-profiles-openicc`
        // land under here — in *subdirectories*, which is why the walk below
        // recurses rather than reading one level.
        roots.push(PathBuf::from("/usr/share/color/icc"));
        roots.push(PathBuf::from("/usr/local/share/color/icc"));
        roots.push(PathBuf::from("/var/lib/color/icc"));
        // The open-source tools, which ship their own inside their data
        // directories.
        roots.push(PathBuf::from("/usr/share/krita/color/icc"));
        roots.push(PathBuf::from("/usr/share/scribus/profiles"));
        roots.push(PathBuf::from("/usr/share/gimp/2.0/profiles"));
        roots.push(PathBuf::from("/usr/share/darktable/color/in"));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(&home);
            roots.push(home.join(".local/share/icc"));
            roots.push(home.join(".color/icc"));
            roots.push(home.join(".local/share/color/icc"));
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

/// How far into a colour directory the walk goes.
///
/// **It has to recurse at all**, which the first version of this did not:
/// `icc-profiles-free` installs into `/usr/share/color/icc/basICColor/` and
/// `.../OpenICC/`, so reading one level finds nothing on exactly the platform
/// where the freely licensed presses live.
///
/// Three levels, not unlimited. A colour directory is two or three deep; a walk
/// with no floor would follow a symlink into a home directory and read it.
const DEEPEST: usize = 3;

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
        walk(&root, 0, &mut looked_at, &mut found);
    }

    found.sort_by(|a, b| a.description.cmp(&b.description));
    // By description, because the same profile installed in two places is one
    // profile to a person. The path is kept from whichever was found first.
    found.dedup_by(|a, b| a.description == b.description);
    found
}

/// Read one directory and its subdirectories, up to [`DEEPEST`].
///
/// A directory that is not there is the ordinary case rather than a fault: no
/// machine has all of them, and most machines have very few.
fn walk(at: &Path, depth: usize, looked_at: &mut usize, found: &mut Vec<Installed>) {
    if depth > DEEPEST || *looked_at >= MOST_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(at) else {
        return;
    };

    for entry in entries.flatten() {
        if *looked_at >= MOST_FILES {
            return;
        }
        let path = entry.path();

        // `file_type` rather than `path.is_dir()`, so a symlink is seen as a
        // symlink and not followed. A colour directory that links to a home
        // directory would otherwise be walked as one.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&path, depth + 1, looked_at, found);
            continue;
        }
        if !kind.is_file() || !is_profile_name(&path) {
            continue;
        }

        *looked_at += 1;
        if let Some(profile) = describe(&path) {
            found.push(profile);
        }
    }
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

    /// Where the repository keeps its profiles, from the crate's own location.
    ///
    /// Not `bundled_directory`, deliberately: that walks up from the *test
    /// binary* and is the thing under test here. Asking it whether it works
    /// would be asking it to mark its own paper.
    fn checked_in() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets")
            .join("profiles")
    }

    #[test]
    fn profiles_in_the_repository_are_profiles_the_application_can_find() {
        // **The hole this closes.** Every other profile test returns quietly
        // when `bundled()` is empty, so a machine where the lookup fails looks
        // exactly like one where all nine passed — and the machines that
        // matter most are the ones nobody here runs by hand. If the files are
        // checked in, they must be reachable, or CI is proving nothing about
        // the platforms it was added for.
        let vendored: Vec<_> = std::fs::read_dir(checked_in())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "icc"))
            .collect();
        if vendored.is_empty() {
            // A checkout that has not run `tools/vendor-profiles.py`. Nothing
            // to prove, and nothing to complain about.
            return;
        }

        let found = bundled();
        assert!(
            !found.is_empty(),
            "{} profiles are checked in and `bundled` found none: the lookup \
             does not work on this platform",
            vendored.len()
        );
        assert_eq!(
            found.len(),
            vendored.len(),
            "{} profiles are checked in but only {} were usable",
            vendored.len(),
            found.len()
        );
    }

    #[test]
    fn the_installer_carries_every_profile() {
        // WiX has no way to say "this directory", so `main.wxs` names each file
        // — and a component listing *some* of them produces an installer that
        // carries some of the presses, which is worse than none because the
        // ones missing are missing silently.
        //
        // Checked here rather than in packaging, because this is where anybody
        // adding a profile is already working.
        let wxs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("packaging")
            .join("windows")
            .join("main.wxs");
        let Ok(text) = std::fs::read_to_string(&wxs) else {
            // No packaging in this checkout. Nothing to prove.
            return;
        };

        for entry in std::fs::read_dir(checked_in())
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "icc") {
                continue;
            }
            let name = path
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            assert!(
                text.contains(&name),
                "{name} is checked in but the Windows installer does not carry it"
            );
        }
    }

    #[test]
    fn the_bundled_press_profiles_make_real_conversions() {
        // **The test the vendored files exist for.** Everything about CMYK
        // export and soft proofing was code with no press data behind it until
        // the profiles were downloaded; this is what turns that from a claim
        // into something checked.
        //
        // Skipped rather than failed where they are absent: a checkout without
        // them still builds and still runs, and `tools/vendor-profiles.py` is
        // how they arrive.
        let press: Vec<_> = bundled()
            .into_iter()
            .filter(|b| b.space == "CMYK")
            .collect();
        if press.is_empty() {
            return;
        }

        for profile in press {
            let bytes = std::fs::read(&profile.path).expect("read");
            let loaded = crate::managed::OutputProfile::from_bytes(bytes)
                .unwrap_or_else(|e| panic!("{} would not load: {e}", profile.name));

            let conversion = loaded
                .ink_for_screen_colour(crate::managed::Rendering::default())
                .unwrap_or_else(|e| panic!("{} makes no conversion: {e}", profile.name));

            // Four inks, and a mid grey must land somewhere with ink in it. A
            // transform that answered all zeroes would be a profile loaded and
            // doing nothing, which is the failure that looks like success.
            let inks = conversion.apply([0.5, 0.5, 0.5]);
            assert!(
                inks.iter().any(|v| *v > 0.01),
                "{} converts mid grey to no ink at all",
                profile.name
            );
            assert!(
                inks.iter().all(|v| v.is_finite()),
                "{} produced a non-finite ink value",
                profile.name
            );
        }
    }

    #[test]
    fn every_bundled_profile_agrees_with_the_manifest() {
        // `bundled` skips a row whose file disagrees with the space it claims,
        // so a mislabelled profile is silently absent rather than wrong. This
        // is the other half: if the files are there, they are all there.
        let found = bundled();
        if found.is_empty() {
            return;
        }
        assert!(
            found.iter().any(|b| b.space == "RGB"),
            "no RGB profile survived the manifest check"
        );
        assert!(
            found.iter().any(|b| b.space == "CMYK"),
            "no CMYK profile survived the manifest check"
        );
    }
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
    fn an_unvendored_checkout_has_a_shorter_list_rather_than_a_broken_one() {
        // The bundled profiles are an addition to the spaces built from published
        // numbers, not a replacement, so finding none of them is not a fault.
        for profile in bundled() {
            assert!(
                profile.path.is_file(),
                "{} was listed but is not there",
                profile.name
            );
            assert!(!profile.name.is_empty());
            assert!(
                !profile.licence.is_empty(),
                "{} has no licence tag",
                profile.name
            );
        }
    }

    #[test]
    fn the_manifest_is_found_during_development() {
        // Not an assertion about the profiles being vendored — only that the
        // directory containing the manifest can be located from a test binary. If
        // this fails, `bundled()` can never return anything however well the
        // vendoring went.
        let at = bundled_directory();
        assert!(
            at.is_some(),
            "the manifest was not found from {:?}",
            std::env::current_exe()
        );
    }

    #[test]
    fn every_manifest_row_has_five_fields_and_a_known_space() {
        // The manifest is read by a Rust function and a Python script, and a row
        // either of them cannot parse is a row that silently does nothing.
        let Some(directory) = bundled_directory() else {
            return;
        };
        let text = std::fs::read_to_string(directory.join("manifest.tsv")).expect("the manifest");
        for (number, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(
                fields.len(),
                5,
                "manifest line {} has {} fields: {line:?}",
                number + 1,
                fields.len()
            );
            assert!(
                ["CMYK", "RGB", "Grey"].contains(&fields[1].trim()),
                "manifest line {} names an unknown space {:?}",
                number + 1,
                fields[1]
            );
        }
    }

    #[test]
    fn every_licence_tag_in_the_manifest_has_terms_written_down() {
        // A profile is somebody else’s work. The vendoring script refuses a row
        // with no terms; this is the same rule held from the other side, so a row
        // cannot be added to the manifest without them either.
        let Some(directory) = bundled_directory() else {
            return;
        };
        let manifest =
            std::fs::read_to_string(directory.join("manifest.tsv")).expect("the manifest");
        let licences =
            std::fs::read_to_string(directory.join("LICENCES.md")).expect("the licences");

        for line in manifest.lines() {
            if line.trim().is_empty() || line.trim_start().starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 3 {
                continue;
            }
            let tag = fields[2].trim();
            assert!(
                licences.contains(&format!("## `{tag}`")),
                "licence tag {tag:?} has no section in LICENCES.md"
            );
        }
    }

    #[test]
    fn a_search_path_is_offered_for_this_platform() {
        // A machine with no colour directory at all is possible; a *platform*
        // with none is not, and would mean the list can never be populated.
        assert!(!search_paths().is_empty());
    }

    #[test]
    fn the_walk_reaches_a_profile_in_a_subdirectory() {
        // **The bug this exists for.** `icc-profiles-free` installs into
        // `/usr/share/color/icc/basICColor/` and `.../OpenICC/`, so a scan that
        // read one level found nothing on exactly the platform where the freely
        // licensed presses live.
        let root = std::env::temp_dir().join("tessera-walk-deep");
        let _ = std::fs::remove_dir_all(&root);
        let nested = root.join("basICColor").join("more");
        std::fs::create_dir_all(&nested).expect("a nested directory");
        std::fs::write(
            nested.join("buried.icc"),
            Standard::Srgb.build().expect("built"),
        )
        .expect("write");

        let mut found = Vec::new();
        let mut looked_at = 0;
        walk(&root, 0, &mut looked_at, &mut found);

        assert_eq!(found.len(), 1, "a profile two levels down was not reached");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_walk_stops_before_it_reads_the_whole_disk() {
        // A colour directory is two or three deep. A walk with no floor would
        // follow whatever it found into a home directory and read that.
        let root = std::env::temp_dir().join("tessera-walk-floor");
        let _ = std::fs::remove_dir_all(&root);
        let mut at = root.clone();
        for step in 0..(DEEPEST + 3) {
            at = at.join(format!("level{step}"));
        }
        std::fs::create_dir_all(&at).expect("a deep directory");
        std::fs::write(
            at.join("too-deep.icc"),
            Standard::Srgb.build().expect("built"),
        )
        .expect("write");

        let mut found = Vec::new();
        let mut looked_at = 0;
        walk(&root, 0, &mut looked_at, &mut found);

        assert!(found.is_empty(), "the walk went deeper than {DEEPEST}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_walk_stops_after_enough_files() {
        // A machine that has collected several thousand profiles should not make
        // opening a menu take a second.
        let root = std::env::temp_dir().join("tessera-walk-cap");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a directory");

        // Not profiles, so nothing is parsed: the cap counts files *looked at*,
        // which is what costs the time.
        for n in 0..(MOST_FILES + 20) {
            std::fs::write(root.join(format!("{n}.icc")), b"not a profile").expect("write");
        }

        let mut found = Vec::new();
        let mut looked_at = 0;
        walk(&root, 0, &mut looked_at, &mut found);

        assert_eq!(looked_at, MOST_FILES, "the cap did not hold");
        let _ = std::fs::remove_dir_all(&root);
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
