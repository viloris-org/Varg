use std::collections::HashMap;

/// Supported locales.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    En,
    #[default]
    Zh,
    Ja,
    Ko,
    Es,
    ZhHant,
}

impl Locale {
    pub const VARIANTS: &'static [Locale] = &[
        Locale::En,
        Locale::Zh,
        Locale::Ja,
        Locale::Ko,
        Locale::Es,
        Locale::ZhHant,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Zh => "中文",
            Locale::Ja => "日本語",
            Locale::Ko => "한국어",
            Locale::Es => "Español",
            Locale::ZhHant => "繁體中文",
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
            Locale::Ja => include_str!("../locales/ja.toml"),
            Locale::Ko => include_str!("../locales/ko.toml"),
            Locale::Es => include_str!("../locales/es.toml"),
            Locale::ZhHant => include_str!("../locales/zh_hant.toml"),
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

    /// Export all translation key-value pairs for the current locale.
    pub fn entries(&self) -> Vec<(&str, &str)> {
        self.map.iter().map(|(&k, &v)| (k, v)).collect()
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
    use std::collections::BTreeSet;

    #[test]
    fn loads_both_locales() {
        let en = Translations::load(Locale::En);
        let zh = Translations::load(Locale::Zh);
        let ja = Translations::load(Locale::Ja);
        let ko = Translations::load(Locale::Ko);
        let es = Translations::load(Locale::Es);
        let zh_hant = Translations::load(Locale::ZhHant);

        assert_eq!(en.tr("app_name"), "Aster");
        assert_eq!(zh.tr("app_name"), "Aster");
        assert_eq!(ja.tr("app_name"), "Aster");
        assert_eq!(ko.tr("app_name"), "Aster");
        assert_eq!(es.tr("app_name"), "Aster");
        assert_eq!(zh_hant.tr("app_name"), "Aster");
        assert_eq!(en.tr("sidebar_projects"), "Projects");
        assert_eq!(zh.tr("sidebar_projects"), "项目");
        assert_eq!(ja.tr("sidebar_projects"), "プロジェクト");
        assert_eq!(ko.tr("sidebar_projects"), "프로젝트");
        assert_eq!(es.tr("sidebar_projects"), "Proyectos");
        assert_eq!(zh_hant.tr("sidebar_projects"), "專案");
    }

    #[test]
    fn format_replaces_placeholders() {
        let en = Translations::load(Locale::En);
        let result = en.tr_fmt("status_console_count", &["42"]);
        assert_eq!(result, "Console: 42");
    }

    #[test]
    fn locale_default_is_chinese() {
        assert_eq!(Locale::default(), Locale::Zh);
        assert_ne!(Locale::En, Locale::Zh);
    }

    #[test]
    fn locale_key_sets_match() {
        let base = Translations::load(Locale::En)
            .entries()
            .into_iter()
            .map(|(key, _)| key.to_owned())
            .collect::<BTreeSet<_>>();

        for locale in [Locale::En, Locale::Zh, Locale::Es] {
            let keys = Translations::load(locale)
                .entries()
                .into_iter()
                .map(|(key, _)| key.to_owned())
                .collect::<BTreeSet<_>>();
            assert_eq!(keys, base, "translation keys differ for {locale:?}");
        }
    }
}
