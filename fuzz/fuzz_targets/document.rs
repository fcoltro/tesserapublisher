//! A `.tsrdf` file somebody sent: the archive, its JSON and every migration.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = tessera_document::format::load_bytes(
        bytes.to_vec(),
        std::path::Path::new("fuzzed.tsrdf"),
    );
});
