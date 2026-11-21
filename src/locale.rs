//! Picks the UI language: saved choice, else `LC_ALL`/`LC_MESSAGES`/`LANG`, else the fallback.

use std::path::PathBuf;

fn config_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("hidforge/locale"))
}

/// `tr_TR.UTF-8` becomes `tr`.
fn language(raw: &str) -> &str {
    raw.split(['_', '.', '@', '-']).next().unwrap_or("")
}

/// Language codes with a file in `locales/`, sorted.
pub fn available() -> Vec<String> {
    let mut all: Vec<String> = rust_i18n::available_locales!()
        .into_iter()
        .map(str::to_string)
        .collect();
    all.sort();
    all
}

pub fn current() -> String {
    rust_i18n::locale().to_string()
}

pub fn init() {
    let saved = config_file().and_then(|p| std::fs::read_to_string(p).ok());
    let env = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
    let available = available();
    let choice = saved
        .as_deref()
        .map(str::trim)
        .into_iter()
        .chain(env.as_deref().map(language))
        .find(|l| available.iter().any(|a| a == l));
    if let Some(l) = choice {
        rust_i18n::set_locale(l);
    }
}

/// Switch now and remember the choice for the next start.
pub fn set(code: &str) {
    rust_i18n::set_locale(code);
    if let Some(path) = config_file() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, code);
    }
}

#[cfg(test)]
mod tests {
    use super::language;

    #[test]
    fn language_part() {
        assert_eq!(language("tr_TR.UTF-8"), "tr");
        assert_eq!(language("en"), "en");
        assert_eq!(language("C"), "C");
        assert_eq!(language("pt-BR"), "pt");
    }
}
