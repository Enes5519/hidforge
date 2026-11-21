//! Privileged operations. The GUI never runs as root: it starts the separate `hidforge-helper`
//! binary through `pkexec`, which validates input before touching `/etc/udev/rules.d`.

use crate::devices;
use crate::hid;
use crate::hidbpf;
use crate::requirements;
use crate::rules::{self, RULES_DIR};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const APPLY_FLAG: &str = "--apply";
pub const REMOVE_FLAG: &str = "--remove";
pub const BPF_INSTALL_FLAG: &str = "--bpf-install";
pub const BPF_REMOVE_FLAG: &str = "--bpf-remove";

/// Where packages install the helper; `assets/org.hidforge.helper.policy` names this path.
pub const HELPER_PATH: &str = "/usr/libexec/hidforge-helper";

/// Largest rule the helper reads from stdin; `rules::validate` enforces the real limit.
const MAX_STDIN: u64 = 8192;

// ---------------------------------------------------------------------------------------------
// Unprivileged side (GUI)
// ---------------------------------------------------------------------------------------------

/// Install `content` as `/etc/udev/rules.d/<name>` (asks for the admin password).
pub fn apply(name: &str, content: &str) -> Result<String, String> {
    rules::validate(name, content)?;
    run_pkexec(&[APPLY_FLAG, name], content)
}

/// Remove a rule this tool installed (asks for the admin password).
pub fn remove(name: &str) -> Result<String, String> {
    rules::id_from_file_name(name).ok_or_else(|| tr!("err.invalid_file_name", name = name))?;
    run_pkexec(&[REMOVE_FLAG, name], "")
}

/// Generate, build, install and load a HID-BPF descriptor fixup for `/dev/hidrawN`.
/// Only the node name crosses the privilege boundary; root re-derives everything from sysfs.
pub fn bpf_install(node: &Path) -> Result<String, String> {
    let name = hidraw_name(node).ok_or_else(|| tr!("err.invalid_device", name = node.display()))?;
    run_pkexec(&[BPF_INSTALL_FLAG, name], "")
}

/// Unload and delete a fixup this tool installed.
pub fn bpf_remove(object: &str) -> Result<String, String> {
    hidbpf::parse_object_name(object).ok_or_else(|| tr!("err.invalid_file_name", name = object))?;
    run_pkexec(&[BPF_REMOVE_FLAG, object], "")
}

/// The installed helper; debug builds (`cargo run`) may use the one built next to them.
// shortcut: the debug fallback is user-writable and has no polkit policy; fine for development,
// drop it if debug builds are ever distributed.
fn helper_path() -> Result<PathBuf, String> {
    let installed = PathBuf::from(HELPER_PATH);
    if installed.is_file() {
        return Ok(installed);
    }
    // Release builds never fall back: a user-writable helper in a build dir would run as root.
    if !cfg!(debug_assertions) {
        return Err(tr!("err.helper_not_installed", path = HELPER_PATH));
    }
    let exe = std::env::current_exe().map_err(|e| tr!("err.exe_path", error = e))?;
    let sibling = exe.with_file_name("hidforge-helper");
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(tr!("err.helper_not_found", path = HELPER_PATH))
    }
}

/// `hidrawN` (digits only), or `None`.
fn hidraw_name(node: &Path) -> Option<&str> {
    let name = node.file_name()?.to_str()?;
    let n = name.strip_prefix("hidraw")?;
    (!n.is_empty() && n.len() <= 4 && n.bytes().all(|b| b.is_ascii_digit())).then_some(name)
}

fn path_str(p: &Path) -> Result<&str, String> {
    p.to_str().ok_or_else(|| tr!("err.temp_path_utf8"))
}

