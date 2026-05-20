use std::collections::HashMap;

/// Supported locales.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    #[default]
    En,
    Zh,
}

impl Locale {
    pub const VARIANTS: &'static [Locale] = &[Locale::En, Locale::Zh];

    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Zh => "中文",
        }
    }
}

/// Compiled translation table.
#[derive(Clone, Debug)]
pub struct Translations {
    locale: Locale,
    map: HashMap<&'static str, &'static str>,
}

fn parse_toml_map(input: &str) -> HashMap<&str, &str> {
    // Minimal TOML key = "value" parser. Only handles flat string values.
    let mut map = HashMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        // Strip surrounding quotes from value
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        map.insert(key, value);
    }
    map
}

impl Translations {
    /// Load translations for the given locale from embedded TOML.
    pub fn load(locale: Locale) -> Self {
        let raw = match locale {
            Locale::En => include_str!("../locales/en.toml"),
            Locale::Zh => include_str!("../locales/zh.toml"),
        };
        let map = parse_toml_map(raw);
        Translations { locale, map }
    }

    /// Returns the current locale.
    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// Look up a translation key. Panics on missing keys (development aid).
    pub fn tr(&self, key: &str) -> &str {
        self.map
            .get(key)
            .copied()
            .unwrap_or_else(|| panic!("missing i18n key `{key}` for locale {:?}", self.locale))
    }

    /// Look up a translation key and format with positional arguments (`{}`).
    pub fn tr_fmt(&self, key: &str, args: &[&str]) -> String {
        let template = self.tr(key);
        let mut result = String::with_capacity(template.len());
        let mut rest = template;
        let mut arg_idx = 0;
        while let Some(pos) = rest.find("{}") {
            result.push_str(&rest[..pos]);
            if let Some(arg) = args.get(arg_idx) {
                result.push_str(arg);
                arg_idx += 1;
            }
            rest = &rest[pos + 2..];
        }
        result.push_str(rest);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_both_locales() {
        let en = Translations::load(Locale::En);
        let zh = Translations::load(Locale::Zh);

        assert_eq!(en.tr("app_name"), "Aster");
        assert_eq!(zh.tr("app_name"), "Aster");
        assert_eq!(en.tr("sidebar_projects"), "Projects");
        assert_eq!(zh.tr("sidebar_projects"), "项目");
    }

    #[test]
    fn format_replaces_placeholders() {
        let en = Translations::load(Locale::En);
        let result = en.tr_fmt("status_console_count", &["42"]);
        assert_eq!(result, "Console: 42");
    }

    #[test]
    fn locale_default_is_english() {
        assert_eq!(Locale::default(), Locale::En);
        assert_ne!(Locale::En, Locale::Zh);
    }
}
