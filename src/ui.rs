mod views;

use crate::devices::{self, Device, UsbId};
use crate::hidbpf;
use crate::privileged;
use crate::requirements::{self, Distro, Missing};
use crate::rules::{self, AccessMode, InstalledRule, RuleOptions};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    App as GpuiApp, AppContext, Bounds, Context, Entity, FocusHandle, ScrollHandle, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowOptions, point, px, size,
};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) const LOGO_ICON: &str = "brand/hidforge-app-icon.svg";

/// Window icon for X11; Wayland takes it from the installed `hidforge.desktop`.
fn window_icon() -> Option<Arc<image::RgbaImage>> {
    let png = include_bytes!("../assets/brand/icons/hidforge-128.png");
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()
        .map(|icon| Arc::new(icon.into_rgba8()))
}

/// Icon catalog plus the embedded hidforge brand mark.
struct Assets;

impl gpui_kit::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        match path {
            LOGO_ICON => Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/brand/hidforge-mark-small-app-icon.svg"
            )))),
            _ => gpui_kit::assets::AllAssets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        gpui_kit::assets::AllAssets.list(path)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Devices,
    Rules,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NextStep {
    GrantAccess,
    ReconnectForAccess,
    FixWine,
    Complete,
}

fn next_step(access: bool, rule_installed: bool, wine_issue: bool) -> NextStep {
    match (access, rule_installed, wine_issue) {
        (false, false, _) => NextStep::GrantAccess,
        (false, true, _) => NextStep::ReconnectForAccess,
        (true, _, true) => NextStep::FixWine,
        (true, _, false) => NextStep::Complete,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Operation {
    Apply(UsbId),
    RemoveRule,
    FixWine(PathBuf),
    RemoveFix,
    Export,
}

impl Operation {
    fn pending_title(&self) -> String {
        match self {
            Self::Apply(_) => tr!("op.apply.pending"),
            Self::RemoveRule => tr!("op.remove_rule.pending"),
            Self::FixWine(_) => tr!("op.fix_wine.pending"),
            Self::RemoveFix => tr!("op.remove_fix.pending"),
            Self::Export => tr!("op.export.pending"),
        }
    }

    fn failure_title(&self) -> String {
        match self {
            Self::Apply(_) => tr!("op.apply.failed"),
            Self::RemoveRule => tr!("op.remove_rule.failed"),
            Self::FixWine(_) => tr!("op.fix_wine.failed"),
            Self::RemoveFix => tr!("op.remove_fix.failed"),
            Self::Export => tr!("op.export.failed"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

struct Notice {
    tone: Tone,
    title: String,
    message: String,
}

impl Notice {
    fn new(tone: Tone, title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            tone,
            title: title.into(),
            message: message.into(),
        }
    }
}

/// System state read from sysfs and the rules directories.
struct Scan {
    devices: Vec<Device>,
    installed: Vec<InstalledRule>,
    reqs: Vec<Missing>,
}

impl Scan {
    fn load() -> Self {
        Self {
            devices: devices::scan(),
            installed: rules::installed(),
            reqs: requirements::check(),
        }
    }
}

pub struct App {
    devices: Vec<Device>,
    installed: Vec<InstalledRule>,
    selected: Option<UsbId>,
    existing: Vec<InstalledRule>,
    bpf_installed: Vec<String>,
    reqs: Vec<Missing>,
    distro: (Distro, String),
    opts: RuleOptions,
    page: Page,
    compact_detail: bool,
    advanced: bool,
    expanded_rule: Option<String>,
    pending_removal: Option<String>,
    operation: Option<Operation>,
    scanning: bool,
    notice: Option<Notice>,
    search: Entity<InputState>,
    detail_scroll: ScrollHandle,
    back_focus: FocusHandle,
    _search_subscription: Subscription,
}

impl App {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr!("search.placeholder")));
        let subscription = cx.subscribe_in(&search, window, |_, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let mut app = Self {
            devices: Vec::new(),
            installed: Vec::new(),
            selected: None,
            existing: Vec::new(),
            bpf_installed: Vec::new(),
            reqs: Vec::new(),
            distro: Distro::detect(),
            opts: RuleOptions {
                hidraw: true,
                usb: false,
                mode: AccessMode::Uaccess,
            },
            page: Page::Devices,
            compact_detail: false,
            advanced: false,
            expanded_rule: None,
            pending_removal: None,
            operation: None,
            scanning: false,
            notice: None,
            search,
            detail_scroll: ScrollHandle::new(),
            back_focus: cx.focus_handle(),
            _search_subscription: subscription,
        };
        app.apply_scan(Scan::load());
        app
    }

    /// Rescan off the UI thread (sysfs walk + `gzip` of the kernel config), then show `notice`.
    fn rescan(&mut self, notice: fn(&Self) -> Notice, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.scanning, true) {
            return;
        }
        let task = cx.background_executor().spawn(async { Scan::load() });
        cx.spawn(async move |this, cx| {
            let scan = task.await;
            let _ = this.update(cx, |this, cx| {
                this.scanning = false;
                this.apply_scan(scan);
                this.notice = Some(notice(this));
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_scan(&mut self, scan: Scan) {
        self.devices = scan.devices;
        self.installed = scan.installed;
        self.reqs = scan.reqs;
        if let Some(id) = &self.selected {
            if self.devices.iter().any(|device| &device.id == id) {
                self.existing = rules::existing_for(id);
                self.bpf_installed = hidbpf::installed_for(id);
                return;
            }
            self.selected = None;
            self.compact_detail = false;
        }
        self.existing.clear();
        self.bpf_installed.clear();
        let suggested = self
            .devices
            .iter()
            .find(|device| !device.has_access() || !device.wine_issues.is_empty())
            .or_else(|| self.devices.first())
            .map(|device| device.id);
        if let Some(id) = suggested {
            self.select(id);
        }
    }

    fn selected_device(&self) -> Option<&Device> {
        self.devices
            .iter()
            .find(|device| Some(&device.id) == self.selected.as_ref())
    }

    fn select(&mut self, id: UsbId) {
        if self.selected.as_ref() == Some(&id) {
            return;
        }
        let Some(device) = self.devices.iter().find(|device| device.id == id) else {
            return;
        };
        self.opts = RuleOptions::default_for(device);
        self.existing = rules::existing_for(&id);
        self.bpf_installed = hidbpf::installed_for(&id);
        self.selected = Some(id);
        self.advanced = false;
        self.detail_scroll.set_offset(point(px(0.), px(0.)));
    }

    fn rules_for<'a>(&'a self, id: &'a UsbId) -> impl Iterator<Item = &'a InstalledRule> + 'a {
        self.installed
            .iter()
            .filter(move |rule| rule.id.as_ref() == Some(id))
    }

    fn filtered(&self, cx: &GpuiApp) -> Vec<&Device> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        self.devices
            .iter()
            .filter(|device| {
                query.is_empty()
                    || device.display_name().to_lowercase().contains(&query)
                    || device.id.to_string().contains(&query)
            })
            .collect()
    }

    fn run_job(
        &mut self,
        operation: Operation,
        job: impl FnOnce() -> Result<String, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.operation.is_some() {
            return;
        }
        self.notice = None;
        self.operation = Some(operation.clone());
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { (job(), Scan::load()) });
        cx.spawn(async move |this, cx| {
            let (result, scan) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.operation = None;
                this.apply_scan(scan);
                this.notice = Some(this.job_notice(operation, result));
                cx.notify();
            });
        })
        .detach();
    }

    fn job_notice(&self, operation: Operation, result: Result<String, String>) -> Notice {
        let message = match result {
            Ok(message) => message,
            Err(error) => return Notice::new(Tone::Error, operation.failure_title(), error),
        };
        match operation {
            Operation::Apply(id) if self.devices.iter().any(|d| d.id == id && d.has_access()) => {
                Notice::new(
                    Tone::Success,
                    tr!("notice.access_ready.title"),
                    tr!("notice.access_ready.body"),
                )
            }
            Operation::Apply(_) => Notice::new(
                Tone::Warning,
                tr!("notice.rule_saved.title"),
                tr!("notice.rule_saved.body"),
            ),
            Operation::RemoveRule => Notice::new(
                Tone::Success,
                tr!("notice.rule_removed.title"),
                tr!("notice.rule_removed.body"),
            ),
            // A failed load leaves the descriptor unfixed, so the node still shows the issue.
            Operation::FixWine(node)
                if self
                    .devices
                    .iter()
                    .any(|d| d.wine_issues.iter().any(|(p, _, _)| p == &node)) =>
            {
                Notice::new(
                    Tone::Warning,
                    tr!("notice.fix_saved.title"),
                    tr!("notice.fix_saved.body"),
                )
            }
            Operation::FixWine(_) => Notice::new(
                Tone::Success,
                tr!("notice.fix_installed.title"),
                tr!("notice.fix_installed.body"),
            ),
            Operation::RemoveFix => Notice::new(
                Tone::Success,
                tr!("notice.fix_removed.title"),
                tr!("notice.fix_removed.body"),
            ),
            Operation::Export => Notice::new(Tone::Success, tr!("notice.exported"), message),
        }
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.selected_device() else {
            return;
        };
        if !self.opts.is_valid() {
            self.notice = Some(Notice::new(
                Tone::Error,
                tr!("notice.not_applied"),
                tr!("notice.pick_type"),
            ));
            cx.notify();
            return;
        }
        let id = device.id;
        let name = rules::file_name(&id);
        let content = rules::generate(device, &self.opts);
        self.run_job(
            Operation::Apply(id),
            move || privileged::apply(&name, &content),
            cx,
        );
    }

    fn remove_rule(&mut self, name: String, cx: &mut Context<Self>) {
        self.pending_removal = None;
        self.run_job(Operation::RemoveRule, move || privileged::remove(&name), cx);
    }

    fn fix_wine(&mut self, node: PathBuf, cx: &mut Context<Self>) {
        self.run_job(
            Operation::FixWine(node.clone()),
            move || privileged::bpf_install(&node),
            cx,
        );
    }

    fn remove_fix(&mut self, name: String, cx: &mut Context<Self>) {
        self.run_job(
            Operation::RemoveFix,
            move || privileged::bpf_remove(&name),
            cx,
        );
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(device) = self.selected_device() else {
            return;
        };
        if self.operation.is_some() || !self.opts.is_valid() {
            return;
        }
        let name = rules::file_name(&device.id);
        let content = rules::generate(device, &self.opts);
        let dir = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/".into());
        let prompt = cx.prompt_for_new_path(&dir, Some(&name));
        self.notice = None;
        self.operation = Some(Operation::Export);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = match prompt.await {
                Ok(Ok(Some(path))) => Some(
                    rules::export(&path, &content)
                        .map(|()| tr!("notice.saved", path = path.display())),
                ),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => Some(Err(error.to_string())),
            };
            let _ = this.update(cx, |this, cx| {
                this.operation = None;
                this.notice = result.map(|result| this.job_notice(Operation::Export, result));
                cx.notify();
            });
        })
        .detach();
    }
}

pub fn run() {
    gpui_kit::application().with_assets(Assets).run(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Light, None, cx);
        views::configure_theme(cx);
        let bounds = Bounds::centered(None, size(px(1120.), px(760.)), cx);
        let opened = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(360.), px(440.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("hidforge".into()),
                    ..Default::default()
                }),
                app_id: Some("hidforge".into()),
                icon: window_icon(),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| App::new(window, cx)),
        );
        if let Err(e) = opened {
            eprintln!("{}", tr!("cli.window_failed", error = format!("{e:#}")));
            std::process::exit(1);
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_precedes_wine_and_existing_rule_prefers_reconnect() {
        assert_eq!(next_step(false, false, true), NextStep::GrantAccess);
        assert_eq!(next_step(false, true, true), NextStep::ReconnectForAccess);
        assert_eq!(next_step(true, false, true), NextStep::FixWine);
    }

    #[test]
    fn working_access_needs_no_rule_when_no_wine_issue_is_detected() {
        assert_eq!(next_step(true, false, false), NextStep::Complete);
        assert_eq!(next_step(true, true, false), NextStep::Complete);
    }
}
