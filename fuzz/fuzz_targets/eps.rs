//! An EPS file's bounding box, read before Ghostscript ever sees it.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let _ = tessera_render::eps::bounding_box(bytes);
});
