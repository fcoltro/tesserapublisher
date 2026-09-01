//! The ONLY module in the workspace permitted to contain `#[cfg(target_os)]`.
//!
//! Empty in milestone 0, and deliberately so. The Task 1 spike found that
//! eframe selects Vulkan on Windows unprompted and that Vello renders
//! correctly on it, so there is no backend to force and no evidence that
//! would justify forcing one. The rule this module exists to enforce matters
//! now; its contents arrive when a real platform difference does.
