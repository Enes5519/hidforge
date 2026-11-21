use hidforge::{devices, hid, hidbpf, privileged, requirements, rules, selfinstall};

rust_i18n::i18n!("locales", fallback = "en");

/// Translated message as an owned `String`.
macro_rules! tr {
    ($($args:tt)*) => {
        rust_i18n::t!($($args)*).into_owned()
    };
}

mod locale;
mod ui;

fn main() {
    locale::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some("--check" | "--list" | "--bpf-source" | "--bpf-source-raw") =
        args.first().map(String::as_str)
    {
        // CLI modes: die quietly on a closed pipe (`hidforge --list | head`) like other tools,
        // instead of println! panicking. Not for the GUI, which writes to pkexec's stdin.
        // SAFETY: called before any thread starts; SIG_DFL is a valid disposition.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    }
    if args.first().map(String::as_str) == Some("--check") {
        let (distro, name) = requirements::Distro::detect();
        let missing = requirements::check();
        println!(
            "{}",
            tr!("cli.distro", name = name, family = format!("{distro:?}"))
        );
        if missing.is_empty() {
            println!("{}", tr!("cli.all_met"));
        }
        for m in &missing {
            println!(
                "{}\n    {}",
                tr!("cli.missing", title = m.req.title()),
                m.detail
            );
        }
        if let Some(cmd) = distro.install_command(&missing) {
            println!("{}", tr!("cli.install", cmd = cmd));
        }
        std::process::exit(if missing.is_empty() { 0 } else { 1 });
    }
    if args.first().map(String::as_str) == Some("--list") {
        for d in devices::scan() {
            let nodes: Vec<String> = d
                .hidraw_nodes
                .iter()
                .chain(&d.usb_nodes)
                .map(|n| n.display().to_string())
                .collect();
            println!(
                "{}  {:<40} {}  {}={}  {}",
                d.id,
                d.display_name(),
                if d.is_hid() { "HID" } else { "USB" },
                tr!("cli.access"),
                if d.has_access() {
                    tr!("cli.yes")
                } else {
                    tr!("cli.no")
                },
                nodes.join(" ")
            );
            for (node, _, a) in &d.wine_issues {
                for issue in &a.issues {
                    println!(
                        "    {}: {}",
                        node.display(),
                        tr!(
                            "cli.wine_issue",
                            collection = issue.collection,
                            missing = issue.missing.join(", "),
                            reports = issue.reports.len()
                        )
                    );
                }
            }
        }
        return;
    }
    // Debug helpers: print the HID-BPF fixup source for a hidraw node, or for a raw descriptor
    // file (`--bpf-source-raw VID:PID FILE`, useful when a fixup is already loaded).
    let source_for = |id: &devices::UsbId, desc: &[u8]| -> Option<Result<String, String>> {
        let a = hid::analyze(desc);
        a.needs_fix()
            .then(|| hidbpf::generate_source(id, desc, &hid::apply(desc, &a.inserts)))
    };
    let job = match args.as_slice() {
        [flag, node] if flag == "--bpf-source" => {
            let node = std::path::Path::new(node);
            devices::scan()
                .into_iter()
                .find(|d| d.hidraw_nodes.iter().any(|n| n == node))
                .zip(devices::report_descriptor(node))
                .map(|(d, desc)| (d.id, desc))
        }
        [flag, id, file] if flag == "--bpf-source-raw" => id
            .split_once(':')
            .and_then(|(v, p)| devices::UsbId::new(v, p))
            .zip(std::fs::read(file).ok()),
        _ => None,
    };
    if args.first().is_some_and(|a| a.starts_with("--bpf-source")) {
        match job.and_then(|(id, desc)| source_for(&id, &desc)) {
            Some(Ok(src)) => print!("{src}"),
            Some(Err(e)) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
            None => {
                eprintln!("{}", tr!("cli.no_device_or_fix"));
                std::process::exit(1);
            }
        }
        return;
    }
    // Without a display the GUI toolkit waits forever with no output.
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        eprintln!("{}", tr!("cli.no_display"));
        std::process::exit(1);
    }
    ui::run();
}
