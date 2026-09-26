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

/// How tightly the interface is packed.
///
/// A preference rather than a constant, and for the reason Adobe found on its
/// own tools: on an application somebody works inside all day, density belongs
/// to the person and the screen rather than to the design. A fourteen-inch
/// laptop at 150% and a thirty-two-inch display at 100% do not want the same
/// rhythm, and neither pair of eyes is wrong.
///
/// It moves the **spacing scale and the row height**, and nothing else. Type
/// size and corner radius stay put deliberately: a preference that scaled the
/// text too would be a zoom, and a zoom is a different control answering a
/// different question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Density {
    /// Tighter. More of the document and more of the panels at once.
    Compact,
    /// The rhythm the interface was drawn to.
    #[default]
    Standard,
    /// Roomier. Bigger targets, more air between them.
    Comfortable,
}

impl Density {
    pub fn label(self) -> &'static str {
        match self {
            Density::Compact => "Compact",
            Density::Standard => "Standard",
            Density::Comfortable => "Comfortable",
        }
    }

    pub fn purpose(self) -> &'static str {
        match self {
            Density::Compact => "Tighter rows. More on screen at once.",
            Density::Standard => "The rhythm the interface was drawn to.",
            Density::Comfortable => "Roomier rows and larger targets.",
        }
    }

    /// What every spacing step and the row height are multiplied by.
    ///
    /// A quarter either side: enough to be worth choosing, not so much that a
    /// panel laid out for one reflows into a different design in the other.
    pub fn factor(self) -> f32 {
        match self {
            Density::Compact => 0.75,
            Density::Standard => 1.0,
            Density::Comfortable => 1.25,
        }
    }
}

fn default_minimum_ppi() -> f64 {
    300.0
}

/// Where the console's model is and how to be let in.
///
/// **The key is kept in the preferences file, in the clear.** Said here
/// and in the settings window rather than hidden: the file is the person's
/// own, in their own configuration folder, which is where every other
/// desktop tool that takes an API key keeps one — and a key they can see is
/// one they can revoke.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Assistant {
    /// `anthropic`, `openai` (which is also Ollama, Groq, Mistral,
    /// OpenRouter, DeepSeek, LM Studio and Gemini's compatible endpoint), or
    /// empty for none.
    #[serde(default)]
    pub provider: String,
    /// In memory here; on disk in the platform's keychain where there is
    /// one (see [`crate::keychain`]), and in this file only where there is
    /// not, or until the keychain has taken it.
    #[serde(default, skip_serializing_if = "crate::keychain::held")]
    pub api_key: String,
    /// The model's name, as the provider spells it.
    #[serde(default)]
    pub model: String,
    /// Empty for the provider's own; a local server or proxy otherwise.
    #[serde(default)]
    pub base_url: String,
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

    /// How tightly the interface is packed.
    #[serde(default)]
    pub density: Density,
    /// The effective resolution below which artwork is reported as too low.
    ///
    /// A preference rather than a constant: 300 is the usual bar for offset
    /// litho, 150 is fine for newsprint, and 72 is right for a screen PDF.
    /// Hard-coding 300 would cry wolf at every newspaper.
    #[serde(default = "default_minimum_ppi")]
    pub minimum_ppi: f64,

    /// The preflight checks switched off, by [`tessera_preflight::Rule::key`].
    ///
    /// The ones off rather than the ones on, and by name rather than as bits:
    /// a check added later is on for somebody whose preferences were written
    /// before it existed, and a rule moving in the list switches nothing.
    #[serde(default)]
    pub preflight_off: Vec<String>,

    /// Whether the canvas snaps objects to guides and to other objects.
    ///
    /// It lived only in the application before, so it was forgotten between
    /// runs. Somebody who turns snapping off is not turning it off for a minute.
    #[serde(default = "yes")]
    pub snapping: bool,

    /// Whether a straight quote typed on the canvas becomes a typographic one.
    ///
    /// On by default, as it is in every layout tool: a straight quote in set
    /// copy is a mistake nobody makes on purpose. Off for the person setting
    /// code, or feet and inches.
    #[serde(default = "yes")]
    pub typographers_quotes: bool,

    /// Whether unknown words are marked on the canvas as they are typed.
    ///
    /// InDesign's "dynamic spelling". On by default: with no dictionary in
    /// the folder it costs nothing and draws nothing, and with one it is
    /// what every editor now does. Off for the person setting a language
    /// the dictionaries do not cover, or names, or code.
    #[serde(default = "yes")]
    pub dynamic_spelling: bool,

    /// The model the console talks to, and how to reach it.
    #[serde(default)]
    pub assistant: Assistant,

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

    /// Whether Tessera looks for a newer version, and when it last did.
    #[serde(default)]
    pub updates: crate::update::Checking,

    /// How many sides the polygon tool draws.
    ///
    /// A preference rather than a dialog on every use: somebody drawing
    /// hexagons is drawing hexagons all afternoon, and a box asking how many
    /// sides each time is a box they dismiss without reading by the third one.
    #[serde(default = "default_polygon_sides")]
    pub polygon_sides: u32,

    /// How far the inner points of a star are pulled in, as a share of the
    /// radius. Zero is a plain polygon.
    #[serde(default)]
    pub polygon_inset: f64,

    /// Shortcuts anybody has changed from the ones this build ships.
    #[serde(default)]
    pub shortcuts: crate::keys::Bindings,

    /// Where every panel sits: which side, which stack, in what order.
    ///
    /// A preference rather than app state, and that is the whole of what makes
    /// "quit and relaunch, and find the layout as it was" work: the file that
    /// already persists is the one it belongs in.
    #[serde(default)]
    pub docking: crate::docking::Docking,

    /// The arrangement in force, by name.
    ///
    /// Remembered so a relaunch comes back to the arrangement somebody left,
    /// which is the half of "quit and relaunch, and find the layout as it was"
    /// that a list of workspaces does not give on its own.
    #[serde(default)]
    pub workspace: Option<String>,

    /// The Glyphs panel's favourites, and the characters inserted lately.
    ///
    /// Not a setting — nothing anybody resets to put things back to normal —
    /// but kept between runs, as InDesign keeps its recently used glyphs: an
    /// en dash or a section sign wanted today is wanted tomorrow.
    #[serde(default)]
    pub glyphs: crate::view::glyphs::GlyphMemory,
}

