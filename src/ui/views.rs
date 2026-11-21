use super::{App, GpuiApp, NextStep, Operation, Page, Tone, next_step};
use crate::{
    devices::{self, Device},
    hidbpf,
    requirements::{self, Requirement},
    rules::{self, AccessMode, InstalledRule},
};
use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{ActiveTheme, Icon, Sizable, Theme};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

const CANVAS: u32 = 0xf2f1eb;
const PAPER: u32 = 0xfffefa;
const INK: u32 = 0x182523;
const SUBTLE: u32 = 0x52615c;
const LINE: u32 = 0xd9ded6;
const RAIL: u32 = 0x182825;
const RAIL_HOVER: u32 = 0x273d37;
const RAIL_ACTIVE: u32 = 0x364d45;
const RAIL_INK: u32 = 0xf5f7ef;
const RAIL_MUTED: u32 = 0xb4c7bd;
const RAIL_WARN: u32 = 0xffc48e;
const ACTION: u32 = 0x205b4e;
const FOCUS: u32 = 0x548b76;
const WARNING: u32 = 0x864218;
const WARNING_BG: u32 = 0xfcebd9;
const SUCCESS: u32 = 0x235942;
const SUCCESS_BG: u32 = 0xe3ede2;
const ERROR: u32 = 0xa0312b;
const ERROR_BG: u32 = 0xfaeae5;
const INFO_BG: u32 = 0xe9ede8;
const COMPACT: f32 = 800.;
const RAIL_WIDTH: f32 = 272.;
const WORKSPACE_INSET: f32 = 28.;
const TAB_INSET: f32 = 10.;
const TAB_HOVER: u32 = 0xdfe6df;
const TAB_PRESSED: u32 = 0xd0dbd2;

fn color(value: u32) -> Hsla {
    rgb(value).into()
}

fn item_id(scope: &'static str, item: impl Into<SharedString>) -> ElementId {
    (ElementId::from(scope), item).into()
}

pub(super) fn configure_theme(cx: &mut GpuiApp) {
    let fira_available = cx
        .text_system()
        .all_font_names()
        .iter()
        .any(|name| name == "Fira Sans");
    Theme::update(cx, |theme| {
        let c = &mut theme.colors;
        c.background = color(PAPER);
        c.foreground = color(INK);
        c.border = color(LINE);
        c.input = color(LINE);
        c.muted = color(INFO_BG);
        c.muted_foreground = color(SUBTLE);
        c.accent = color(INFO_BG);
        c.accent_foreground = color(INK);
        c.primary = color(ACTION);
        c.primary_hover = color(0x184d42);
        c.primary_active = color(0x123c33);
        c.primary_foreground = color(PAPER);
        c.button_primary = c.primary;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = c.primary_foreground;
        c.button_danger = color(ERROR_BG);
        c.button_danger_hover = color(0xf7dcd4);
        c.button_danger_active = color(0xf2cfc6);
        c.button_danger_foreground = color(ERROR);
        c.ring = color(FOCUS);
        if fira_available {
            theme.font_family = "Fira Sans".into();
        }
        theme.radius = px(7.);
        theme.radius_lg = px(12.);
    });
}

fn icon(name: IconName, size: f32, tint: u32) -> Icon {
    Icon::new(name).size(px(size)).text_color(color(tint))
}

fn icon_tile(name: IconName, size: f32, tint: u32, fill: u32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(8.))
        .bg(color(fill))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name, if size >= 52. { 25. } else { 19. }, tint))
}

fn heading(text: impl Into<SharedString>, size: f32) -> Div {
    div()
        .text_size(px(size))
        .line_height(relative(1.2))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

fn body(text: impl Into<SharedString>) -> Div {
    div()
        .text_sm()
        .line_height(relative(1.45))
        .text_color(color(SUBTLE))
        .child(text.into())
}

fn shell() -> Div {
    div()
        .min_w_0()
        .v_flex()
        .rounded(px(12.))
        .bg(color(PAPER))
        .border_1()
        .border_color(color(LINE))
}

fn chip(text: impl Into<SharedString>, tone: Tone) -> Div {
    let (tint, fill) = tone_colors(tone);
    div()
        .flex_none()
        .px_2()
        .py_1()
        .rounded(px(5.))
        .bg(color(fill))
        .text_color(color(tint))
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
}

fn tone_colors(tone: Tone) -> (u32, u32) {
    match tone {
        Tone::Info => (SUBTLE, INFO_BG),
        Tone::Success => (SUCCESS, SUCCESS_BG),
        Tone::Warning => (WARNING, WARNING_BG),
        Tone::Error => (ERROR, ERROR_BG),
    }
}

fn device_icon(device: &Device) -> IconName {
    let name = device.display_name().to_ascii_lowercase();
    if name.contains("keyboard") || name.contains("klavye") {
        IconName::Keyboard
    } else if name.contains("mouse") || name.contains("fare") {
        IconName::Mouse
    } else if name.contains("headset") || name.contains("headphone") {
        IconName::Headphones
    } else {
        IconName::Plug
    }
}

fn rail_button(id: impl Into<ElementId>, selected: bool, cx: &GpuiApp) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(color(RAIL_INK))
                .color(if selected {
                    color(RAIL_ACTIVE)
                } else {
                    Hsla::default()
                })
                .hover(color(if selected { RAIL_ACTIVE } else { RAIL_HOVER }))
                .active(color(RAIL_ACTIVE)),
        )
        .rounded(px(8.))
        .w_full()
        .justify_start()
        .px_3()
        .cursor_pointer()
}

