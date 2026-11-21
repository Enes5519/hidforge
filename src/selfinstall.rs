//! First-run install of the root helper and its polkit policy (and their removal).
//!
//! The extracted tarball / AppImage runs without any install. For root work the GUI asks for
//! one authorization and starts `hidforge-helper --install-self` through `pkexec`. That mode
//! copies the *running executable* (`/proc/self/exe`, one fd, never re-resolved by path) to
//! `HELPER_PATH` and writes the polkit policy compiled into the binary, so nothing is read from
//! a user-controlled path after authorization. No shell, no user-supplied strings.
//!
//! Limits (see the README): the tarball itself is trusted, as with `sudo ./install.sh`; the
//! version check is a guard against old tarballs, not a signature check.

use crate::privileged::HELPER_PATH;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const INSTALL_FLAG: &str = "--install-self";
pub const UNINSTALL_FLAG: &str = "--uninstall-self";
pub const VERSION_FLAG: &str = "--version";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const POLICY_PATH: &str = "/usr/share/polkit-1/actions/org.hidforge.helper.policy";
/// Compiled in: the policy always matches this exact helper build and is never read from disk.
const POLICY: &str = include_str!("../assets/org.hidforge.helper.policy");
/// Root-owned record that *this mechanism* installed the files; absent for packages.
const STATE_DIR: &str = "/var/lib/hidforge";
const MANIFEST: &str = "/var/lib/hidforge/self-install";
/// Serializes installs and removals; `/run` is root-only, so nobody else can plant this file.
const LOCK: &str = "/run/hidforge-setup.lock";
/// A package manager's "who owns this file" command. Exit 0 = owned. "Not owned" is exit 1 *and*
/// the tool's own sentence (checked with `LC_ALL=C`; wording verified against the real tools):
/// any other outcome (missing database, lock, crash) is "unknown" and is never read as "free".
struct Query {
    tool: &'static str,
    args: &'static [&'static str],
    not_owned: &'static str,
}

const OWNER_QUERIES: [Query; 3] = [
    Query {
        tool: "dpkg",
        args: &["-S"],
        not_owned: "no path found matching pattern",
    },
    Query {
        tool: "rpm",
        args: &["-qf"],
        not_owned: "is not owned by any package",
    },
    Query {
        tool: "pacman",
        args: &["-Qo"],
        not_owned: "no package owns",
    },
];
/// Shipped by distribution packages; while it exists we never touch the files they own.
const PACKAGE_MARKER: &str = "/usr/share/hidforge/package-managed";

// ---------------------------------------------------------------------------------------------
// Unprivileged side
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// No helper installed: offer the install.
    Missing,
    /// Installed by `--install-self`; `version` is the version it recorded.
    SelfInstalled { version: String },
    /// Package manager or manual install: never modified by hidforge.
    Unmanaged,
}

impl State {
    pub fn detect() -> Self {
        if Path::new(PACKAGE_MARKER).exists() {
            return Self::Unmanaged;
        }
        let manifest = read_manifest();
        match (Path::new(HELPER_PATH).is_file(), manifest) {
            (false, _) => Self::Missing,
            (true, Some(version)) => Self::SelfInstalled { version },
            (true, None) => Self::Unmanaged,
        }
    }

    /// The installed helper is older than this GUI.
    pub fn needs_update(&self) -> bool {
        matches!(self, Self::SelfInstalled { version }
            if parse_version(version) < parse_version(VERSION))
    }
}

/// Authorize once and install the helper that ships next to this program.
pub fn install() -> Result<String, String> {
    let staged = stage_helper()?;
    let result = (|| {
        let meta =
            fs::metadata(&staged.path).map_err(|e| format!("{}: {e}", staged.path.display()))?;
        crate::privileged::run_pkexec_program(&staged.path, &[INSTALL_FLAG, &identity(&meta)], "")
    })();
    // No polkit agent: the error tells the person to run this exact path with sudo, so keep the
    // private copy (tmpfs, 0700, gone at logout) instead of deleting what the message points to.
    let hint_needs_file = staged.dir.is_some()
        && result
            .as_ref()
            .is_err_and(|e| e.contains(&staged.path.display().to_string()));
    if !hint_needs_file {
        staged.cleanup();
    }
    result
}

/// Remove what `install` put on the system (asks for the admin password via the helper's policy).
pub fn uninstall() -> Result<String, String> {
    crate::privileged::run_pkexec(&[UNINSTALL_FLAG], "")
}

#[derive(Debug)]
struct Staged {
    path: PathBuf,
    /// Private directory to delete afterwards (AppImage only).
    dir: Option<PathBuf>,
}

