//! What the HID-BPF fix needs on the host, and how to install it on common distributions.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Searched instead of `$PATH`, so the GUI and the pkexec helper (which gets a reset PATH)
/// agree on what is installed.
const SYSTEM_BIN_DIRS: [&str; 6] = [
    "/usr/bin",
    "/usr/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/bin",
    "/sbin",
];

/// struct_ops based HID-BPF (what our generated program uses) landed in Linux 6.11.
const MIN_KERNEL: (u32, u32) = (6, 11);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    UdevHidBpf,
    Clang,
    LibbpfHeaders,
    UapiHeaders,
    Kernel,
}

impl Requirement {
    pub fn title(self) -> String {
        match self {
            Requirement::UdevHidBpf => tr!("req.title.udev_hid_bpf"),
            Requirement::Clang => tr!("req.title.clang"),
            Requirement::LibbpfHeaders => tr!("req.title.libbpf"),
            Requirement::UapiHeaders => tr!("req.title.uapi"),
            Requirement::Kernel => tr!("req.title.kernel"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub req: Requirement,
    /// Why it is considered missing (shown under the title).
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distro {
    Arch,
    Debian,
    Fedora,
    Suse,
    Unknown,
}

impl Distro {
    pub fn detect() -> (Self, String) {
        let text = fs::read_to_string("/etc/os-release")
            .or_else(|_| fs::read_to_string("/usr/lib/os-release"))
            .unwrap_or_default();
        Self::from_os_release(&text)
    }

    /// Returns the family and a human-readable name (`PRETTY_NAME`, else `ID`).
    pub fn from_os_release(text: &str) -> (Self, String) {
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
                .unwrap_or_default()
        };
        let ids = format!("{} {}", field("ID"), field("ID_LIKE")).to_ascii_lowercase();
        let has = |w: &str| ids.split_whitespace().any(|x| x == w);
        let family = if has("arch") || has("cachyos") || has("manjaro") || has("endeavouros") {
            Distro::Arch
        } else if has("debian") || has("ubuntu") {
            Distro::Debian
        } else if has("fedora") || has("rhel") || has("centos") {
            Distro::Fedora
        } else if has("suse")
            || has("opensuse")
            || ids.split_whitespace().any(|x| x.starts_with("opensuse"))
        {
            Distro::Suse
        } else {
            Distro::Unknown
        };
        let pretty = field("PRETTY_NAME");
        (
            family,
            if pretty.is_empty() {
                field("ID")
            } else {
                pretty
            },
        )
    }

    /// Package providing a requirement, or `None` when no package can fix it.
    fn package(self, req: Requirement) -> Option<&'static str> {
        use Requirement::*;
        Some(match (self, req) {
            (_, Kernel) | (Distro::Unknown, _) => return None,
            (_, UdevHidBpf) => "udev-hid-bpf",
            (_, Clang) => "clang",
            (Distro::Arch, LibbpfHeaders) => "libbpf",
            (Distro::Debian, LibbpfHeaders) => "libbpf-dev",
            (Distro::Fedora | Distro::Suse, LibbpfHeaders) => "libbpf-devel",
            (Distro::Arch, UapiHeaders) => "linux-api-headers",
            (Distro::Debian, UapiHeaders) => "linux-libc-dev",
            (Distro::Fedora, UapiHeaders) => "kernel-headers",
            (Distro::Suse, UapiHeaders) => "linux-glibc-devel",
        })
    }

    fn install_prefix(self) -> Option<&'static str> {
        Some(match self {
            Distro::Arch => "sudo pacman -S --needed",
            Distro::Debian => "sudo apt install",
            Distro::Fedora => "sudo dnf install",
            Distro::Suse => "sudo zypper install",
            Distro::Unknown => return None,
        })
    }

    /// One install command for everything installable in `missing`, if the distro is known.
    pub fn install_command(self, missing: &[Missing]) -> Option<String> {
        let mut pkgs: Vec<&str> = missing.iter().filter_map(|m| self.package(m.req)).collect();
        pkgs.dedup();
        if pkgs.is_empty() {
            return None;
        }
        Some(format!("{} {}", self.install_prefix()?, pkgs.join(" ")))
    }

    /// Whether udev-hid-bpf is known to be packaged for this family. Elsewhere we point to
    /// upstream in addition to the package name.
    pub fn packages_udev_hid_bpf(self) -> bool {
        self == Distro::Arch
    }
}

pub const UDEV_HID_BPF_UPSTREAM: &str = "https://gitlab.freedesktop.org/libevdev/udev-hid-bpf";

/// Absolute path of a tool in the system bin directories.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    SYSTEM_BIN_DIRS
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

/// Extra `-I` directory for multiarch distributions (Debian/Ubuntu keep `asm/` there, and
/// clang does not search it for `-target bpf`).
pub fn multiarch_include() -> Option<PathBuf> {
    let p = PathBuf::from(format!("/usr/include/{}-linux-gnu", std::env::consts::ARCH));
    p.join("asm").is_dir().then_some(p)
}

fn kernel_version(release: &str) -> Option<(u32, u32)> {
    let mut it = release.split(|c: char| !c.is_ascii_digit());
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// `Some(true/false)` from the kernel config if it can be read, `None` when unknown.
fn kernel_config_has(option: &str) -> Option<bool> {
    let release = fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    let config = Command::new("gzip")
        .args(["-dc", "/proc/config.gz"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .or_else(|| fs::read_to_string(format!("/boot/config-{}", release.trim())).ok())?;
    let want = format!("{option}=y");
    Some(config.lines().any(|l| l.trim() == want))
}

fn check_kernel() -> Option<String> {
    let release = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let release = release.trim();
    if let Some(v) = kernel_version(release)
        && v < MIN_KERNEL
    {
        return Some(tr!(
            "req.kernel.too_old",
            release = release,
            major = MIN_KERNEL.0,
            minor = MIN_KERNEL.1
        ));
    }
    if !Path::new("/sys/kernel/btf/vmlinux").exists() {
        return Some(tr!("req.kernel.no_btf"));
    }
    if kernel_config_has("CONFIG_HID_BPF") == Some(false) {
        return Some(tr!("req.kernel.no_hid_bpf"));
    }
    None
}

/// Everything missing for the HID-BPF fix. Empty means "good to go".
pub fn check() -> Vec<Missing> {
    let mut out = Vec::new();
    let mut need = |req: Requirement, detail: String| out.push(Missing { req, detail });
    if find_tool("udev-hid-bpf").is_none() {
        need(Requirement::UdevHidBpf, tr!("req.missing.udev_hid_bpf"));
    }
    if find_tool("clang").is_none() {
        need(Requirement::Clang, tr!("req.missing.clang"));
    }
    if !["bpf_helpers.h", "bpf_tracing.h"]
        .iter()
        .all(|h| Path::new("/usr/include/bpf").join(h).is_file())
    {
        need(Requirement::LibbpfHeaders, tr!("req.missing.libbpf"));
    }
    if !Path::new("/usr/include/linux/types.h").is_file() {
        need(Requirement::UapiHeaders, tr!("req.missing.uapi"));
    }
    if let Some(why) = check_kernel() {
        need(Requirement::Kernel, why);
    }
    out
}

/// Short summary for errors raised by the root helper.
pub fn describe(missing: &[Missing]) -> String {
    let names: Vec<String> = missing.iter().map(|m| m.req.title()).collect();
    tr!("req.describe", names = names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(req: Requirement) -> Missing {
        Missing {
            req,
            detail: String::new(),
        }
    }

    #[test]
    fn detects_distro_families() {
        let cases = [
            (
                "ID=cachyos\nID_LIKE=arch\nPRETTY_NAME=\"CachyOS Linux\"",
                Distro::Arch,
                "CachyOS Linux",
            ),
            ("ID=arch", Distro::Arch, "arch"),
            ("ID=ubuntu\nID_LIKE=debian", Distro::Debian, "ubuntu"),
            (
                "ID=linuxmint\nID_LIKE=\"ubuntu debian\"",
                Distro::Debian,
                "linuxmint",
            ),
            ("ID=fedora", Distro::Fedora, "fedora"),
            (
                "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"",
                Distro::Suse,
                "opensuse-tumbleweed",
            ),
            ("ID=nixos", Distro::Unknown, "nixos"),
            ("", Distro::Unknown, ""),
        ];
        for (text, family, name) in cases {
            assert_eq!(
                Distro::from_os_release(text),
                (family, name.to_string()),
                "{text}"
            );
        }
    }

    #[test]
    fn install_commands_per_family() {
        let all = [
            Requirement::UdevHidBpf,
            Requirement::Clang,
            Requirement::LibbpfHeaders,
            Requirement::UapiHeaders,
        ]
        .map(m);
        assert_eq!(
            Distro::Arch.install_command(&all).unwrap(),
            "sudo pacman -S --needed udev-hid-bpf clang libbpf linux-api-headers"
        );
        assert_eq!(
            Distro::Debian.install_command(&all).unwrap(),
            "sudo apt install udev-hid-bpf clang libbpf-dev linux-libc-dev"
        );
        assert_eq!(
            Distro::Fedora
                .install_command(&[m(Requirement::LibbpfHeaders)])
                .unwrap(),
            "sudo dnf install libbpf-devel"
        );
        // Unknown distro and kernel-only problems have no package to suggest.
        assert!(Distro::Unknown.install_command(&all).is_none());
        assert!(
            Distro::Arch
                .install_command(&[m(Requirement::Kernel)])
                .is_none()
        );
    }

    #[test]
    fn parses_kernel_versions() {
        assert_eq!(kernel_version("7.3.4-1-cachyos"), Some((7, 3)));
        assert_eq!(kernel_version("6.11.0"), Some((6, 11)));
        assert!(kernel_version("6.10.2-arch1") < Some(MIN_KERNEL));
        assert_eq!(kernel_version("garbage"), None);
    }
}
