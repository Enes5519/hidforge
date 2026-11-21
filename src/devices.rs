//! USB / HID device discovery straight from sysfs.

use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const USB_DEVICES: &str = "/sys/bus/usb/devices";
const HIDRAW_CLASS: &str = "/sys/class/hidraw";

/// USB vendor/product pair. Formatted as lowercase 4-digit hex (`{:04x}`) wherever it ends up
/// in a file name or rule, so any value is safe to put in a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UsbId {
    pub vid: u16,
    pub pid: u16,
}

impl UsbId {
    /// Parse a vendor/product pair. Returns `None` unless both are exactly 4 hex digits.
    pub fn new(vid: &str, pid: &str) -> Option<Self> {
        let parse = |s: &str| {
            let s = s.trim();
            if s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
                u16::from_str_radix(s, 16).ok()
            } else {
                None
            }
        };
        Some(Self {
            vid: parse(vid)?,
            pid: parse(pid)?,
        })
    }
}

impl std::fmt::Display for UsbId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vid, self.pid)
    }
}

/// One physical device model (all instances / interfaces with the same VID:PID merged).
#[derive(Clone, Debug)]
pub struct Device {
    pub id: UsbId,
    pub vendor: String,
    pub product: String,
    pub serial: Option<String>,
    /// `/dev/hidrawN` nodes belonging to this device.
    pub hidraw_nodes: Vec<PathBuf>,
    /// `/dev/bus/usb/BBB/DDD` nodes belonging to this device.
    pub usb_nodes: Vec<PathBuf>,
    /// hidraw nodes whose report descriptor Wine mishandles (see `hid`), with their
    /// interface number.
    pub wine_issues: Vec<(PathBuf, Option<u8>, crate::hid::Analysis)>,
}

impl Device {
    pub fn display_name(&self) -> String {
        match (self.vendor.is_empty(), self.product.is_empty()) {
            (false, false) => format!("{} {}", self.vendor, self.product),
            (true, false) => self.product.clone(),
            (false, true) => self.vendor.clone(),
            (true, true) => tr!("device.unknown_id", id = self.id),
        }
    }

    pub fn is_hid(&self) -> bool {
        !self.hidraw_nodes.is_empty()
    }

    /// True when the current user can read+write every hidraw node (or the usb node if no hidraw).
    pub fn has_access(&self) -> bool {
        let nodes = if self.is_hid() {
            &self.hidraw_nodes
        } else {
            &self.usb_nodes
        };
        !nodes.is_empty() && nodes.iter().all(|n| can_read_write(n))
    }
}

fn read_attr(dir: &Path, name: &str) -> Option<String> {
    let s = fs::read_to_string(dir.join(name)).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Raw HID report descriptor of a `/dev/hidrawN` node, read from sysfs (world-readable).
pub fn report_descriptor(node: &Path) -> Option<Vec<u8>> {
    let name = node.file_name()?;
    fs::read(
        Path::new(HIDRAW_CLASS)
            .join(name)
            .join("device/report_descriptor"),
    )
    .ok()
}

/// Interface number (`bInterfaceNumber`) of a hidraw node, for display.
pub fn interface_number(node: &Path) -> Option<u8> {
    let name = node.file_name()?;
    let s = read_attr(
        &Path::new(HIDRAW_CLASS).join(name).join("device/.."),
        "bInterfaceNumber",
    )?;
    u8::from_str_radix(&s, 16).ok()
}

/// `/sys/devices/.../0003:VVVV:PPPP.NNNN` HID device behind a hidraw node.
pub fn hid_syspath(node: &Path) -> Option<PathBuf> {
    let name = node.file_name()?;
    fs::canonicalize(Path::new(HIDRAW_CLASS).join(name).join("device")).ok()
}

/// HID devices (any bus binding state) of a USB VID:PID on a given interface.
pub fn hid_syspaths_for(id: &UsbId, iface: u8) -> Vec<PathBuf> {
    let prefix = format!("0003:{:04X}:{:04X}.", id.vid, id.pid);
    let Ok(entries) = fs::read_dir("/sys/bus/hid/devices") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|e| fs::canonicalize(e.path()).ok())
        .filter(|p| {
            read_attr(&p.join(".."), "bInterfaceNumber")
                .and_then(|s| u8::from_str_radix(&s, 16).ok())
                == Some(iface)
        })
        .collect()
}

pub fn can_read_write(path: &Path) -> bool {
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated string that outlives the call.
    unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) == 0 }
}

