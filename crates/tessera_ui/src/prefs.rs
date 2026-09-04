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
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: Self::PATH_VERSION,
            // The unit most of the world lays out pages in.
            unit: Unit::Millimetres,
            theme: ThemeChoice::default(),
        }
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
        let written = Preferences {
            version: Preferences::PATH_VERSION,
            unit: Unit::Millimetres,
            theme: ThemeChoice::Light,
        };
        written.save_to(&path).expect("save failed");

        let (read, complaint) = Preferences::load_from(&path);
        assert_eq!(read, written);
        assert_eq!(complaint, None);
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
}
