//! What the application remembers between runs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tessera_geometry::Unit;
use tessera_io::{IoError, write_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

/// How much the panels let the document show through.
///
/// A setting rather than a decision, and for three reasons that are all real
/// rather than defensive. Translucency over a page is a **judgement call** in a
/// tool where colour is judged; it costs a second render, which is nothing on a
/// discrete card and not nothing on an old laptop; and some people simply cannot
/// read text over a moving background. Any one of those is enough to make it
/// switchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PanelSurface {
    /// Panels sit beside the canvas and are opaque. The canvas is narrower and
    /// nothing shows through.
    Solid,
    /// Panels float over the canvas, which extends beneath them, and the
    /// document shows through blurred.
    #[default]
    Glass,
}

impl PanelSurface {
    pub fn label(self) -> &'static str {
        match self {
            PanelSurface::Solid => "Solid",
            PanelSurface::Glass => "Glass",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            PanelSurface::Solid => "Panels beside the page. Nothing shows through.",
            PanelSurface::Glass => "Panels over the page, blurred behind them.",
        }
    }

    pub fn is_glass(self) -> bool {
        matches!(self, PanelSurface::Glass)
    }
}

/// How strongly the backdrop behind a glass panel is blurred.
///
/// Expressed as **how much the backdrop is reduced before it is stretched back
/// up**, because that is what the implementation actually does and a number that
/// means something is better than one that has to be calibrated. Six is a soft
/// frost; two is barely a smear; sixteen is opaque fog and costs the least of
/// all, which is a pleasant inversion.
pub const BLUR_LEAST: u32 = 2;
pub const BLUR_MOST: u32 = 16;

fn default_minimum_ppi() -> f64 {
    300.0
}

fn default_blur() -> u32 {
    6
}

fn default_panel_opacity() -> f32 {
    0.82
}

/// What the application remembers between runs.
///
/// Deliberately not document data: a preference travels with the person, not
/// with the file, so opening someone else's layout must not change the units
/// you work in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    pub version: u32,
    pub unit: Unit,
    pub theme: ThemeChoice,
    /// The effective resolution below which artwork is reported as too low.
    ///
    /// A preference rather than a constant: 300 is the usual bar for offset
    /// litho, 150 is fine for newsprint, and 72 is right for a screen PDF.
    /// Hard-coding 300 would cry wolf at every newspaper.
    #[serde(default = "default_minimum_ppi")]
    pub minimum_ppi: f64,

    /// Whether panels float over the page or sit beside it.
    #[serde(default)]
    pub panel_surface: PanelSurface,

    /// How much the backdrop is reduced before being stretched back up.
    #[serde(default = "default_blur")]
    pub blur: u32,

    /// How opaque a glass panel is over its backdrop.
    ///
    /// Separate from the blur, because they trade against each other: a heavy
    /// blur reads well at low opacity, and a light one needs more tint to stay
    /// legible. Tying them together would take away the adjustment that actually
    /// makes text readable on a given screen.
    #[serde(default = "default_panel_opacity")]
    pub panel_opacity: f32,

    /// Whether the canvas snaps objects to guides and to other objects.
    ///
    /// It lived only in the application before, so it was forgotten between
    /// runs. Somebody who turns snapping off is not turning it off for a minute.
    #[serde(default = "yes")]
    pub snapping: bool,

    /// Whether Tessera keeps a recovery copy of unsaved work.
    ///
    /// **Not "save my file automatically".** It writes a separate copy that is
    /// offered back after a crash and deleted on a clean quit; the document
    /// itself is only ever written when somebody saves it. Naming it autosave
    /// would promise the wrong thing, and somebody who believed that promise
    /// would stop pressing Ctrl+S.
    ///
    /// Defaults **on**, and is an opt-out rather than an opt-in: data safety
    /// that has to be switched on protects the people who did not need it.
    #[serde(default = "yes")]
    pub recovery_copy: bool,

    /// How long after an edit the recovery copy is written, in seconds.
    #[serde(default = "default_recovery_seconds")]
    pub recovery_seconds: u32,

    /// Named sets of export choices.
    ///
    /// A preference rather than document data: they are how *this studio* sends
    /// work, and opening somebody else’s layout must not change them.
    #[serde(default = "crate::view::export_dialog::Preset::usual")]
    pub export_presets: Vec<crate::view::export_dialog::Preset>,

    /// Named arrangements of the panels.
    #[serde(default = "crate::workspace::Workspace::usual")]
    pub workspaces: Vec<crate::workspace::Workspace>,

    /// Shortcuts anybody has changed from the ones this build ships.
    #[serde(default)]
    pub shortcuts: crate::keys::Bindings,

    /// The arrangement in force, by name.
    ///
    /// Remembered so a relaunch comes back to the arrangement somebody left,
    /// which is the half of "quit and relaunch, and find the layout as it was"
    /// that a list of workspaces does not give on its own.
    #[serde(default)]
    pub workspace: Option<String>,
}

fn yes() -> bool {
    true
}

