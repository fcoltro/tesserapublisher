//! A CSV or tab-separated data source for data merge.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = tessera_import::delimited::read(bytes);
});