/// Walk up from a sysfs path until a directory with `idVendor` (a USB device, not an interface).
fn usb_device_ancestor(start: &Path) -> Option<PathBuf> {
    let mut cur = fs::canonicalize(start).ok()?;
    loop {
        if cur.join("idVendor").is_file() && cur.join("idProduct").is_file() {
            return Some(cur);
        }
        if !cur.pop() || cur == Path::new("/sys") {
            return None;
        }
    }
}

/// Enumerate USB devices, skipping hubs. Devices sharing VID:PID are merged.
pub fn scan() -> Vec<Device> {
    let mut by_id: BTreeMap<UsbId, Device> = BTreeMap::new();
    let mut sysfs_to_id: BTreeMap<PathBuf, UsbId> = BTreeMap::new();

    if let Ok(entries) = fs::read_dir(USB_DEVICES) {
        for entry in entries.flatten() {
            let Ok(dir) = fs::canonicalize(entry.path()) else {
                continue;
            };
            let (Some(vid), Some(pid)) =
                (read_attr(&dir, "idVendor"), read_attr(&dir, "idProduct"))
            else {
                continue; // interface, not a device
            };
            if read_attr(&dir, "bDeviceClass").as_deref() == Some("09") {
                continue; // hub
            }
            let Some(id) = UsbId::new(&vid, &pid) else {
                continue;
            };

            let usb_node = match (read_attr(&dir, "busnum"), read_attr(&dir, "devnum")) {
                (Some(b), Some(d)) => match (b.parse::<u32>(), d.parse::<u32>()) {
                    (Ok(b), Ok(d)) => Some(PathBuf::from(format!("/dev/bus/usb/{b:03}/{d:03}"))),
                    _ => None,
                },
                _ => None,
            };

            let dev = by_id.entry(id).or_insert_with(|| Device {
                id,
                vendor: read_attr(&dir, "manufacturer").unwrap_or_default(),
                product: read_attr(&dir, "product").unwrap_or_default(),
                serial: read_attr(&dir, "serial"),
                hidraw_nodes: Vec::new(),
                usb_nodes: Vec::new(),
                wine_issues: Vec::new(),
            });
            dev.usb_nodes.extend(usb_node);
            sysfs_to_id.insert(dir, id);
        }
    }

    if let Ok(entries) = fs::read_dir(HIDRAW_CLASS) {
        for entry in entries.flatten() {
            let Some(usb_dir) = usb_device_ancestor(&entry.path().join("device")) else {
                continue; // bluetooth / i2c HID, not USB
            };
            if let Some(dev) = sysfs_to_id.get(&usb_dir).and_then(|id| by_id.get_mut(id)) {
                dev.hidraw_nodes
                    .push(PathBuf::from("/dev").join(entry.file_name()));
            }
        }
    }

    let mut devices: Vec<Device> = by_id.into_values().collect();
    for d in &mut devices {
        d.hidraw_nodes.sort_by_key(|p| natural_key(p));
        d.usb_nodes.sort();
        d.wine_issues = d
            .hidraw_nodes
            .iter()
            .filter_map(|n| {
                let a = crate::hid::analyze(&report_descriptor(n)?);
                a.needs_fix().then(|| (n.clone(), interface_number(n), a))
            })
            .collect();
    }
    // HID devices first (keyboards, mice), then by name.
    devices.sort_by_key(|d| (!d.is_hid(), d.display_name().to_lowercase()));
    devices
}

/// `/dev/hidraw10` sorts after `/dev/hidraw9`.
fn natural_key(p: &Path) -> (String, u32) {
    let s = p.to_string_lossy();
    let digits = s.len() - s.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (head, tail) = s.split_at(s.len() - digits);
    (head.to_string(), tail.parse().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_id_normalises() {
        let id = UsbId::new("1A2C", " 2002\n").unwrap();
        assert_eq!(id.to_string(), "1a2c:2002");
        assert!(UsbId::new("1a2", "2002").is_none());
        assert!(UsbId::new("zzzz", "2002").is_none());
    }

    #[test]
    fn hidraw_nodes_sort_numerically() {
        let mut v: Vec<PathBuf> = ["/dev/hidraw10", "/dev/hidraw8", "/dev/hidraw9"]
            .map(PathBuf::from)
            .into();
        v.sort_by_key(|p| natural_key(p));
        assert_eq!(
            v,
            ["/dev/hidraw8", "/dev/hidraw9", "/dev/hidraw10"].map(PathBuf::from)
        );
    }
}
