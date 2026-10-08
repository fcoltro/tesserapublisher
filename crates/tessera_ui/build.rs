//! The commit Tessera was built from, for Help > About: `TESSERA_COMMIT`,
//! empty when the build is not in a git checkout (a source archive) or git
//! is not there to ask.

fn main() {
    // A new commit moves the branch's ref; a checkout of another moves HEAD.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=TESSERA_COMMIT={commit}");
}