fn outline_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    glyph: IconName,
) -> Button {
    Button::new(id).outline().label(label).icon(glyph)
}

fn mono(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .min_w_0()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .line_height(relative(1.5))
        .child(text.into())
}

fn code_block(text: impl Into<SharedString>, theme: &Theme) -> Div {
    mono(text, theme).p_3().rounded(px(7.)).bg(color(INFO_BG))
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = window.viewport_size().width;
        let compact = width < px(COMPACT);
        let small = width < px(540.);
        let busy = self.operation.is_some();
        let main = match self.page {
            Page::Rules => self.render_rules(small, cx).into_any_element(),
            Page::Devices if compact && (!self.compact_detail || self.selected.is_none()) => {
                self.render_directory(true, cx).into_any_element()
            }
            Page::Devices => div()
                .h_flex()
                .items_stretch()
                .size_full()
                .min_h_0()
                .when(!compact, |el| el.child(self.render_directory(false, cx)))
                .child(self.render_detail(compact, small, cx))
                .into_any_element(),
        };
        div()
            .v_flex()
            .size_full()
            .bg(color(CANVAS))
            .text_color(color(INK))
            .child(self.render_toolbar(compact, small, busy, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(main),
            )
            .when(busy || self.notice.is_some(), |el| {
                el.child(self.render_feedback(cx))
            })
    }
}

