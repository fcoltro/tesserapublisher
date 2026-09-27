//! The archive's `meta.json` entry.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    pub format_version: u32,
    pub app_version: String,
    pub created: String,
    pub modified: String,
}

impl Meta {
    /// `created` and `modified` start empty: `save` fills them from the
    /// dates the application put on the document, as local wall-clock
    /// ISO-8601 (`Stamp::iso`). A document nobody dated saves them empty,
    /// which is honest where a fabricated date would not be.
    pub fn current() -> Self {
        Self {
            format_version: super::FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created: String::new(),
            modified: String::new(),
        }
    }
}
