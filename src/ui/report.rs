//! "Report a problem": opens the GitHub issue form (`.github/ISSUE_TEMPLATE/bug_report.yml`)
//! with version, system and device fields prefilled. The user reviews and submits it in the
//! browser, so nothing is sent without them seeing it. Kept in English for the maintainers.

use super::App;
use crate::devices::Device;
use crate::requirements::Missing;

pub(super) const VERSION: &str = env!("CARGO_PKG_VERSION");
const NEW_ISSUE: &str = "https://github.com/Enes5519/hidforge/issues/new";

impl App {
    pub(super) fn issue_url(&self) -> String {
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        let system = system_info(
            &self.distro.1,
            kernel.trim(),
            &crate::locale::current(),
            &self.reqs,
        );
        let rules: Vec<&str> = self.existing.iter().map(|r| r.file_name.as_str()).collect();
        let device = self
            .selected_device()
            .map(|d| device_info(d, &rules, &self.bpf_installed));
        issue_url(&system, device.as_deref())
    }
}

fn system_info(distro: &str, kernel: &str, locale: &str, reqs: &[Missing]) -> String {
    let missing = if reqs.is_empty() {
        "none".to_string()
    } else {
        reqs.iter()
            .map(|m| format!("{:?}", m.req))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "Distribution: {distro}\nKernel: {kernel}\nUI language: {locale}\nMissing requirements: {missing}"
    )
}

/// Leaves out the serial number: it identifies the user's unit and doesn't help debugging.
fn device_info(device: &Device, rules: &[&str], bpf: &[String]) -> String {
    let list = |items: Vec<String>| {
        if items.is_empty() {
            "none".to_string()
        } else {
            items.join(", ")
        }
    };
    let mut out = format!(
        "Device: {}\nID: {}\nAccess: {}\nInstalled rules: {}\nInstalled HID-BPF fixes: {}\nWine issues:",
        device.display_name(),
        device.id,
        if device.has_access() { "yes" } else { "no" },
        list(rules.iter().map(|r| r.to_string()).collect()),
        list(bpf.to_vec()),
    );
    if device.wine_issues.is_empty() {
        out.push_str(" none");
    }
    for (node, _, analysis) in &device.wine_issues {
        for issue in &analysis.issues {
            out.push_str(&format!(
                "\n- {}: collection {} loses {} ({} reports)",
                node.display(),
                issue.collection,
                issue.missing.join(", "),
                issue.reports.len()
            ));
        }
    }
    out
}

fn issue_url(system: &str, device: Option<&str>) -> String {
    let mut params = vec![
        ("template", "bug_report.yml"),
        ("version", VERSION),
        ("system", system),
    ];
    if let Some(device) = device {
        params.push(("device", device));
    }
    url::Url::parse_with_params(NEW_ISSUE, params)
        .expect("NEW_ISSUE is a valid URL")
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::UsbId;

    #[test]
    fn builds_prefilled_issue_url_without_serial() {
        let device = Device {
            id: UsbId::new("046d", "c08b").unwrap(),
            vendor: "Logitech".into(),
            product: "G502 & Co".into(),
            serial: Some("SECRET123".into()),
            hidraw_nodes: Vec::new(),
            usb_nodes: Vec::new(),
            wine_issues: Vec::new(),
        };
        let info = device_info(&device, &["70-hidforge-046d-c08b.rules"], &[]);
        assert!(info.contains("ID: 046d:c08b"));
        assert!(info.contains("Wine issues: none"));
        assert!(!info.contains("SECRET123"));

        let url = issue_url(&system_info("Arch Linux", "6.12.1", "en", &[]), Some(&info));
        assert!(url.starts_with(
            "https://github.com/Enes5519/hidforge/issues/new?template=bug_report.yml&version="
        ));
        assert!(url.contains("Missing+requirements%3A+none"));
        assert!(url.contains("G502+%26+Co"));
        assert!(!url.contains("SECRET123"));
    }
}