fn yes() -> bool {
    true
}

/// Six, which is the polygon anybody draws without being asked.
fn default_polygon_sides() -> u32 {
    6
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
            density: Density::default(),
            minimum_ppi: default_minimum_ppi(),
            preflight_off: Vec::new(),
            snapping: yes(),
            typographers_quotes: yes(),
            dynamic_spelling: yes(),
            assistant: Assistant::default(),
            recovery_copy: yes(),
            recovery_seconds: default_recovery_seconds(),
            export_presets: crate::view::export_dialog::Preset::usual(),
            docking: crate::docking::Docking::default(),
            updates: crate::update::Checking::default(),
            polygon_sides: default_polygon_sides(),
            polygon_inset: 0.0,
            shortcuts: crate::keys::Bindings::default(),
            workspaces: crate::workspace::Workspace::usual(),
            workspace: None,
            glyphs: Default::default(),
        }
    }
}

impl Preferences {
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
    // A headless application — every test — remembers nothing to disk. One
    // did, and every `cargo test` wrote its defaults over the preferences
    // of whoever ran it.
    if !state.persists {
        return;
    }
    let Some(path) = Preferences::path() else {
        // The platform will not say where preferences live, which is a real
        // condition on a stripped-down container rather than an error. They last
        // for this run and that is all that was ever promised.
        return;
    };
    // The key goes to the keychain first, so the file written next can
    // leave it out — or keep it, if the keychain would not take it.
    crate::keychain::store(&state.prefs.assistant.api_key);
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
            density: Density::Comfortable,
            minimum_ppi: 150.0,
            preflight_off: vec!["outside-bleed".to_string()],
            snapping: false,
            typographers_quotes: false,
            dynamic_spelling: false,
            assistant: Assistant::default(),
            recovery_copy: false,
            recovery_seconds: 42,
            export_presets: crate::view::export_dialog::Preset::usual(),
            docking: crate::docking::Docking::default(),
            updates: crate::update::Checking::default(),
            polygon_sides: 6,
            polygon_inset: 0.0,
            shortcuts: crate::keys::Bindings::default(),
            workspaces: crate::workspace::Workspace::usual(),
            workspace: None,
            glyphs: crate::view::glyphs::GlyphMemory {
                recent: vec!['§', '—'],
                favourites: vec!['→'],
            },
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
        assert_eq!(
            read.density,
            Density::default(),
            "a file written before the density existed reads as somebody who never chose"
        );
        assert!(read.snapping, "snapping defaults on, as it always was");

        let _ = std::fs::remove_file(&path);
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
