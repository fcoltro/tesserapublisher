//! An InDesign package: the zip, its XML, and everything read from it.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ =
        tessera_import::idml::import_bytes(bytes.to_vec(), std::path::Path::new("fuzzed.idml"));
});