impl App {
    fn render_toolbar(
        &self,
        compact: bool,
        small: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let tab = |page, label, glyph, cx: &mut Context<Self>| {
            let active = self.page == page;
            let button = Button::new(match page {
                Page::Devices => "nav-devices",
                Page::Rules => "nav-rules",
            })
            .label(label)
            .when(!small, |button| button.icon(glyph))
            .h(px(38.))
            .px(px(TAB_INSET))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .toggled(active)
            .custom(
                ButtonCustomVariant::new(cx)
                    .foreground(color(if active { RAIL_INK } else { INK }))
                    .hover(color(if active { RAIL_HOVER } else { TAB_HOVER }))
                    .active(color(if active { RAIL_ACTIVE } else { TAB_PRESSED })),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if page == Page::Devices && this.page == Page::Devices {
                    this.compact_detail = false;
                }
                this.page = page;
                this.pending_removal = None;
                cx.notify();
            }));
            div()
                .relative()
                .h_full()
                .h_flex()
                .child(
                    div()
                        .when(active, |el| el.rounded(cx.theme().radius).bg(color(RAIL)))
                        .child(button),
                )
                .when(active, |el| {
                    el.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(px(TAB_INSET))
                            .right(px(TAB_INSET))
                            .h(px(3.))
                            .bg(color(ACTION)),
                    )
                })
        };
        div()
            .h_flex()
            .flex_none()
            .when(compact, |el| el.gap_2())
            .pr(px(if small { 10. } else { WORKSPACE_INSET }))
            .h(px(56.))
            .bg(color(PAPER))
            .border_b_1()
            .border_color(color(LINE))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .flex_none()
                    .pl(px(if small { 10. } else { 20. }))
                    .when(!compact, |el| {
                        el.h_full()
                            .w(px(RAIL_WIDTH))
                            .border_r_1()
                            .border_color(color(LINE))
                    })
                    .child(img(super::LOGO_ICON).flex_none().size(px(if small {
                        28.
                    } else {
                        32.
                    })))
                    .child(heading("hidforge", if small { 16. } else { 20. })),
            )
            .child(
                div()
                    .h_flex()
                    .h_full()
                    .gap(if small { px(2.) } else { px(8.) })
                    .when(!compact, |el| el.pl(px(WORKSPACE_INSET - TAB_INSET)))
                    .child(tab(
                        Page::Devices,
                        tr!("nav.devices"),
                        IconName::LayoutGrid,
                        cx,
                    ))
                    .child(tab(
                        Page::Rules,
                        if small {
                            tr!("nav.rules_short")
                        } else {
                            tr!("nav.rules")
                        },
                        IconName::FileText,
                        cx,
                    )),
            )
            .child(div().flex_1())
            // Only offered once a second language file exists in `locales/`.
            .when(crate::locale::available().len() > 1, |el| {
                el.child(
                    Button::new("language")
                        .ghost()
                        .h(px(34.))
                        .mr_2()
                        .label(crate::locale::current().to_uppercase())
                        .accessibility_label(tr!("nav.language"))
                        .tooltip(tr!("nav.language"))
                        .on_click(cx.listener(|_, _, _, cx| {
                            let all = crate::locale::available();
                            let now = crate::locale::current();
                            let next = all
                                .iter()
                                .position(|l| *l == now)
                                .map_or(0, |i| (i + 1) % all.len());
                            crate::locale::set(&all[next]);
                            cx.notify();
                        })),
                )
            })
            .child(
                Button::new("refresh")
                    .outline()
                    .h(px(34.))
                    .when(small, |button| button.w(px(34.)))
                    .icon(IconName::RefreshCw)
                    .when(!small, |button| button.label(tr!("nav.refresh")))
                    .accessibility_label(tr!("nav.refresh_hint"))
                    .tooltip(tr!("nav.refresh_hint"))
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rescan(
                            |this| {
                                super::Notice::new(
                                    Tone::Info,
                                    tr!("scan.done"),
                                    tr!("scan.found", count = this.devices.len()),
                                )
                            },
                            cx,
                        );
                    })),
            )
    }

    fn render_directory(&self, compact: bool, cx: &mut Context<Self>) -> Div {
        let devices = self.filtered(cx);
        let (attention, others): (Vec<_>, Vec<_>) = devices
            .into_iter()
            .map(|device| (device, device.has_access()))
            .partition(|(device, access)| !*access || !device.wine_issues.is_empty());
        let list_empty = attention.is_empty() && others.is_empty();
        let group =
            |label, warning: bool, entries: Vec<(&Device, bool)>, cx: &mut Context<Self>| {
                let count = entries.len();
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .h_flex()
                            .px_2()
                            .py_1()
                            .gap_2()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(color(if warning { RAIL_WARN } else { RAIL_MUTED }))
                            .child(div().flex_1().child(label))
                            .child(count.to_string()),
                    )
                    .children(entries.into_iter().map(|(device, access)| {
                        self.render_device_row(device, access, compact, cx)
                    }))
            };

        div()
            .v_flex()
            .h_full()
            .min_h_0()
            .bg(color(RAIL))
            .text_color(color(RAIL_INK))
            .when(compact, |el| el.w_full())
            .when(!compact, |el| el.w(px(RAIL_WIDTH)).flex_none())
            .child(
                div()
                    .v_flex()
                    .p_4()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().flex_1().child(heading(tr!("rail.title"), 20.)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(color(RAIL_MUTED))
                                    .child(tr!("rail.connected", count = self.devices.len())),
                            ),
                    )
                    .child(
                        div()
                            .rounded(px(7.))
                            .bg(color(PAPER))
                            .text_color(color(INK))
                            .child(
                                Input::new(&self.search)
                                    .prefix(IconName::Search)
                                    .cleanable(true),
                            ),
                    ),
            )
            .child(
                div()
                    .id("device-directory")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .v_flex()
                    .px_3()
                    .pb_4()
                    .gap_4()
                    .when(!attention.is_empty(), |el| {
                        el.child(group(tr!("rail.attention"), true, attention, cx))
                    })
                    .when(!others.is_empty(), |el| {
                        el.child(group(tr!("rail.others"), false, others, cx))
                    })
                    .when(list_empty, |el| {
                        el.child(
                            div()
                                .v_flex()
                                .gap_2()
                                .p_2()
                                .child(heading(
                                    if self.devices.is_empty() {
                                        tr!("rail.empty")
                                    } else {
                                        tr!("rail.no_match")
                                    },
                                    16.,
                                ))
                                .child(div().text_sm().text_color(color(RAIL_MUTED)).child(
                                    if self.devices.is_empty() {
                                        tr!("rail.empty_hint")
                                    } else {
                                        tr!("rail.no_match_hint")
                                    },
                                )),
                        )
                    }),
            )
    }

    fn render_device_row(
        &self,
        device: &Device,
        access: bool,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let selected = !compact && self.selected.as_ref() == Some(&device.id);
        let id = device.id;
        let name = device.display_name();
        let summary = match (access, device.wine_issues.is_empty()) {
            (false, false) => tr!("rail.summary.both"),
            (false, true) => tr!("rail.summary.no_access"),
            (true, false) => tr!("rail.summary.wine"),
            (true, true) => tr!("rail.summary.ok"),
        };
        let warning = !access || !device.wine_issues.is_empty();
        rail_button(item_id("device", id.to_string()), selected, cx)
            .with_size(px(58.))
            .h(px(58.))
            .accessibility_label(format!("{name}. {summary}"))
            .tooltip(name.clone())
            .toggled(selected)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select(id);
                this.compact_detail = true;
                if compact {
                    this.back_focus.focus(window, cx);
                }
                cx.notify();
            }))
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(icon_tile(
                        device_icon(device),
                        31.,
                        RAIL_INK,
                        if selected { RAIL } else { RAIL_HOVER },
                    ))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).child(name))
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(color(if warning { RAIL_WARN } else { RAIL_MUTED }))
                                    .child(summary),
                            ),
                    )
                    .child(icon(
                        IconName::ChevronRight,
                        15.,
                        if selected { RAIL_INK } else { RAIL_MUTED },
                    )),
            )
    }
}

