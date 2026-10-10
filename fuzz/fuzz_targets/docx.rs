//! A Word document placed as a story.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = tessera_import::docx::import_bytes(bytes.to_vec(), std::path::Path::new("fuzzed.docx"));
});