/// Run the helper as root with `input` on its stdin.
fn run_pkexec(args: &[&str], input: &str) -> Result<String, String> {
    let mut child = Command::new("pkexec")
        .arg(helper_path()?)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tr!("err.pkexec_start", error = e))?;
    if let Some(mut stdin) = child.stdin.take() {
        // Rules are under 4 KiB, so the pipe buffers them while the password prompt is open.
        // A write error means pkexec already exited (e.g. prompt dismissed); its status says why.
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| tr!("err.pkexec_wait", error = e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    match out.status.code() {
        Some(0) => Ok(stdout),
        Some(126) | Some(127)
            if stderr.is_empty()
                || stderr.contains("dismissed")
                || stderr.contains("Not authorized") =>
        {
            Err(tr!("err.auth_denied"))
        }
        _ => Err(if stderr.is_empty() {
            tr!("err.operation_failed", status = out.status)
        } else {
            stderr
        }),
    }
}

// ---------------------------------------------------------------------------------------------
// Privileged side (runs as root via pkexec)
// ---------------------------------------------------------------------------------------------

/// Entry point for helper mode. Returns the process exit code.
pub fn helper_main(args: &[String]) -> i32 {
    let result = match args {
        [flag, name] if flag == APPLY_FLAG => helper_apply(name),
        [flag, name] if flag == REMOVE_FLAG => helper_remove(name),
        [flag, name] if flag == BPF_INSTALL_FLAG => helper_bpf_install(name),
        [flag, name] if flag == BPF_REMOVE_FLAG => helper_bpf_remove(name),
        _ => Err(tr!("err.usage")),
    };
    match result {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn require_root() -> Result<(), String> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        return Err(tr!("err.need_root"));
    }
    Ok(())
}

fn helper_apply(name: &str) -> Result<String, String> {
    require_root()?;
    // Read once into memory; validate what we will actually write.
    let mut content = String::new();
    std::io::stdin()
        .take(MAX_STDIN)
        .read_to_string(&mut content)
        .map_err(|e| tr!("err.rule_read", error = e))?;
    let id = rules::validate(name, &content)?;

    let dir = Path::new(RULES_DIR);
    fs::create_dir_all(dir).map_err(|e| format!("{RULES_DIR}: {e}"))?;
    // Per-process name: two helpers for the same device must not share a staging file.
    let staging = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let target = dir.join(name);
    let _ = fs::remove_file(&staging);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&staging)
        .and_then(|mut f| {
            f.write_all(content.as_bytes())?;
            f.sync_all()
        })
        .map_err(|e| format!("{}: {e}", staging.display()))?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o644)).map_err(|e| e.to_string())?;
    fs::rename(&staging, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    // Drop the copy an older version wrote under the pre-rename name, so the device is not
    // matched by two of our rules. The name is derived from the validated id only.
    let legacy = dir.join(rules::legacy_file_name(&id));
    if legacy != target {
        let _ = fs::remove_file(&legacy);
    }

    Ok(with_reload(tr!(
        "helper.installed",
        path = target.display()
    )))
}