impl App {
    fn render_detail(
        &self,
        compact: bool,
        small: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(device) = self.selected_device() else {
            return div()
                .flex_1()
                .p_6()
                .child(empty_state(
                    IconName::Unplug,
                    tr!("workspace.empty_title"),
                    tr!("workspace.empty_body"),
                ))
                .into_any_element();
        };
        let access = device.has_access();
        let installed: Vec<_> = self.rules_for(&device.id).collect();
        let step = next_step(
            access,
            !installed.is_empty(),
            !device.wine_issues.is_empty(),
        );
        div()
            .id("device-workspace")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w(px(1000.))
                    .mx_auto()
                    .p(px(if small { 16. } else { WORKSPACE_INSET }))
                    .gap(if small { px(16.) } else { px(22.) })
                    .when(compact, |el| {
                        el.child(
                            div().h_flex().child(
                                Button::new("back-to-devices")
                                    .ghost()
                                    .icon(IconName::ArrowLeft)
                                    .label(tr!("nav.back_to_devices"))
                                    .track_focus(&self.back_focus)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.compact_detail = false;
                                        this.search.read(cx).focus_handle(cx).focus(window, cx);
                                        cx.notify();
                                    })),
                            ),
                        )
                    })
                    .child(
                        div()
                            .h_flex()
                            .gap_4()
                            .child(icon_tile(
                                device_icon(device),
                                if small { 46. } else { 60. },
                                RAIL_INK,
                                RAIL,
                            ))
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .flex_1()
                                    .child(heading(
                                        device.display_name(),
                                        if small { 24. } else { 32. },
                                    ))
                                    .child(body(format!(
                                        "{}  ·  {}",
                                        if device.is_hid() {
                                            tr!("device.kind_hid")
                                        } else {
                                            tr!("device.kind_usb")
                                        },
                                        device.id
                                    ))),
                            ),
                    )
                    .child(self.render_next_step(device, step, small, cx))
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .child(heading(tr!("device.controls"), 16.))
                            .child(self.render_checks(device, access, small)),
                    )
                    .child(self.render_device_rules(&installed, cx))
                    .child(
                        div().h_flex().child(
                            outline_button(
                                "toggle-advanced",
                                if self.advanced {
                                    tr!("device.advanced_hide")
                                } else {
                                    tr!("device.advanced_show")
                                },
                                if self.advanced {
                                    IconName::ChevronDown
                                } else {
                                    IconName::SlidersHorizontal
                                },
                            )
                            .toggled(self.advanced)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.advanced = !this.advanced;
                                cx.notify();
                            })),
                        ),
                    )
                    .when(self.advanced, |el| {
                        el.child(self.render_advanced(device, step, !installed.is_empty(), cx))
                    }),
            )
            .into_any_element()
    }

    fn apply_button(
        &self,
        label: impl Into<SharedString>,
        is_primary: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let applying = matches!(self.operation, Some(Operation::Apply(_)));
        Button::new(if is_primary {
            "apply-recommended"
        } else {
            "apply-advanced"
        })
        .label(label)
        .icon(IconName::ShieldCheck)
        .h(px(42.))
        .px_3()
        .when(is_primary, |button| button.primary())
        .when(!is_primary, |button| button.outline())
        .disabled(!self.opts.is_valid() || (self.operation.is_some() && !applying))
        .loading(applying)
        .on_click(cx.listener(|this, _, _, cx| this.apply(cx)))
    }

    fn render_next_step(
        &self,
        device: &Device,
        step: NextStep,
        small: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let blocked = !self.reqs.is_empty();
        let wine_saved = !device.wine_issues.is_empty()
            && device.wine_issues.iter().all(|(_, iface, _)| {
                iface.is_some_and(|number| {
                    self.bpf_installed.iter().any(|name| {
                        hidbpf::parse_object_name(name).is_some_and(|(_, iface)| iface == number)
                    })
                })
            });
        let (glyph, title, message) = match step {
            NextStep::GrantAccess => (
                IconName::LockKeyhole,
                tr!("next.grant.title"),
                tr!("next.grant.body"),
            ),
            NextStep::ReconnectForAccess => (
                IconName::Unplug,
                tr!("next.reconnect.title"),
                tr!("next.reconnect.body"),
            ),
            NextStep::FixWine if wine_saved => (
                IconName::Unplug,
                tr!("next.wine_pending.title"),
                tr!("next.wine_pending.body"),
            ),
            NextStep::FixWine if blocked => (
                IconName::TriangleAlert,
                tr!("next.wine_prereq.title"),
                tr!("next.wine_prereq.body"),
            ),
            NextStep::FixWine => (
                IconName::Wrench,
                tr!("next.wine_fix.title"),
                tr!("next.wine_fix.body"),
            ),
            NextStep::Complete if !self.bpf_installed.is_empty() => (
                IconName::CircleCheck,
                tr!("next.done.title"),
                tr!("next.done.body"),
            ),
            NextStep::Complete if !device.is_hid() => (
                IconName::CircleCheck,
                tr!("next.nothing.title"),
                tr!("next.nothing.body_usb"),
            ),
            NextStep::Complete => (
                IconName::CircleCheck,
                tr!("next.nothing.title"),
                tr!("next.nothing.body_clean"),
            ),
        };
        let (tint, fill) = if step == NextStep::Complete {
            (SUCCESS, SUCCESS_BG)
        } else {
            (WARNING, WARNING_BG)
        };
        let action = match step {
            NextStep::GrantAccess => Some(self.apply_button(tr!("next.grant.button"), true, cx)),
            NextStep::ReconnectForAccess | NextStep::FixWine
                if step != NextStep::FixWine || wine_saved =>
            {
                Some(
                    outline_button("recheck", tr!("next.recheck"), IconName::RefreshCw)
                        .disabled(self.operation.is_some())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rescan(
                                |_| {
                                    super::Notice::new(
                                        Tone::Info,
                                        tr!("next.rechecked.title"),
                                        tr!("next.rechecked.body"),
                                    )
                                },
                                cx,
                            );
                        })),
                )
            }
            NextStep::FixWine if !blocked && device.wine_issues.len() == 1 => {
                Some(self.wine_button(
                    &device.wine_issues[0].0,
                    tr!("next.wine_fix.button").into(),
                    true,
                    cx,
                ))
            }
            _ => None,
        };
        let title_content = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(color(tint))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if step == NextStep::Complete {
                        tr!("status.title")
                    } else {
                        tr!("status.next")
                    }),
            )
            .child(heading(title, if small { 20. } else { 24. }))
            .child(body(message));
        let (inline_action, stacked_action) = if small {
            (None, action)
        } else {
            (action, None)
        };
        let row = div()
            .h_flex()
            .items_start()
            .gap_3()
            .child(icon_tile(glyph, 38., tint, PAPER))
            .child(title_content)
            .children(inline_action);
        div()
            .v_flex()
            .gap_4()
            .p_4()
            .rounded(px(12.))
            .bg(color(fill))
            .child(row)
            .when_some(stacked_action, |el, action| {
                el.child(div().h_flex().child(action))
            })
            .when(
                step == NextStep::GrantAccess && !self.opts.is_valid(),
                |el| el.child(body(tr!("next.pick_access_type"))),
            )
            .when(step == NextStep::FixWine && !wine_saved && blocked, |el| {
                el.child(self.render_requirements(cx))
            })
            .when(
                step == NextStep::FixWine
                    && !wine_saved
                    && !blocked
                    && device.wine_issues.len() > 1,
                |el| {
                    el.child(div().flex().flex_wrap().gap_2().children(
                        device.wine_issues.iter().map(|(node, iface, _)| {
                            let label = iface.map_or_else(
                                || tr!("wine.fix_node", node = node.display()),
                                |iface| tr!("wine.fix_iface", iface = iface),
                            );
                            self.wine_button(node, label, true, cx)
                        }),
                    ))
                },
            )
    }

    fn wine_button(
        &self,
        node: &std::path::Path,
        label: String,
        primary: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let installing =
            matches!(&self.operation, Some(Operation::FixWine(active)) if active == node);
        let id = SharedString::from(node.display().to_string());
        let node = node.to_owned();
        Button::new(item_id(
            if primary {
                "wine-recommended"
            } else {
                "wine-advanced"
            },
            id,
        ))
        .label(label)
        .icon(IconName::Wrench)
        .h(px(42.))
        .px_3()
        .when(primary, |button| button.primary())
        .when(!primary, |button| button.outline())
        .loading(installing)
        .disabled(!self.reqs.is_empty() || (self.operation.is_some() && !installing))
        .on_click(cx.listener(move |this, _, _, cx| this.fix_wine(node.clone(), cx)))
    }

    fn render_checks(&self, device: &Device, access: bool, small: bool) -> Div {
        let (wine_status, wine_tone, wine_detail) = if !device.wine_issues.is_empty() {
            (
                tr!("wine.status.issue"),
                Tone::Warning,
                if self.reqs.is_empty() {
                    tr!("wine.detail.issue")
                } else {
                    tr!("wine.detail.missing_reqs")
                },
            )
        } else if !self.bpf_installed.is_empty() {
            (
                tr!("wine.status.installed"),
                Tone::Success,
                tr!("wine.detail.installed"),
            )
        } else if !device.is_hid() {
            (tr!("wine.status.na"), Tone::Info, tr!("wine.detail.na"))
        } else {
            (
                tr!("wine.status.clean"),
                Tone::Info,
                tr!("wine.detail.clean"),
            )
        };
        let check = |glyph, title, state, tone, detail| {
            let title = div()
                .h_flex()
                .min_w_0()
                .gap_2()
                .child(icon(glyph, 17., ACTION))
                .child(heading(title, 14.));
            if small {
                return shell().p_3().child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .items_center()
                        .child(title.flex_1())
                        .child(chip(state, tone)),
                );
            }
            shell()
                .flex_1()
                .p_3()
                .gap_2()
                .child(title)
                .child(div().h_flex().child(chip(state, tone)))
                .child(body(detail))
        };
        div()
            .flex()
            .gap_2()
            .when(small, |el| el.flex_col())
            .child(check(
                IconName::ShieldCheck,
                tr!("access.title"),
                if access {
                    tr!("access.yes")
                } else {
                    tr!("access.no")
                },
                if access { Tone::Success } else { Tone::Warning },
                if access {
                    tr!("access.detail_yes")
                } else {
                    tr!("access.detail_no")
                },
            ))
            .child(check(
                IconName::Activity,
                tr!("wine.title"),
                wine_status,
                wine_tone,
                wine_detail,
            ))
    }

    fn render_device_rules(&self, installed: &[&InstalledRule], cx: &mut Context<Self>) -> Div {
        let related = self.existing.iter().filter(|rule| !rule.ours).count();
        let summary = match installed.len() {
            0 => tr!("rules.count.zero").to_string(),
            1 => tr!("rules.count.one").to_string(),
            n => tr!("rules.count.other", n = n),
        };
        let open_rule = installed.first().map(|rule| rule.file_name.clone());
        shell()
            .p_3()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .items_start()
                    .gap_3()
                    .child(icon_tile(IconName::FileText, 34., ACTION, INFO_BG))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(heading(tr!("rules.installed_title"), 14.))
                            .child(body(summary))
                            .when(related > 0, |el| {
                                el.child(body(tr!("rules.related", count = related)))
                            }),
                    ),
            )
            .when_some(open_rule, |el, name| {
                el.child(
                    div().h_flex().child(
                        outline_button(
                            "open-device-rules",
                            tr!("rules.view"),
                            IconName::ArrowRight,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.page = Page::Rules;
                            this.expanded_rule = Some(name.clone());
                            this.pending_removal = None;
                            cx.notify();
                        })),
                    ),
                )
            })
    }

    fn render_requirements(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (distro, distro_name) = &self.distro;
        let command = distro.install_command(&self.reqs);
        let needs_upstream = !distro.packages_udev_hid_bpf()
            && self
                .reqs
                .iter()
                .any(|entry| entry.req == Requirement::UdevHidBpf);
        let refresh_hint = if command.is_some() {
            tr!("reqs.hint_command")
        } else {
            tr!("reqs.hint_manual")
        };
        shell()
            .p_3()
            .gap_2()
            .child(heading(tr!("reqs.title"), 15.))
            .children(self.reqs.iter().map(|entry| {
                div()
                    .v_flex()
                    .gap_1()
                    .child(heading(entry.req.title(), 13.))
                    .child(body(entry.detail.clone()))
            }))
            .child(body(refresh_hint))
            .when_some(command, |el, command| {
                el.child(body(tr!("reqs.install_command", distro = distro_name)))
                    .child(code_block(command, theme))
            })
            .when(needs_upstream, |el| {
                el.child(body(tr!("reqs.upstream_hint")))
                    .child(mono(requirements::UDEV_HID_BPF_UPSTREAM, theme))
            })
    }
}