impl Staged {
    fn cleanup(&self) {
        if let Some(dir) = &self.dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// The helper file `pkexec` will run. Beside the executable; inside an AppImage the FUSE mount is
/// unreadable for root, so copy it into a fresh 0700 directory under `$XDG_RUNTIME_DIR` first.
fn stage_helper() -> Result<Staged, String> {
    let exe = std::env::current_exe().map_err(|e| tr!("err.exe_path", error = e))?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    stage_helper_from(
        &exe.with_file_name("hidforge-helper"),
        std::env::var_os("APPIMAGE").is_some(),
        runtime,
    )
}

fn stage_helper_from(
    sibling: &Path,
    appimage: bool,
    runtime: Option<PathBuf>,
) -> Result<Staged, String> {
    if !sibling.is_file() {
        return Err(tr!("err.helper_not_found", path = sibling.display()));
    }
    if !appimage {
        let path = fs::canonicalize(sibling).map_err(|e| format!("{}: {e}", sibling.display()))?;
        return Ok(Staged { path, dir: None });
    }
    let runtime = runtime
        .filter(|p| p.is_absolute())
        .ok_or_else(|| tr!("err.setup_no_runtime_dir"))?;
    let dir = runtime.join(format!("hidforge-setup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join("hidforge-helper");
    let copied = File::open(sibling).and_then(|mut src| {
        let mut dst = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)?;
        io::copy(&mut src, &mut dst)?;
        dst.sync_all()
    });
    let staged = Staged {
        path,
        dir: Some(dir),
    };
    match copied {
        Ok(()) => Ok(staged),
        Err(e) => {
            staged.cleanup();
            Err(format!("{}: {e}", sibling.display()))
        }
    }
}

/// Identity of a file as `dev:ino:size:ctime`; any write, replace or chmod after the GUI looked
/// at the file changes it (ctime cannot be set from userspace).
fn identity(meta: &fs::Metadata) -> String {
    format!(
        "{}:{}:{}:{}.{}",
        meta.dev(),
        meta.ino(),
        meta.size(),
        meta.ctime(),
        meta.ctime_nsec()
    )
}

// ---------------------------------------------------------------------------------------------
// Privileged side (runs as root)
// ---------------------------------------------------------------------------------------------

/// `hidforge-helper --install-self [IDENTITY]`. `IDENTITY` is passed by the GUI; a person running
/// it by hand through sudo omits it.
pub fn helper_install(identity_arg: Option<&str>) -> Result<String, String> {
    require_root()?;
    let mut exe = File::open("/proc/self/exe").map_err(|e| format!("/proc/self/exe: {e}"))?;
    let meta = exe.metadata().map_err(|e| e.to_string())?;
    if let Some(expected) = identity_arg {
        // The file `pkexec` ran must be the one the GUI inspected before the password prompt.
        if expected != identity(&meta) {
            return Err(tr!("err.setup_changed"));
        }
    }
    install_files(&mut exe, &meta, &Paths::system())
}

/// `hidforge-helper --uninstall-self`.
pub fn helper_uninstall() -> Result<String, String> {
    require_root()?;
    uninstall_files(&Paths::system())
}

/// Locations, so the logic can be tested in a scratch directory.
struct Paths {
    helper: PathBuf,
    policy: PathBuf,
    state_dir: PathBuf,
    manifest: PathBuf,
    marker: PathBuf,
    lock: PathBuf,
    /// Installed package managers' "who owns this file" commands.
    queries: Vec<(PathBuf, &'static Query)>,
    /// Besides root, the only uid allowed to own the directories (0 in production; tests only).
    owner: u32,
}

impl Paths {
    fn system() -> Self {
        Self {
            helper: HELPER_PATH.into(),
            policy: POLICY_PATH.into(),
            state_dir: STATE_DIR.into(),
            manifest: MANIFEST.into(),
            marker: PACKAGE_MARKER.into(),
            lock: LOCK.into(),
            queries: OWNER_QUERIES
                .iter()
                .filter_map(|q| Some((crate::requirements::find_tool(q.tool)?, q)))
                .collect(),
            owner: 0,
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

/// Held while installing or removing; released when dropped (or when the process dies).
struct Lock(#[allow(dead_code)] File);

/// Non-blocking exclusive lock: a second installer fails fast instead of waiting or racing.
fn acquire_lock(p: &Paths) -> Result<Lock, String> {
    ensure_root_dir(parent(&p.lock)?, p.owner, false)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&p.lock)
        .map_err(|e| io_err(&p.lock, &e))?;
    // SAFETY: the descriptor is open and owned by `file` for the whole call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(tr!("err.setup_busy"));
    }
    Ok(Lock(file))
}

/// Refuse to touch an existing `path` that a package owns, or whose ownership cannot be
/// established because a query failed (fail closed: neither overwrite nor delete). A path that does
/// not exist cannot be owned. With no package manager installed there is nothing to ask; the
/// marker file and our own record are then the only evidence.
fn check_not_package_owned(path: &Path, p: &Paths) -> Result<(), String> {
    if fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    for (tool, query) in &p.queries {
        let out = Command::new(tool)
            .args(query.args)
            .arg(path)
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output();
        let Ok(out) = out else {
            return Err(tr!("err.setup_owner_unknown", tool = query.tool));
        };
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .to_lowercase();
        match out.status.code() {
            Some(0) => return Err(tr!("err.setup_packaged")),
            Some(1) if text.contains(query.not_owned) => {}
            _ => return Err(tr!("err.setup_owner_unknown", tool = query.tool)),
        }
    }
    Ok(())
}

fn install_files(exe: &mut File, meta: &fs::Metadata, p: &Paths) -> Result<String, String> {
    let (helper_dir, policy_dir) = (parent(&p.helper)?, parent(&p.policy)?);
    let dirs = [p.state_dir.as_path(), helper_dir, policy_dir];
    // Trust the directories before trusting anything stored in them (the record, existing files).
    for dir in dirs {
        ensure_root_dir(dir, p.owner, false)?;
    }
    let _lock = acquire_lock(p)?;
    if p.marker.exists() {
        return Err(tr!("err.setup_packaged"));
    }
    check_not_package_owned(&p.helper, p)?;
    check_not_package_owned(&p.policy, p)?;
    // Running the installed helper: nothing to install, and it must not rewrite itself.
    if fs::metadata(&p.helper).is_ok_and(|h| h.dev() == meta.dev() && h.ino() == meta.ino()) {
        return Err(tr!("err.setup_already_installed"));
    }
    let recorded = read_manifest_at(&p.manifest);
    let exists = |path: &Path| fs::symlink_metadata(path).is_ok();
    // Files without our record belong to a package or the administrator: never overwrite.
    if recorded.is_none() && (exists(&p.helper) || exists(&p.policy)) {
        return Err(tr!("err.setup_foreign"));
    }
    if let Some(old) = &recorded
        && parse_version(VERSION) < parse_version(old)
    {
        return Err(tr!(
            "err.setup_downgrade",
            installed = old,
            version = VERSION
        ));
    }

    // Only now create what is missing (a refused install leaves no directories behind).
    for dir in dirs {
        ensure_root_dir(dir, p.owner, true)?;
    }
    // Record ownership before the files exist (version unchanged on an update), so a crash
    // never leaves files that look foreign; the new version is recorded only after success.
    let fresh = recorded.is_none();
    if fresh {
        write_atomic(
            &p.state_dir,
            &p.manifest,
            0o644,
            manifest_text("0.0.0").as_bytes(),
        )?;
    }
    // Atomic replacement: a failure leaves the previous helper working.
    let written = (|| {
        write_atomic_from(helper_dir, &p.helper, 0o755, exe)?;
        write_atomic(policy_dir, &p.policy, 0o644, POLICY.as_bytes())?;
        write_atomic(
            &p.state_dir,
            &p.manifest,
            0o644,
            manifest_text(VERSION).as_bytes(),
        )
    })();
    if let Err(e) = written {
        if fresh {
            // Nothing of ours existed before this run (checked above): undo it completely.
            for path in [&p.policy, &p.helper, &p.manifest] {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir(&p.state_dir);
        }
        return Err(e);
    }
    Ok(tr!("helper.setup_installed", version = VERSION))
}

fn uninstall_files(p: &Paths) -> Result<String, String> {
    for dir in [
        p.state_dir.as_path(),
        parent(&p.helper)?,
        parent(&p.policy)?,
    ] {
        ensure_root_dir(dir, p.owner, false)?;
    }
    let _lock = acquire_lock(p)?;
    if p.marker.exists() {
        return Err(tr!("err.setup_packaged"));
    }
    check_not_package_owned(&p.helper, p)?;
    check_not_package_owned(&p.policy, p)?;
    if read_manifest_at(&p.manifest).is_none() {
        return Err(tr!("err.setup_not_ours"));
    }
    // Policy first, helper next, record last: a crash leaves something retryable.
    for path in [&p.policy, &p.helper, &p.manifest] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(path, &e)),
        }
    }
    let _ = fs::remove_dir(&p.state_dir);
    Ok(tr!("helper.setup_removed"))
}

/// `path: error`, except a read-only file system gets an actionable message (immutable
/// distributions such as Silverblue or SteamOS keep `/usr` read-only).
fn io_err(path: &Path, e: &io::Error) -> String {
    if e.raw_os_error() == Some(libc::EROFS) {
        tr!("err.setup_readonly", path = path.display())
    } else {
        format!("{}: {e}", path.display())
    }
}

fn parent(path: &Path) -> Result<&Path, String> {
    path.parent().ok_or_else(|| path.display().to_string())
}

fn manifest_text(version: &str) -> String {
    format!("version={version}\n")
}

fn read_manifest() -> Option<String> {
    read_manifest_at(Path::new(MANIFEST))
}

fn read_manifest_at(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("version="))
        .map(str::to_string)
}

/// `X.Y.Z` or `X.Y.Z-rc.N` as a comparable value: a release candidate sorts below its final
/// release (`0.1.0-rc.1 < 0.1.0-rc.2 < 0.1.0`); any other suffix, or garbage, sorts lowest.
fn parse_version(v: &str) -> [u64; 4] {
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let mut out = [0; 4];
    for (slot, part) in out.iter_mut().zip(core.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    out[3] = match pre {
        None => u64::MAX,
        Some(p) => p
            .strip_prefix("rc.")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0),
    };
    out
}

/// Every existing component from `/` down is a real directory (no symlink) owned by root that
/// nobody else can write to. With `create`, missing ones are made 0755 (and checked too); without,
/// a missing component ends the walk, since nothing below it exists to be trusted or distrusted.
fn ensure_root_dir(dir: &Path, owner: u32, create: bool) -> Result<(), String> {
    let mut cur = PathBuf::from("/");
    for part in dir.components().skip(1) {
        cur.push(part);
        let meta = match fs::symlink_metadata(&cur) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !create => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o755)
                    .create(&cur)
                    .map_err(|e| io_err(&cur, &e))?;
                fs::set_permissions(&cur, fs::Permissions::from_mode(0o755))
                    .map_err(|e| io_err(&cur, &e))?;
                fs::symlink_metadata(&cur).map_err(|e| io_err(&cur, &e))?
            }
            Err(e) => return Err(io_err(&cur, &e)),
        };
        if !meta.is_dir() || (meta.uid() != 0 && meta.uid() != owner) || meta.mode() & 0o022 != 0 {
            return Err(tr!("err.setup_insecure_dir", path = cur.display()));
        }
    }
    Ok(())
}

