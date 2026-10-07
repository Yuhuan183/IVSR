//! Choosing the interface language.

use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-TW")]
    ZhTw,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::ZhTw];

    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::ZhTw => "zh-TW",
        }
    }

    /// Maps a locale string (`zh_TW.UTF-8`, `zh-Hant-TW`, `en-US`) to a
    /// supported language. Traditional Chinese locales map to `zh-TW`;
    /// everything else, including Simplified Chinese, has no translation yet.
    pub fn from_locale(locale: &str) -> Option<Lang> {
        let tag = locale.split(['.', '@']).next().unwrap_or_default().replace('_', "-").to_ascii_lowercase();
        let parts: Vec<&str> = tag.split('-').collect();
        match parts.as_slice() {
            ["en", ..] => Some(Lang::En),
            ["zh", rest @ ..] => {
                let traditional = rest.iter().any(|p| matches!(*p, "hant" | "tw" | "hk" | "mo"));
                let simplified = rest.iter().any(|p| matches!(*p, "hans" | "cn" | "sg"));
                (traditional && !simplified).then_some(Lang::ZhTw)
            }
            _ => None,
        }
    }
}

/// `IVSR_LANG`, then `ui.language`, then the OS locale; English otherwise.
pub fn resolve(config: &Config) -> Lang {
    resolve_from(std::env::var("IVSR_LANG").ok().as_deref(), &config.ui.language, sys_locale::get_locales())
}

fn resolve_from(env: Option<&str>, configured: &str, system: impl Iterator<Item = String>) -> Lang {
    let explicit = |v: &str| (!v.is_empty() && v != "auto").then(|| Lang::from_locale(v)).flatten();
    if let Some(lang) = env.and_then(explicit).or_else(|| explicit(configured)) {
        return lang;
    }
    // The first preferred language decides; later ones are fallbacks the OS
    // would also try for apps lacking the first.
    system.filter_map(|l| Lang::from_locale(&l)).next().unwrap_or(Lang::En)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_map_to_supported_languages() {
        for (locale, lang) in [
            ("zh_TW.UTF-8", Some(Lang::ZhTw)),
            ("zh-Hant-TW", Some(Lang::ZhTw)),
            ("zh-HK", Some(Lang::ZhTw)),
            ("zh-Hant", Some(Lang::ZhTw)),
            ("zh-CN", None),
            ("zh-Hans-TW", None),
            ("en_US.UTF-8", Some(Lang::En)),
            ("ja-JP", None),
            ("C", None),
        ] {
            assert_eq!(Lang::from_locale(locale), lang, "{locale}");
        }
    }

    #[test]
    fn env_beats_config_beats_system() {
        let sys = || vec!["ja-JP".to_string(), "zh-Hant-TW".to_string()].into_iter();
        assert_eq!(resolve_from(Some("en"), "zh-TW", sys()), Lang::En);
        assert_eq!(resolve_from(None, "zh-TW", std::iter::empty()), Lang::ZhTw);
        assert_eq!(resolve_from(Some("auto"), "auto", sys()), Lang::ZhTw);
        assert_eq!(resolve_from(None, "auto", vec!["fr-FR".to_string()].into_iter()), Lang::En);
    }
}