impl App {
    fn render_advanced(
        &self,
        device: &Device,
        step: NextStep,
        installed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme().clone();
        let busy = self.operation.is_some();
        let mode = usize::from(self.opts.mode == AccessMode::InputGroup);
        let preview = rules::generate(device, &self.opts);
        let path = format!("{}/{}", rules::RULES_DIR, rules::file_name(&device.id));
        let permission_form = shell()
            .p_4()
            .gap_3()
            .child(heading(tr!("opts.title"), 17.))
            .child(body(tr!("opts.hint")))
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(heading(tr!("opts.type"), 14.))
                    .child(
                        Checkbox::new("access-hidraw")
                            .label(tr!("opts.hidraw"))
                            .checked(self.opts.hidraw)
                            .disabled(busy)
                            .on_click(cx.listener(|this, value: &bool, _, cx| {
                                this.opts.hidraw = *value;
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new("access-usb")
                            .label(tr!("opts.usb"))
                            .checked(self.opts.usb)
                            .disabled(busy)
                            .on_click(cx.listener(|this, value: &bool, _, cx| {
                                this.opts.usb = *value;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(heading(tr!("opts.scope"), 14.))
                    .child(
                        RadioGroup::vertical("access-scope")
                            .selected_index(Some(mode))
                            .disabled(busy)
                            .child(Radio::new("current-session").label(tr!("opts.scope_session")))
                            .child(Radio::new("input-group").label(tr!("opts.scope_group")))
                            .on_click(cx.listener(|this, index: &usize, _, cx| {
                                this.opts.mode = if *index == 0 {
                                    AccessMode::Uaccess
                                } else {
                                    AccessMode::InputGroup
                                };
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .border_t_1()
                    .border_color(color(LINE))
                    .pt_3()
                    .child(heading(tr!("preview.title"), 15.))
                    .child(body(tr!("preview.hint")))
                    .child(mono(path, &theme).text_color(color(SUBTLE)))
                    .child(code_block(preview.trim_end().to_string(), &theme))
                    .when(!self.opts.is_valid(), |el| {
                        el.child(body(tr!("preview.pick_type")).text_color(color(WARNING)))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(self.apply_button(
                                if installed {
                                    tr!("rule.update")
                                } else {
                                    tr!("rule.install")
                                },
                                false,
                                cx,
                            ))
                            .child(
                                outline_button(
                                    "export-rule",
                                    tr!("rule.export"),
                                    IconName::Download,
                                )
                                .h(px(42.))
                                .disabled(busy || !self.opts.is_valid())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.export(window, cx)),
                                ),
                            ),
                    ),
            );

        let nodes = device.hidraw_nodes.iter().chain(&device.usb_nodes);
        let diagnostics = shell()
            .p_4()
            .gap_3()
            .child(heading(tr!("info.title"), 17.))
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(body(format!("VID:PID {}", device.id)))
                    .when_some(device.serial.clone(), |el, serial| {
                        el.child(body(tr!("info.serial", serial = serial)))
                    }),
            )
            .children(nodes.map(|node| {
                let access = devices::can_read_write(node);
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .items_center()
                    .child(mono(node.display().to_string(), &theme))
                    .child(chip(
                        if access {
                            tr!("info.rw")
                        } else {
                            tr!("access.no")
                        },
                        if access { Tone::Success } else { Tone::Warning },
                    ))
            }))
            .when(self.existing.iter().any(|rule| !rule.ours), |el| {
                el.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(heading(tr!("info.other_rules"), 14.))
                        .children(
                            self.existing
                                .iter()
                                .filter(|rule| !rule.ours)
                                .map(|rule| mono(rule.path.display().to_string(), &theme)),
                        ),
                )
            });

        div()
            .v_flex()
            .gap_3()
            .child(permission_form)
            .child(diagnostics)
            .when(
                !device.wine_issues.is_empty() || !self.bpf_installed.is_empty(),
                |el| el.child(self.render_wine_diagnostics(device, step, cx)),
            )
    }

    fn render_wine_diagnostics(
        &self,
        device: &Device,
        step: NextStep,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme().clone();
        let mut panel = shell()
            .p_4()
            .gap_3()
            .child(heading(tr!("wine.diag_title"), 17.));
        if step != NextStep::FixWine && !device.wine_issues.is_empty() && !self.reqs.is_empty() {
            panel = panel.child(self.render_requirements(cx));
        }
        for (node, _, analysis) in &device.wine_issues {
            panel = panel.child(
                div()
                    .v_flex()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(color(LINE))
                    .child(mono(node.display().to_string(), &theme))
                    .children(analysis.issues.iter().map(|issue| {
                        body(tr!(
                            "wine.issue",
                            collection = issue.collection,
                            missing = issue.missing.join(", "),
                            reports = issue.reports.len()
                        ))
                    }))
                    .when(self.reqs.is_empty(), |el| {
                        el.child(div().h_flex().child(self.wine_button(
                            node,
                            tr!("wine.fix_this").into(),
                            false,
                            cx,
                        )))
                    }),
            );
        }
        for name in &self.bpf_installed {
            let remove_name = name.clone();
            panel = panel.child(
                div()
                    .v_flex()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(color(LINE))
                    .child(heading(tr!("wine.installed_title"), 14.))
                    .child(mono(format!("{}/{name}", hidbpf::BPF_DIR), &theme))
                    .child(
                        div().h_flex().child(
                            Button::new(item_id("remove-fix", name.clone()))
                                .danger()
                                .label(tr!("wine.remove_fix"))
                                .disabled(self.operation.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_fix(remove_name.clone(), cx)
                                })),
                        ),
                    ),
            );
        }
        panel
    }

    fn render_rules(&self, small: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let connected = self
            .installed
            .iter()
            .filter(|rule| self.connected_device(rule).is_some())
            .count();
        div()
            .id("rule-workspace")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .v_flex()
                    .max_w(px(980.))
                    .mx_auto()
                    .p(if small { px(16.) } else { px(32.) })
                    .gap_5()
                    .child(
                        div()
                            .h_flex()
                            .gap_4()
                            .child(icon_tile(
                                IconName::FileText,
                                if small { 46. } else { 60. },
                                RAIL_INK,
                                RAIL,
                            ))
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .flex_1()
                                    .child(heading(tr!("nav.rules"), if small { 24. } else { 34. }))
                                    .child(body(tr!("list.subtitle"))),
                            ),
                    )
                    .when(self.installed.is_empty(), |el| {
                        el.child(
                            empty_state(
                                IconName::FolderOpen,
                                tr!("list.empty_title"),
                                tr!("list.empty_body"),
                            )
                            .child(
                                div().h_flex().child(
                                    outline_button(
                                        "rules-back-to-devices",
                                        tr!("nav.back_to_devices"),
                                        IconName::ArrowLeft,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.page = Page::Devices;
                                            this.compact_detail = false;
                                            cx.notify();
                                        },
                                    )),
                                ),
                            ),
                        )
                    })
                    .when(!self.installed.is_empty(), |el| {
                        el.child(
                            shell()
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .gap_2()
                                        .items_center()
                                        .p_3()
                                        .child(
                                            div()
                                                .flex_1()
                                                .child(heading(tr!("list.panel_title"), 16.)),
                                        )
                                        .child(chip(
                                            tr!("list.count", count = self.installed.len()),
                                            Tone::Info,
                                        ))
                                        .child(chip(
                                            tr!("list.connected", count = connected),
                                            Tone::Info,
                                        )),
                                )
                                .children(
                                    self.installed
                                        .iter()
                                        .map(|rule| self.render_rule_row(rule, small, cx)),
                                ),
                        )
                    }),
            )
    }

