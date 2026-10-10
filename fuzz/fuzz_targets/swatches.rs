//! An Adobe swatch exchange (`.ase`) file.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = tessera_document::swatch_exchange::read(bytes);
});
