<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/hidforge-horizontal-reversed.svg">
    <img src="assets/brand/hidforge-horizontal.svg" alt="hidforge" width="320">
  </picture>
</p>

# hidforge

Make Windows-only keyboard and mouse software work on Linux.

Many peripherals ship their settings app (DPI, RGB, macros) for Windows only. You can run it
under Wine or Bottles, but it often either can't find the device or can't change its settings.
hidforge fixes both from a simple desktop app: it installs a udev rule so your user can access
the device, and loads a small HID-BPF fix into the kernel when Wine misreads the device's
descriptor.

<p align="center">
  <img src="assets/screenshot.png" alt="hidforge showing a device that needs access, with the next step highlighted" width="720">
</p>

## What it does

- **Lists your USB devices** and shows which ones need attention.
- **Gives the app access to the device.** Linux locks these devices to root by default; one
  click installs a rule that lets your user talk to it.
- **Fixes a Wine compatibility bug.** Some devices describe themselves in a way Wine
  misreads, so settings never reach the device. The app detects this and installs a small
  fix in the kernel, only for that device.
- **Tells you what's missing.** If a required package isn't installed, it shows the install
  command for your distribution.

Everything that needs root asks for your password through the system's usual prompt (polkit).

## How to use

1. Open hidforge and pick your device. Devices with an access or Wine issue appear first under
   **Needs attention**.
2. Follow **Next step**. If needed, grant access first, then install the Wine fix.
3. After a Wine fix, fully close Wine/Bottles, then open your vendor software again.
4. Open **Installed rules** to inspect or remove a rule this app installed. **Technical
   details** contains access options, the rule preview and export.

The interface is available in English and Turkish. It follows `LC_ALL`/`LC_MESSAGES`/`LANG`;
you can also switch it in the app (the choice is saved in `~/.config/hidforge/locale`).

## Command line

```sh
hidforge --check                          # list missing requirements and the install command for your distro (exit 1 if any)
hidforge --list                           # list detected USB devices
hidforge --bpf-source /dev/hidrawN        # print the generated HID-BPF fix for a device
hidforge --bpf-source-raw VID:PID FILE    # same, from a saved report descriptor file
```

## Requirements

- Linux with polkit
- For the Wine fix: `udev-hid-bpf`, `clang`, `libbpf` and kernel 6.11 or newer. Run
  `hidforge --check` to see what is missing.

Developed and tested on Arch-based distributions. Debian, Fedora and openSUSE should work
(install commands are provided) but are untested.

## Build

Needs Rust 1.88 or newer and the system libraries the GUI toolkit (gpui) links against. On
Debian/Ubuntu, CI installs:

```sh
sudo apt-get install libasound2-dev libfontconfig-dev libwayland-dev libx11-xcb-dev \
  libxkbcommon-x11-dev libssl-dev libzstd-dev libvulkan-dev
```

Other distributions need the equivalent packages.

```sh
cargo build --release
```

This builds two programs: `hidforge` (the app) and `hidforge-helper` (the small part that runs
as root, only for installing or removing rules and fixes). Install the helper and its polkit
policy so the password prompt names hidforge and only this helper can run:

```sh
sudo install -Dm755 target/release/hidforge-helper /usr/libexec/hidforge-helper
sudo install -Dm644 assets/org.hidforge.helper.policy /usr/share/polkit-1/actions/org.hidforge.helper.policy
install -Dm755 target/release/hidforge ~/.local/bin/hidforge
```

Release builds only use the installed helper. For development, a debug build (`cargo run`)
falls back to the `hidforge-helper` built next to it, with a generic password prompt. Reinstall
the helper after updating, since an installed one takes precedence.

Optional: show the app icon in your launcher and window title bar:

```sh
install -Dm644 assets/hidforge.desktop ~/.local/share/applications/hidforge.desktop
install -Dm644 assets/brand/icons/hidforge.svg ~/.local/share/icons/hicolor/scalable/apps/hidforge.svg
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 assets/brand/icons/hidforge-$s.png ~/.local/share/icons/hicolor/${s}x${s}/apps/hidforge.png
done
```

## Contributing

- **Translations:** copy `locales/en.yml` to `locales/<code>.yml` and translate the values. The
  language appears in the app automatically after a rebuild.
- **Before a PR:** run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` (the same checks CI runs).
- **Security:** the GUI never runs as root. Only `hidforge-helper` does, and CI fails if it
  links the GUI toolkit. Keep it small and validate all its input.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). The small HID-BPF programs hidforge generates and
loads into the kernel for your devices carry their own `GPL-2.0-only` header, as the kernel
requires.