    fn connected_device(&self, rule: &InstalledRule) -> Option<&Device> {
        let id = rule.id.as_ref()?;
        self.devices.iter().find(|device| &device.id == id)
    }

    fn render_rule_row(&self, rule: &InstalledRule, small: bool, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme().clone();
        let attached = self.connected_device(rule);
        let name = attached
            .map(Device::display_name)
            .or_else(|| saved_device_name(rule).map(str::to_owned))
            .unwrap_or_else(|| {
                rule.id.as_ref().map_or_else(
                    || tr!("device.unknown").into(),
                    |id| tr!("device.usb_id", id = id),
                )
            });
        let expanded = self.expanded_rule.as_deref() == Some(&rule.file_name);
        let confirming = self.pending_removal.as_deref() == Some(&rule.file_name);
        let toggle_name = rule.file_name.clone();
        let remove_name = rule.file_name.clone();
        let confirm_name = rule.file_name.clone();
        let id = SharedString::from(rule.file_name.clone());
        let toggle = outline_button(
            item_id("rule-details", id.clone()),
            if expanded {
                tr!("list.hide_rule")
            } else {
                tr!("list.show_rule")
            },
            if expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
        )
        .accessibility_label(tr!(
            "list.a11y_toggle",
            name = name,
            action = if expanded {
                tr!("list.a11y_hide")
            } else {
                tr!("list.a11y_show")
            }
        ))
        .toggled(expanded)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.expanded_rule = (!expanded).then(|| toggle_name.clone());
            this.pending_removal = None;
            cx.notify();
        }));
        let status = chip(
            if attached.is_some() {
                tr!("list.attached")
            } else {
                tr!("list.detached")
            },
            if attached.is_some() {
                Tone::Success
            } else {
                Tone::Info
            },
        );
        div()
            .v_flex()
            .gap_3()
            .p_4()
            .border_t_1()
            .border_color(color(LINE))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .items_start()
                    .when(small, |el| el.flex_col())
                    .child(
                        div()
                            .h_flex()
                            .gap_3()
                            .flex_1()
                            .min_w_0()
                            .child(icon_tile(IconName::FileText, 38., ACTION, INFO_BG))
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .min_w_0()
                                    .child(heading(name, 16.))
                                    .when_some(rule.id.as_ref(), |el, id| {
                                        el.child(body(format!("VID:PID {id}")))
                                    }),
                            ),
                    )
                    .child(div().h_flex().gap_2().child(status).child(toggle)),
            )
            .when(expanded, |el| {
                el.child(
                    div()
                        .v_flex()
                        .gap_2()
                        .p_3()
                        .rounded(px(7.))
                        .bg(color(INFO_BG))
                        .child(heading(tr!("list.file_title"), 14.))
                        .child(
                            mono(rule.path.display().to_string(), &theme).text_color(color(SUBTLE)),
                        )
                        .child(mono(rule.content.trim_end().to_string(), &theme)),
                )
                .when(!confirming, |el| {
                    el.child(
                        div().h_flex().child(
                            Button::new(item_id("rule-removal", id.clone()))
                                .danger()
                                .label(tr!("list.remove"))
                                .disabled(self.operation.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pending_removal = Some(remove_name.clone());
                                    cx.notify();
                                })),
                        ),
                    )
                })
                .when(confirming, |el| {
                    el.child(
                        div()
                            .v_flex()
                            .gap_2()
                            .p_3()
                            .rounded(px(7.))
                            .bg(color(ERROR_BG))
                            .child(heading(tr!("list.confirm_title"), 15.))
                            .child(body(tr!("list.confirm_body")))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new(item_id("cancel-removal", id.clone()))
                                            .outline()
                                            .label(tr!("list.cancel"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pending_removal = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new(item_id("confirm-removal", id))
                                            .danger()
                                            .label(tr!("list.remove"))
                                            .disabled(self.operation.is_some())
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.remove_rule(confirm_name.clone(), cx)
                                            })),
                                    ),
                            ),
                    )
                })
            })
    }

    fn render_feedback(&self, cx: &mut Context<Self>) -> Div {
        let (tone, title, message) = if let Some(operation) = &self.operation {
            (
                Tone::Info,
                operation.pending_title(),
                if *operation == Operation::Export {
                    tr!("busy.export")
                } else {
                    tr!("busy.admin")
                },
            )
        } else if let Some(notice) = &self.notice {
            (notice.tone, notice.title.clone(), notice.message.clone())
        } else {
            return div();
        };
        let (tint, fill) = tone_colors(tone);
        let glyph = match tone {
            Tone::Info => IconName::RefreshCw,
            Tone::Success => IconName::CircleCheck,
            Tone::Warning | Tone::Error => IconName::CircleAlert,
        };
        div()
            .h_flex()
            .flex_none()
            .items_start()
            .gap_3()
            .px_4()
            .py_3()
            .bg(color(fill))
            .border_t_1()
            .border_color(color(LINE))
            .child(icon(glyph, 20., tint))
            .child(
                div()
                    .id("feedback-text")
                    .min_w_0()
                    .flex_1()
                    .max_h(px(100.))
                    .overflow_y_scroll()
                    .v_flex()
                    .gap_1()
                    .child(heading(title, 15.).text_color(color(tint)))
                    .child(body(message)),
            )
            .when(self.operation.is_none(), |el| {
                el.child(
                    Button::new("dismiss-feedback")
                        .ghost()
                        .icon(IconName::X)
                        .accessibility_label(tr!("feedback.dismiss"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notice = None;
                            cx.notify();
                        })),
                )
            })
    }
}