fn write_atomic(dir: &Path, target: &Path, mode: u32, data: &[u8]) -> Result<(), String> {
    write_atomic_from(dir, target, mode, &mut &*data)
}

/// Stage next to `target` (exclusive create, never follows a symlink), flush to disk, then
/// `rename` over it: readers see the old or the new file, never half of one. `rename` replaces a
/// symlink or hard link at `target` instead of writing through it.
fn write_atomic_from(
    dir: &Path,
    target: &Path,
    mode: u32,
    src: &mut impl io::Read,
) -> Result<(), String> {
    let name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| target.display().to_string())?;
    remove_stale(dir, name);
    let staging = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&staging)?;
        io::copy(src, &mut f)?;
        // The creation mode is subject to umask; set the final one explicitly.
        f.set_permissions(fs::Permissions::from_mode(mode))?;
        f.flush()?;
        f.sync_all()?;
        fs::rename(&staging, target)?;
        File::open(dir)?.sync_all()
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&staging);
        return Err(io_err(target, &e));
    }
    Ok(())
}

/// Leftovers of an earlier crashed run (our own staging names only). Callers hold the install
/// lock, so no live installer can own one of these files.
fn remove_stale(dir: &Path, name: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let prefix = format!(".{name}.");
    for e in entries.flatten() {
        let file = e.file_name();
        let Some(file) = file.to_str() else { continue };
        if file.starts_with(&prefix)
            && file.ends_with(".tmp")
            && e.file_type().is_ok_and(|t| t.is_file())
        {
            let _ = fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn scratch(tag: &str) -> PathBuf {
        // Not /tmp: its sticky world-writable mode is rightly refused by `ensure_root_dir`.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-scratch")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn paths(root: &Path) -> Paths {
        Paths {
            helper: root.join("libexec/hidforge-helper"),
            policy: root.join("actions/org.hidforge.helper.policy"),
            state_dir: root.join("state"),
            manifest: root.join("state/self-install"),
            marker: root.join("share/package-managed"),
            lock: root.join("setup.lock"),
            queries: Vec::new(),
            // SAFETY: geteuid has no preconditions.
            owner: unsafe { libc::geteuid() },
        }
    }

    #[test]
    fn policy_names_the_installed_helper_path() {
        let annotation = format!(">{HELPER_PATH}</annotate>");
        assert!(POLICY.contains("org.freedesktop.policykit.exec.path"));
        assert!(POLICY.contains(&annotation));
        // No credential caching: every privileged operation asks again.
        assert!(!POLICY.contains("_keep"));
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
        assert!(parse_version("0.1.0-rc.1") < parse_version("0.1.0-rc.2"));
        assert!(parse_version("0.1.0-rc.2") < parse_version("0.1.0"));
        assert!(parse_version("0.1.0-rc.10") > parse_version("0.1.0-rc.9"));
        assert!(parse_version("0.1.0") < parse_version("0.1.1-rc.1"));
        assert!(parse_version("0.1.0-beta") < parse_version("0.1.0-rc.1"));
        assert!(parse_version("garbage") < parse_version("0.0.1-rc.1"));
    }

    #[test]
    fn identity_changes_when_the_file_is_modified() {
        let dir = scratch("identity");
        let f = dir.join("h");
        fs::write(&f, b"one").unwrap();
        let before = identity(&fs::metadata(&f).unwrap());
        assert_eq!(before, identity(&fs::metadata(&f).unwrap()));
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&f, b"two!").unwrap();
        assert_ne!(before, identity(&fs::metadata(&f).unwrap()));
        // A replacement through rename is a different inode.
        let other = dir.join("o");
        fs::write(&other, b"two!").unwrap();
        let before = identity(&fs::metadata(&f).unwrap());
        fs::rename(&other, &f).unwrap();
        assert_ne!(before, identity(&fs::metadata(&f).unwrap()));
    }

    #[test]
    fn atomic_write_replaces_symlink_and_never_writes_through() {
        let dir = scratch("symlink");
        let victim = dir.join("victim");
        fs::write(&victim, b"keep").unwrap();
        let target = dir.join("target");
        symlink(&victim, &target).unwrap();
        write_atomic(&dir, &target, 0o644, b"new").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert_eq!(fs::read(&target).unwrap(), b"new");
        assert!(
            !fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::metadata(&target).unwrap().mode() & 0o777, 0o644);
    }

    #[test]
    fn atomic_write_replaces_hard_link_without_touching_the_other_name() {
        let dir = scratch("hardlink");
        let other = dir.join("other");
        fs::write(&other, b"keep").unwrap();
        let target = dir.join("target");
        fs::hard_link(&other, &target).unwrap();
        write_atomic(&dir, &target, 0o755, b"new").unwrap();
        assert_eq!(fs::read(&other).unwrap(), b"keep");
        assert_eq!(fs::read(&target).unwrap(), b"new");
    }

    #[test]
    fn stale_staging_files_are_cleaned_but_other_files_stay() {
        let dir = scratch("stale");
        fs::write(dir.join(".target.1.tmp"), b"x").unwrap();
        fs::write(dir.join(".other.1.tmp"), b"x").unwrap();
        write_atomic(&dir, &dir.join("target"), 0o644, b"new").unwrap();
        assert!(!dir.join(".target.1.tmp").exists());
        assert!(dir.join(".other.1.tmp").exists());
    }

    #[test]
    fn directories_must_be_real_and_not_writable_by_others() {
        let dir = scratch("dirs");
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        assert!(ensure_root_dir(&dir, me, true).is_ok());
        // Production rule (owner 0): a directory owned by an ordinary user is refused.
        if me != 0 {
            assert!(ensure_root_dir(&dir, 0, true).is_err());
        }
        let link = dir.join("link");
        symlink("/usr", &link).unwrap();
        assert!(ensure_root_dir(&link, me, true).is_err());
        // Group/other-writable directories are refused.
        let open = dir.join("open");
        fs::create_dir(&open).unwrap();
        fs::set_permissions(&open, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(ensure_root_dir(&open, me, true).is_err());
        assert!(ensure_root_dir(Path::new("/usr"), 0, true).is_ok());
    }

    #[test]
    fn install_refuses_foreign_files_packages_and_downgrades() {
        let root = scratch("install");
        let p = paths(&root);
        let exe_path = root.join("exe");
        fs::write(&exe_path, b"binary").unwrap();
        let mut exe = File::open(&exe_path).unwrap();
        let meta = exe.metadata().unwrap();

        // Existing helper with no record (package or manual install): untouched.
        fs::create_dir_all(p.helper.parent().unwrap()).unwrap();
        fs::write(&p.helper, b"packaged").unwrap();
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert_eq!(fs::read(&p.helper).unwrap(), b"packaged");
        fs::remove_file(&p.helper).unwrap();

        // Package marker wins over everything.
        fs::create_dir_all(p.marker.parent().unwrap()).unwrap();
        fs::write(&p.marker, b"").unwrap();
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert!(uninstall_files(&p).is_err());
        fs::remove_file(&p.marker).unwrap();

        // Recorded newer version: no downgrade.
        fs::create_dir_all(&p.state_dir).unwrap();
        fs::write(&p.manifest, "version=999.0.0\n").unwrap();
        assert!(install_files(&mut exe, &meta, &p).is_err());
    }

    #[test]
    fn install_update_and_failed_update_keep_a_working_helper() {
        let root = scratch("update");
        let p = paths(&root);
        let exe_path = root.join("exe");
        fs::write(&exe_path, b"binary-v1").unwrap();
        let mut exe = File::open(&exe_path).unwrap();
        let meta = exe.metadata().unwrap();

        let msg = install_files(&mut exe, &meta, &p).unwrap();
        assert!(msg.contains(VERSION));
        assert_eq!(fs::read(&p.helper).unwrap(), b"binary-v1");
        assert_eq!(fs::read(&p.policy).unwrap(), POLICY.as_bytes());
        assert_eq!(fs::metadata(&p.helper).unwrap().mode() & 0o777, 0o755);
        assert_eq!(read_manifest_at(&p.manifest).as_deref(), Some(VERSION));

        // Same version again (repair/update) replaces the helper.
        fs::write(&exe_path, b"binary-v2").unwrap();
        let mut exe = File::open(&exe_path).unwrap();
        let meta = exe.metadata().unwrap();
        install_files(&mut exe, &meta, &p).unwrap();
        assert_eq!(fs::read(&p.helper).unwrap(), b"binary-v2");

        // A policy directory that cannot be written fails the update; the helper stays usable
        // and the recorded version does not advance.
        fs::write(&p.manifest, "version=0.0.1\n").unwrap();
        fs::remove_file(&p.policy).unwrap();
        fs::create_dir(&p.policy).unwrap(); // a directory where the policy file goes
        fs::write(&exe_path, b"binary-v3").unwrap();
        let mut exe = File::open(&exe_path).unwrap();
        let meta = exe.metadata().unwrap();
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert_eq!(read_manifest_at(&p.manifest).as_deref(), Some("0.0.1"));
        assert!(p.helper.is_file());
        assert!(
            State::SelfInstalled {
                version: "0.0.1".into()
            }
            .needs_update()
        );
    }

    #[test]
    fn uninstall_needs_our_record_and_removes_only_our_files() {
        let root = scratch("uninstall");
        let p = paths(&root);
        fs::create_dir_all(p.helper.parent().unwrap()).unwrap();
        fs::create_dir_all(p.policy.parent().unwrap()).unwrap();
        fs::write(&p.helper, b"h").unwrap();
        fs::write(&p.policy, b"p").unwrap();
        let sibling = p.helper.with_file_name("unrelated");
        fs::write(&sibling, b"u").unwrap();
        // No record: not ours.
        assert!(uninstall_files(&p).is_err());
        assert!(p.helper.exists());
        fs::create_dir_all(&p.state_dir).unwrap();
        fs::write(&p.manifest, manifest_text("0.1.0")).unwrap();
        uninstall_files(&p).unwrap();
        assert!(!p.helper.exists() && !p.policy.exists() && !p.manifest.exists());
        assert!(sibling.exists());
        // Idempotent once gone: no record left, so it reports "not ours" rather than failing hard.
        assert!(uninstall_files(&p).is_err());
    }

    #[test]
    fn running_installed_helper_cannot_reinstall_itself() {
        let root = scratch("self");
        let p = paths(&root);
        fs::create_dir_all(p.helper.parent().unwrap()).unwrap();
        fs::write(&p.helper, b"h").unwrap();
        let mut exe = File::open(&p.helper).unwrap();
        let meta = exe.metadata().unwrap();
        assert!(install_files(&mut exe, &meta, &p).is_err());
    }

    fn exe_file(root: &Path, content: &[u8]) -> (File, fs::Metadata) {
        let path = root.join("exe");
        fs::write(&path, content).unwrap();
        let f = File::open(&path).unwrap();
        let meta = f.metadata().unwrap();
        (f, meta)
    }

    #[test]
    fn interrupted_installs_recover_with_a_normal_retry() {
        // Disk states a crash can leave, in write order: record only; record + helper; helper and
        // policy but the record still on the previous version.
        let states: [(&str, bool, bool, &str); 3] = [
            ("crash-a", false, false, "0.0.0"),
            ("crash-b", true, false, "0.0.0"),
            ("crash-c", true, true, "0.0.1"),
        ];
        for (tag, helper, policy, recorded) in states {
            let root = scratch(tag);
            let p = paths(&root);
            for dir in [
                p.helper.parent().unwrap(),
                p.policy.parent().unwrap(),
                &p.state_dir,
            ] {
                fs::create_dir_all(dir).unwrap();
            }
            fs::write(&p.manifest, manifest_text(recorded)).unwrap();
            if helper {
                fs::write(&p.helper, b"half-updated").unwrap();
            }
            if policy {
                fs::write(&p.policy, b"old policy").unwrap();
            }
            fs::write(
                p.helper.with_file_name(".hidforge-helper.99999.tmp"),
                b"junk",
            )
            .unwrap();

            let (mut exe, meta) = exe_file(&root, b"good-binary");
            install_files(&mut exe, &meta, &p).unwrap_or_else(|e| panic!("{tag}: {e}"));
            assert_eq!(fs::read(&p.helper).unwrap(), b"good-binary", "{tag}");
            assert_eq!(fs::read(&p.policy).unwrap(), POLICY.as_bytes(), "{tag}");
            assert_eq!(
                read_manifest_at(&p.manifest).as_deref(),
                Some(VERSION),
                "{tag}"
            );
            assert!(
                !p.helper
                    .with_file_name(".hidforge-helper.99999.tmp")
                    .exists(),
                "{tag}"
            );
            // And the same state can be removed cleanly instead.
            uninstall_files(&p).unwrap();
            assert!(
                !p.helper.exists() && !p.policy.exists() && !p.manifest.exists(),
                "{tag}"
            );
        }
    }

    #[test]
    fn interrupted_state_without_a_record_is_refused_not_adopted() {
        // Files with no record cannot be told apart from a package's: left alone (manual cleanup).
        let root = scratch("norecord");
        let p = paths(&root);
        fs::create_dir_all(p.policy.parent().unwrap()).unwrap();
        fs::write(&p.policy, b"someone else's policy").unwrap();
        let (mut exe, meta) = exe_file(&root, b"x");
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert_eq!(fs::read(&p.policy).unwrap(), b"someone else's policy");
    }

    #[test]
    fn failed_first_install_rolls_back_completely() {
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores directory permissions; the container script covers this
        }
        let root = scratch("rollback");
        let p = paths(&root);
        let libexec = p.helper.parent().unwrap();
        fs::create_dir_all(libexec).unwrap();
        fs::set_permissions(libexec, fs::Permissions::from_mode(0o555)).unwrap();
        let (mut exe, meta) = exe_file(&root, b"x");
        let err = install_files(&mut exe, &meta, &p).unwrap_err();
        assert!(err.contains("hidforge-helper"), "{err}");
        assert!(!p.manifest.exists() && !p.state_dir.exists());
        assert!(!p.helper.exists() && !p.policy.exists());
        fs::set_permissions(libexec, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn refused_install_creates_no_directories() {
        let root = scratch("nodirs");
        let p = paths(&root);
        fs::create_dir_all(p.marker.parent().unwrap()).unwrap();
        fs::write(&p.marker, b"").unwrap();
        let (mut exe, meta) = exe_file(&root, b"x");
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert!(!p.state_dir.exists());
        assert!(!p.helper.parent().unwrap().exists());
        assert!(!p.policy.parent().unwrap().exists());
    }

    #[test]
    fn untrusted_directories_stop_install_and_uninstall_before_any_change() {
        let root = scratch("untrusted");
        let p = paths(&root);
        fs::create_dir_all(&p.state_dir).unwrap();
        fs::write(&p.manifest, manifest_text("0.0.1")).unwrap();
        fs::create_dir_all(p.helper.parent().unwrap()).unwrap();
        fs::write(&p.helper, b"keep").unwrap();
        // State dir writable by others: its record cannot be trusted.
        fs::set_permissions(&p.state_dir, fs::Permissions::from_mode(0o777)).unwrap();
        let (mut exe, meta) = exe_file(&root, b"x");
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert!(uninstall_files(&p).is_err());
        assert_eq!(fs::read(&p.helper).unwrap(), b"keep");
        assert!(p.manifest.exists());
    }

    #[test]
    fn package_files_survive_update_and_uninstall() {
        // A package owns the helper and ships the marker: neither path may touch its files,
        // even when an old record of ours is still lying around.
        let root = scratch("pkg");
        let p = paths(&root);
        for dir in [
            p.helper.parent().unwrap(),
            p.policy.parent().unwrap(),
            &p.state_dir,
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::create_dir_all(p.marker.parent().unwrap()).unwrap();
        fs::write(&p.marker, b"").unwrap();
        fs::write(&p.helper, b"pkg helper").unwrap();
        fs::write(&p.policy, b"pkg policy").unwrap();
        fs::write(&p.manifest, manifest_text("0.0.1")).unwrap();
        let (mut exe, meta) = exe_file(&root, b"x");
        assert!(install_files(&mut exe, &meta, &p).is_err());
        assert!(uninstall_files(&p).is_err());
        assert_eq!(fs::read(&p.helper).unwrap(), b"pkg helper");
        assert_eq!(fs::read(&p.policy).unwrap(), b"pkg policy");
    }

    #[test]
    fn read_only_file_system_gets_its_own_message() {
        let ro = io::Error::from_raw_os_error(libc::EROFS);
        let msg = io_err(Path::new("/usr/libexec/hidforge-helper"), &ro);
        assert_eq!(
            msg,
            tr!("err.setup_readonly", path = "/usr/libexec/hidforge-helper")
        );
        let other = io::Error::from_raw_os_error(libc::EACCES);
        assert!(io_err(Path::new("/x"), &other).starts_with("/x: "));
    }

    #[test]
    fn second_installer_is_refused_while_the_lock_is_held() {
        let root = scratch("lock");
        let p = paths(&root);
        let guard = acquire_lock(&p).unwrap();
        let (mut exe, meta) = exe_file(&root, b"x");
        assert_eq!(
            install_files(&mut exe, &meta, &p).unwrap_err(),
            tr!("err.setup_busy")
        );
        assert_eq!(uninstall_files(&p).unwrap_err(), tr!("err.setup_busy"));
        assert!(!p.helper.exists());
        drop(guard);
        // A child forked by a parallel test can briefly still hold a copy of the descriptor
        // (until its exec closes it), so the lock frees a moment after the drop.
        let mut result = install_files(&mut exe, &meta, &p);
        for _ in 0..100 {
            if result != Err(tr!("err.setup_busy")) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            result = install_files(&mut exe, &meta, &p);
        }
        result.unwrap();
    }

    #[test]
    fn parallel_installs_never_corrupt_or_delete_each_others_files() {
        let root = scratch("parallel");
        let p = std::sync::Arc::new(paths(&root));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let (p, barrier, root) = (p.clone(), barrier.clone(), root.clone());
                std::thread::spawn(move || {
                    let path = root.join(format!("exe{i}"));
                    fs::write(&path, format!("binary-{i}")).unwrap();
                    let mut exe = File::open(&path).unwrap();
                    let meta = exe.metadata().unwrap();
                    barrier.wait();
                    install_files(&mut exe, &meta, &p)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(results.iter().any(Result::is_ok), "someone must win");
        for e in results.iter().filter_map(|r| r.as_ref().err()) {
            assert_eq!(e, &tr!("err.setup_busy"), "only the lock may refuse");
        }
        let helper = fs::read_to_string(&p.helper).unwrap();
        assert!(helper.starts_with("binary-"), "{helper}");
        assert_eq!(fs::read(&p.policy).unwrap(), POLICY.as_bytes());
        assert_eq!(read_manifest_at(&p.manifest).as_deref(), Some(VERSION));
        let dir = p.helper.parent().unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    // Fake "who owns this file" tools: /bin/sh -c '<script>' sh <path>.
    static OWNED: Query = Query {
        tool: "fake",
        args: &["-c", "exit 0", "sh"],
        not_owned: "no package owns",
    };
    static NOT_OWNED: Query = Query {
        tool: "fake",
        args: &[
            "-c",
            "echo 'error: No package owns' \"$1\" >&2; exit 1",
            "sh",
        ],
        not_owned: "no package owns",
    };
    static DB_BROKEN: Query = Query {
        tool: "fake",
        args: &[
            "-c",
            "echo 'error: could not open database' >&2; exit 1",
            "sh",
        ],
        not_owned: "no package owns",
    };
    static CRASHED: Query = Query {
        tool: "fake",
        args: &["-c", "exit 2", "sh"],
        not_owned: "no package owns",
    };

    fn with_query(root: &Path, query: &'static Query) -> Paths {
        let mut p = paths(root);
        p.queries = vec![(PathBuf::from("/bin/sh"), query)];
        p
    }

    fn pkg_owned_state(p: &Paths) {
        for dir in [
            p.helper.parent().unwrap(),
            p.policy.parent().unwrap(),
            &p.state_dir,
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        // An old record of ours, and files a package put there later (no marker shipped).
        fs::write(&p.manifest, manifest_text("0.0.1")).unwrap();
        fs::write(&p.helper, b"pkg helper").unwrap();
        fs::write(&p.policy, b"pkg policy").unwrap();
    }

    #[test]
    fn owned_or_unknown_ownership_never_overwrites_or_deletes() {
        // "Owned" and every kind of failed query (database error, crash, tool cannot start) are
        // refused alike: fail closed.
        let missing: &'static Query = &Query {
            tool: "fake",
            args: &[],
            not_owned: "x",
        };
        for (tag, query, tool) in [
            ("owned", &OWNED, "/bin/sh"),
            ("broken", &DB_BROKEN, "/bin/sh"),
            ("crashed", &CRASHED, "/bin/sh"),
            ("nostart", missing, "/nonexistent/tool"),
        ] {
            let root = scratch(&format!("owner-{tag}"));
            let mut p = with_query(&root, query);
            p.queries[0].0 = PathBuf::from(tool);
            pkg_owned_state(&p);
            let (mut exe, meta) = exe_file(&root, b"x");
            let install = install_files(&mut exe, &meta, &p).unwrap_err();
            let uninstall = uninstall_files(&p).unwrap_err();
            let expected = if tag == "owned" {
                tr!("err.setup_packaged")
            } else {
                tr!("err.setup_owner_unknown", tool = "fake")
            };
            assert_eq!((install, uninstall), (expected.clone(), expected), "{tag}");
            assert_eq!(fs::read(&p.helper).unwrap(), b"pkg helper", "{tag}");
            assert_eq!(fs::read(&p.policy).unwrap(), b"pkg policy", "{tag}");
            assert!(p.manifest.exists(), "{tag}");
        }
    }

    #[test]
    fn a_definite_not_owned_answer_allows_update_and_removal() {
        let root = scratch("owner-free");
        let p = with_query(&root, &NOT_OWNED);
        pkg_owned_state(&p);
        let (mut exe, meta) = exe_file(&root, b"ours");
        install_files(&mut exe, &meta, &p).unwrap();
        assert_eq!(fs::read(&p.helper).unwrap(), b"ours");
        uninstall_files(&p).unwrap();
        assert!(!p.helper.exists());
    }

    #[test]
    fn a_path_that_does_not_exist_is_not_queried() {
        let root = scratch("owner-absent");
        // The tool would answer "owned", but there is nothing to own: a fresh install proceeds.
        let p = with_query(&root, &OWNED);
        assert!(check_not_package_owned(&p.helper, &p).is_ok());
        let (mut exe, meta) = exe_file(&root, b"x");
        install_files(&mut exe, &meta, &p).unwrap();
    }

    #[test]
    fn appimage_helper_is_staged_privately_and_cleaned_up() {
        let root = scratch("stage");
        let sibling = root.join("squashfs-root-hidforge-helper");
        fs::write(&sibling, b"helper bytes").unwrap();
        let runtime = root.join("run-user");
        fs::create_dir(&runtime).unwrap();

        let staged = stage_helper_from(&sibling, true, Some(runtime.clone())).unwrap();
        let dir = staged.dir.clone().unwrap();
        assert!(staged.path.starts_with(&dir) && dir.starts_with(&runtime));
        assert_eq!(fs::read(&staged.path).unwrap(), b"helper bytes");
        assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&staged.path).unwrap().mode() & 0o777, 0o700);
        staged.cleanup();
        assert!(!dir.exists());

        // No runtime dir (or a relative one) is an error, not a fallback to /tmp.
        assert!(stage_helper_from(&sibling, true, None).is_err());
        assert!(stage_helper_from(&sibling, true, Some("relative".into())).is_err());
        // A missing helper is reported by path.
        let err = stage_helper_from(&root.join("nope"), true, Some(runtime)).unwrap_err();
        assert!(err.contains("nope"), "{err}");
        // Outside an AppImage nothing is copied: the canonical sibling path is used.
        let direct = stage_helper_from(&sibling, false, None).unwrap();
        assert!(direct.dir.is_none() && direct.path == fs::canonicalize(&sibling).unwrap());
    }
}