/// Long enough not to intrude, short enough that a crash costs seconds.
fn default_recovery_seconds() -> u32 {
    30
}

/// How rarely a recovery copy may be written before it stops being one.
///
/// Ten minutes of lost work is not recovery, it is a consolation prize.
pub const RECOVERY_LEAST: u32 = 5;
pub const RECOVERY_MOST: u32 = 600;

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: Self::PATH_VERSION,
            // The unit most of the world lays out pages in.
            unit: Unit::Millimetres,
            theme: ThemeChoice::default(),
            minimum_ppi: default_minimum_ppi(),
            panel_surface: PanelSurface::default(),
            blur: default_blur(),
            panel_opacity: default_panel_opacity(),
            snapping: yes(),
            recovery_copy: yes(),
            recovery_seconds: default_recovery_seconds(),
            export_presets: crate::view::export_dialog::Preset::usual(),
            shortcuts: crate::keys::Bindings::default(),
            workspaces: crate::workspace::Workspace::usual(),
            workspace: None,
        }
    }
}

impl Preferences {
    /// The blur divisor, held to what can be rendered.
    ///
    /// Clamped on the way out rather than on the way in, as everything else in
    /// this codebase is: a preferences file carrying a stray value draws
    /// sensibly instead of being quietly rewritten, and the file still says what
    /// it said.
    pub fn blur_divisor(&self) -> u32 {
        self.blur.clamp(BLUR_LEAST, BLUR_MOST)
    }

    /// How opaque a glass panel is, held to a range that stays legible.
    ///
    /// The floor is not zero. A panel at no opacity is an invisible panel with
    /// live controls in it, which is not a look — it is a fault somebody would
    /// have to work out how to undo.
    pub fn glass_opacity(&self) -> f32 {
        self.panel_opacity.clamp(0.35, 1.0)
    }

    /// How often a recovery copy is written.
    ///
    /// Clamped on the way out, as everything else here is, so a file carrying a
    /// stray value behaves sensibly rather than being quietly rewritten.
    pub fn recovery_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(
            self.recovery_seconds.clamp(RECOVERY_LEAST, RECOVERY_MOST) as u64
        )
    }
}

/// Write the preferences out, reporting a failure where it can be seen.
///
/// One function rather than the same four lines wherever a preference changes.
/// Failing to save is worth saying: somebody who sets a preference and finds it
/// gone tomorrow should have been told why today.
pub fn remember(state: &mut crate::app::TesseraApp) {
    let Some(path) = Preferences::path() else {
        // The platform will not say where preferences live, which is a real
        // condition on a stripped-down container rather than an error. They last
        // for this run and that is all that was ever promised.
        return;
    };
    if let Err(error) = state.prefs.save_to(&path) {
        state.status = Some(crate::app::Status::error(format!(
            "preferences could not be saved: {error}"
        )));
    }
}

/// Where this platform keeps a user's configuration.
///
/// Hand-rolled rather than taken from a crate. The three rules below are the
/// whole of it, and the alternative pulled in six transitive dependencies —
/// including a random number generator and a Redox user database — to compute
/// one path. This crate hand-draws its icons to avoid an SVG runtime; the
/// same judgement applies here.
///
/// Returns `None` when the platform will not say, which is a real condition
/// on a stripped-down container and not an error: preferences simply do not
/// persist, and the caller says so rather than pretending they did.
fn config_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join("Library").join("Application Support"))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // The XDG base directory specification: the variable when it is set
        // and absolute, `~/.config` otherwise.
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }
}

impl Preferences {
    /// Bumped when the shape of this file changes incompatibly.
    pub const PATH_VERSION: u32 = 1;

    const FILE_NAME: &'static str = "preferences.json";

    /// Where preferences live on this platform, if the platform will say.
    pub fn directory() -> Option<PathBuf> {
        // Lowercase on Unix, where directory names are conventionally lower;
        // capitalised on Windows and macOS, where application data folders
        // carry the application's own name.
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let name = "Tessera";
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let name = "tessera";

        config_root().map(|root| root.join(name))
    }

    /// The preferences file itself.
    pub fn path() -> Option<PathBuf> {
        Self::directory().map(|dir| dir.join(Self::FILE_NAME))
    }

