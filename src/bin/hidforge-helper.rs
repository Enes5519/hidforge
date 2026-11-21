//! Root helper, started by the GUI through `pkexec`. See `hidforge::privileged`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(hidforge::privileged::helper_main(&args));
}
