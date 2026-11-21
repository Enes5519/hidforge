//! Real-root, real-polkit tests of the GUI-side install/uninstall (`selfinstall::install()` /
//! `uninstall()`). They write to /usr, /etc and /var, so they are `#[ignore]`d and refuse to run
//! outside the disposable container set up by tests/root/polkit-e2e.sh. Never run them on a
//! real system.

use hidforge::selfinstall::{self, State};
use std::path::Path;

fn require_container() {
    assert_eq!(
        std::env::var("HIDFORGE_ISOLATED_TEST").as_deref(),
        Ok("1"),
        "refusing: container-only test"
    );
    assert!(
        Path::new("/run/.containerenv").exists() || Path::new("/.dockerenv").exists(),
        "refusing: not inside a container"
    );
}

fn install_update_uninstall() {
    require_container();
    assert_eq!(State::detect(), State::Missing);
    let msg = selfinstall::install().expect("install through pkexec");
    assert!(msg.contains(selfinstall::VERSION), "{msg}");
    assert_eq!(
        State::detect(),
        State::SelfInstalled {
            version: selfinstall::VERSION.into()
        }
    );
    // A second run is a repair/update of the same version and must also succeed.
    selfinstall::install().expect("reinstall");
    selfinstall::uninstall().expect("uninstall through the dedicated polkit action");
    assert_eq!(State::detect(), State::Missing);
}

#[test]
#[ignore = "container only: tests/root/polkit-e2e.sh"]
fn gui_flow_helper_beside_the_program() {
    install_update_uninstall();
}

/// Run with APPIMAGE=1 and XDG_RUNTIME_DIR set, like an AppImage start.
#[test]
#[ignore = "container only: tests/root/polkit-e2e.sh"]
fn gui_flow_appimage_staging() {
    require_container();
    assert!(std::env::var_os("APPIMAGE").is_some());
    install_update_uninstall();
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap();
    let left: Vec<_> = std::fs::read_dir(&runtime)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("hidforge-setup-")
        })
        .collect();
    assert!(left.is_empty(), "staged copy not cleaned up: {left:?}");
}

/// stdin is not a terminal and no agent runs: the GUI gets an actionable message.
#[test]
#[ignore = "container only: tests/root/polkit-e2e.sh"]
fn gui_flow_without_agent_gives_the_sudo_hint() {
    require_container();
    let err = selfinstall::install().expect_err("no agent must fail");
    eprintln!("ERR: {err}");
    assert!(err.contains("polkit"), "{err}");
    assert!(
        err.contains("sudo ") && err.contains("--install-self"),
        "{err}"
    );
    assert_eq!(State::detect(), State::Missing);
}