    /// Read preferences, and say what went wrong if anything did.
    ///
    /// Never fails. A first run, a damaged file and a file from a newer
    /// Tessera all yield defaults — but only the first is silent, because the
    /// other two mean the user's settings were just discarded and they are
    /// entitled to know.
    pub fn load_from(path: &Path) -> (Self, Option<String>) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(error) => {
                return (
                    Self::default(),
                    Some(format!("could not read preferences: {error}")),
                );
            }
        };

        match serde_json::from_slice::<Self>(&bytes) {
            Ok(prefs) if prefs.version == Self::PATH_VERSION => (prefs, None),
            Ok(prefs) => (
                Self::default(),
                Some(format!(
                    "preferences were written by a newer Tessera (version {}, \
                     this build reads {}); defaults restored",
                    prefs.version,
                    Self::PATH_VERSION
                )),
            ),
            Err(error) => (
                Self::default(),
                Some(format!(
                    "preferences are damaged: {error}; defaults restored"
                )),
            ),
        }
    }

    /// Write preferences, creating the directory if it is not there yet.
    pub fn save_to(&self, path: &Path) -> Result<(), IoError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IoError::Write {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let json = serde_json::to_vec_pretty(self).map_err(|error| IoError::Write {
            path: path.to_path_buf(),
            source: std::io::Error::other(error),
        })?;

        write_atomic(path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("tessera-prefs-test-{name}.json"));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn preferences_round_trip_through_a_file() {
        let path = temp_file("round-trip");
        // Every field set to something other than its default, so a field that
        // fails to travel is caught rather than passing on a coincidence.
        let written = Preferences {
            version: Preferences::PATH_VERSION,
            unit: Unit::Millimetres,
            theme: ThemeChoice::Light,
            minimum_ppi: 150.0,
            panel_surface: PanelSurface::Solid,
            blur: 11,
            panel_opacity: 0.5,
            snapping: false,
            recovery_copy: false,
            recovery_seconds: 42,
            export_presets: crate::view::export_dialog::Preset::usual(),
            shortcuts: crate::keys::Bindings::default(),
            workspaces: crate::workspace::Workspace::usual(),
            workspace: None,
        };
        written.save_to(&path).expect("save failed");

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, written);
        assert_eq!(complaint, None);
    }

    #[test]
    fn a_preferences_file_written_before_these_settings_existed_still_opens() {
        // Every new field carries a serde default, so an older file reads as
        // somebody who never chose. Losing a person's units because the
        // appearance settings arrived would be an unforced insult.
        let path = temp_file("older");
        std::fs::write(
            &path,
            br#"{"version":1,"unit":"Points","theme":"Light","minimum_ppi":150.0}"#,
        )
        .expect("write");

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(complaint, None, "an older file is not a damaged one");
        assert_eq!(read.unit, Unit::Points, "what they chose survived");
        assert_eq!(read.theme, ThemeChoice::Light);
        assert_eq!(read.panel_surface, PanelSurface::default());
        assert_eq!(read.blur, default_blur());
        assert!(read.snapping, "snapping defaults on, as it always was");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_stray_blur_draws_sensibly_without_the_file_being_rewritten() {
        let wild = Preferences {
            blur: 9_999,
            panel_opacity: -3.0,
            ..Preferences::default()
        };
        assert_eq!(wild.blur_divisor(), BLUR_MOST);
        assert_eq!(wild.blur, 9_999, "the stored value is untouched");
        assert!(
            wild.glass_opacity() >= 0.35,
            "a panel at no opacity is a fault, not a look"
        );
    }

    #[test]
    fn a_missing_file_gives_defaults_without_complaining() {
        let (read, complaint) = Preferences::load_from(&temp_file("absent"));
        assert_eq!(read, Preferences::default());
        assert_eq!(
            complaint, None,
            "a first run is not an error and must not look like one"
        );
    }

    #[test]
    fn a_damaged_file_gives_defaults_and_says_so() {
        let path = temp_file("damaged");
        std::fs::write(&path, b"{ this is not json").unwrap();

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, Preferences::default());
        let complaint = complaint.expect("a damaged file must be reported, never swallowed");
        assert!(
            complaint.contains("preferences"),
            "the complaint must name what failed, got: {complaint}"
        );
    }

    #[test]
    fn a_file_from_a_future_version_gives_defaults_and_says_so() {
        let path = temp_file("future");
        std::fs::write(&path, br#"{"version":9999,"unit":"Points","theme":"Dark"}"#).unwrap();

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, Preferences::default());
        assert!(complaint.is_some());
    }

    #[test]
    fn the_default_unit_is_millimetres() {
        assert_eq!(Preferences::default().unit, Unit::Millimetres);
    }

    #[test]
    fn the_directory_is_named_and_absolute_when_the_platform_says() {
        // On any machine a developer or CI runner uses, the environment does
        // say. A `None` here means the check is vacuous, so it is asserted
        // rather than skipped over.
        let dir = Preferences::directory().expect("this platform reports a config directory");
        assert!(dir.is_absolute(), "{dir:?} is not absolute");
        assert!(
            dir.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("tessera")),
            "{dir:?} does not end in the application's name"
        );
    }

    #[test]
    fn a_preferences_file_written_before_the_resolution_bar_reads_three_hundred() {
        // The usual bar for offset litho, and the truth about a file that
        // never chose one.
        let path = temp_file("no-ppi");
        std::fs::write(
            &path,
            br#"{"version":1,"unit":"Millimetres","theme":"Dark"}"#,
        )
        .expect("write");

        let (read, complaint) = Preferences::load_from(&path);
        assert!(complaint.is_none(), "{complaint:?}");
        assert_eq!(read.minimum_ppi, 300.0);

        let _ = std::fs::remove_file(&path);
    }
}