fn helper_remove(name: &str) -> Result<String, String> {
    require_root()?;
    rules::id_from_file_name(name).ok_or_else(|| tr!("err.invalid_file_name", name = name))?;
    let target = Path::new(RULES_DIR).join(name);
    fs::remove_file(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(with_reload(tr!("helper.deleted", path = target.display())))
}

/// The file change already happened, so a failed reload is a warning, not a failure: the rule
/// takes effect at the next plug-in anyway.
fn with_reload(done: String) -> String {
    match reload_udev() {
        Ok(()) => done,
        Err(e) => tr!("helper.reload_failed", done = done, error = e),
    }
}

fn helper_bpf_install(name: &str) -> Result<String, String> {
    require_root()?;
    let node = PathBuf::from("/dev").join(name);
    if hidraw_name(&node) != Some(name) {
        return Err(tr!("err.invalid_device", name = name));
    }
    let dev = devices::scan()
        .into_iter()
        .find(|d| d.hidraw_nodes.contains(&node))
        .ok_or_else(|| tr!("err.usb_not_found", name = name))?;
    let iface =
        devices::interface_number(&node).ok_or_else(|| tr!("err.iface_unreadable", name = name))?;
    let syspath =
        devices::hid_syspath(&node).ok_or_else(|| tr!("err.hid_not_found", name = name))?;
    let desc = devices::report_descriptor(&node)
        .ok_or_else(|| tr!("err.descriptor_unreadable", name = name))?;
    let analysis = hid::analyze(&desc);
    if !analysis.needs_fix() {
        return Err(tr!("err.no_fix_needed", name = name));
    }
    let missing = requirements::check();
    if !missing.is_empty() {
        return Err(requirements::describe(&missing));
    }
    let tool = |n: &str| {
        requirements::find_tool(n)
            .and_then(|p| p.to_str().map(str::to_string))
            .ok_or_else(|| tr!("err.tool_not_found", name = n))
    };
    let (clang, loader) = (tool("clang")?, tool("udev-hid-bpf")?);
    let fixed = hid::apply(&desc, &analysis.inserts);
    let object = hidbpf::object_name(&dev.id, iface);

    // Private build directory; removed on every exit path.
    // /run is root-only, unlike /tmp.
    let build = PathBuf::from(format!("/run/hidforge-bpf.{}", std::process::id()));
    let _ = fs::remove_dir_all(&build);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&build)
        .map_err(|e| format!("{}: {e}", build.display()))?;
    let result = (|| {
        let src = build.join("fixup.bpf.c");
        let obj = build.join(&object);
        fs::write(&src, hidbpf::generate_source(&dev.id, &desc, &fixed)?)
            .map_err(|e| e.to_string())?;
        let (src_s, obj_s) = (path_str(&src)?, path_str(&obj)?);
        let multiarch = requirements::multiarch_include().map(|p| format!("-I{}", p.display()));
        let mut args: Vec<&str> = hidbpf::CLANG_ARGS.to_vec();
        args.extend(multiarch.as_deref());
        args.extend(["-c", src_s, "-o", obj_s]);
        run(&clang, &args)?;
        run(&loader, &["install", "--force", obj_s])?;
        Ok::<(), String>(())
    })();
    let _ = fs::remove_dir_all(&build);
    result?;

    // Load now as well; on failure the installed rule still applies at next plug-in.
    let installed = Path::new(hidbpf::BPF_DIR).join(&object);
    let load = run(
        &loader,
        &["add", path_str(&syspath)?, path_str(&installed)?],
    );
    let _ = run("udevadm", &["settle", "--timeout=5"]);
    match load {
        Ok(()) => Ok(tr!("helper.bpf_loaded", object = object)),
        Err(e) => Ok(tr!(
            "helper.bpf_installed_not_loaded",
            object = object,
            error = e
        )),
    }
}

fn helper_bpf_remove(object: &str) -> Result<String, String> {
    require_root()?;
    let (id, iface) = hidbpf::parse_object_name(object)
        .ok_or_else(|| tr!("err.invalid_file_name", name = object))?;
    if let Some(loader) =
        requirements::find_tool("udev-hid-bpf").and_then(|p| p.to_str().map(str::to_string))
    {
        for syspath in devices::hid_syspaths_for(&id, iface) {
            if let Some(p) = syspath.to_str() {
                let _ = run(&loader, &["remove", p]);
            }
        }
    }
    let obj = Path::new(hidbpf::BPF_DIR).join(object);
    let rule = hidbpf::rule_path(object);
    for p in [&obj, &rule] {
        match fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", p.display())),
        }
    }
    // Files are already gone, so a failed reload is a warning (same as `with_reload`).
    let done = tr!("helper.bpf_removed", object = object);
    if let Err(e) = run("udevadm", &["control", "--reload-rules"]) {
        return Ok(tr!("helper.reload_failed", done = done, error = e));
    }
    let _ = run("udevadm", &["settle", "--timeout=5"]);
    Ok(done)
}

fn reload_udev() -> Result<(), String> {
    run("udevadm", &["control", "--reload-rules"])?;
    run(
        "udevadm",
        &[
            "trigger",
            "--action=change",
            "--subsystem-match=hidraw",
            "--subsystem-match=usb",
        ],
    )?;
    // `trigger --settle` has no timeout; a stuck udev worker would hang the helper (and the GUI).
    let _ = run("udevadm", &["settle", "--timeout=5"]);
    Ok(())
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| tr!("err.cmd_start", cmd = cmd, error = e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidraw_name_is_strict() {
        assert_eq!(hidraw_name(Path::new("/dev/hidraw12")), Some("hidraw12"));
        assert_eq!(hidraw_name(Path::new("hidraw0")), Some("hidraw0"));
        for bad in [
            "/dev/hidraw",
            "/dev/hidraw1x",
            "/dev/hidraw-1",
            "/dev/sda",
            "/dev/hidraw12345",
            "/dev/hidraw1/..",
        ] {
            assert_eq!(hidraw_name(Path::new(bad)), None, "{bad}");
        }
    }
}
