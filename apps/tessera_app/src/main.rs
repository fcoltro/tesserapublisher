//! Tessera Publisher.
//!
//! Thin by design: this binary wires things together and owns nothing. The
//! interface lives in `tessera_ui`.

mod platform;

fn main() {
    println!("Tessera Publisher {}", env!("CARGO_PKG_VERSION"));
}
