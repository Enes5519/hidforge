//! Device, rule and HID-BPF logic shared by the GUI (`hidforge`) and the root helper
//! (`hidforge-helper`). Depends only on `std`, `libc` and `rust-i18n`, so the helper, which runs as root,
//! links none of the GUI's dependencies (only `rust-i18n`, for its messages).

rust_i18n::i18n!("locales", fallback = "en");

/// Translated message as an owned `String`.
macro_rules! tr {
    ($($args:tt)*) => {
        rust_i18n::t!($($args)*).into_owned()
    };
}

pub mod devices;
pub mod hid;
pub mod hidbpf;
pub mod privileged;
pub mod requirements;
pub mod rules;