fn empty_state(
    glyph: IconName,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
) -> Div {
    shell()
        .gap_3()
        .p_5()
        .child(icon_tile(glyph, 42., ACTION, INFO_BG))
        .child(heading(title, 20.))
        .child(body(description))
}

fn saved_device_name(rule: &InstalledRule) -> Option<&str> {
    let id = rule.id.as_ref()?;
    let comment = rule.content.lines().next()?;
    let name_and_id = comment
        .strip_prefix("# Generated by hidforge: ")
        .or_else(|| comment.strip_prefix("# Generated by driver-generator: "))?;
    name_and_id
        .strip_suffix(&format!(" ({id})"))
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::saved_device_name;
    use crate::{devices::UsbId, rules::InstalledRule};

    #[test]
    fn installed_rule_retains_device_name_when_disconnected() {
        let id = UsbId::new("1234", "5678").unwrap();
        for generator in ["hidforge", "driver-generator"] {
            let rule = InstalledRule {
                path: "/etc/udev/rules.d/example.rules".into(),
                file_name: "example.rules".into(),
                id: Some(id),
                content: format!("# Generated by {generator}: USB Mouse ({id})\n"),
                ours: true,
            };
            assert_eq!(saved_device_name(&rule), Some("USB Mouse"));
        }
    }
}
