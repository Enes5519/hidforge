fn main() {
    // rust-i18n embeds `locales/` at compile time; rebuild when a language file changes or is added.
    println!("cargo:rerun-if-changed=locales");
}
