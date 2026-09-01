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
    pub fn current() -> Self {
        // ISO-8601 timestamps arrive with a date crate in milestone 3, when
        // document metadata becomes user-visible. Empty is honest here; a
        // fabricated date would not be.
        Self {
            format_version: super::FORMAT_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            created: String::new(),
            modified: String::new(),
        }
    }
}
