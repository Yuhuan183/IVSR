//! Localizable text supplied by engines and catalogs.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

pub const DEFAULT_LANG: &str = "en";

/// Text in several languages keyed by BCP 47 tag (`en`, `zh-TW`). English is
/// always present and is the fallback. Deserializes from a bare string too.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Text(BTreeMap<String, String>);

impl Text {
    pub fn en(text: impl Into<String>) -> Self {
        let mut map = BTreeMap::new();
        map.insert(DEFAULT_LANG.to_string(), text.into());
        Self(map)
    }

    pub fn with(mut self, lang: &str, text: impl Into<String>) -> Self {
        self.0.insert(lang.to_string(), text.into());
        self
    }

    /// Text for `lang`, falling back to its base language and then English.
    pub fn get(&self, lang: &str) -> &str {
        let base = lang.split(['-', '_']).next().unwrap_or(lang);
        self.0
            .get(lang)
            .or_else(|| self.0.get(base))
            .or_else(|| self.0.get(DEFAULT_LANG))
            .or_else(|| self.0.values().next())
            .map(String::as_str)
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.values().all(|v| v.is_empty())
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Text::en(text)
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Text::en(text)
    }
}

impl std::fmt::Display for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.get(DEFAULT_LANG))
    }
}

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Plain(String),
            Map(BTreeMap<String, String>),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Plain(s) => Text::en(s),
            Raw::Map(m) => Text(m),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_falls_back_to_base_language_then_english() {
        let t = Text::en("Fast").with("zh-TW", "快速");
        assert_eq!(t.get("zh-TW"), "快速");
        assert_eq!(t.get("fr"), "Fast");
        let base = Text::en("Fast").with("zh", "快");
        assert_eq!(base.get("zh-TW"), "快");
    }

    #[test]
    fn deserializes_from_string_or_map() {
        let plain: Text = serde_json::from_str(r#""Photo model""#).unwrap();
        assert_eq!(plain.get("zh-TW"), "Photo model");
        let map: Text = serde_json::from_str(r#"{"en": "Photo", "zh-TW": "照片"}"#).unwrap();
        assert_eq!(map.get("zh-TW"), "照片");
        assert_eq!(serde_json::to_string(&map).unwrap(), r#"{"en":"Photo","zh-TW":"照片"}"#);
    }
}
