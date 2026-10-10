//! A placed Photoshop file's composite: headers, channels and compression.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = tessera_render::psd::read(bytes);
});
