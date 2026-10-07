//! Self-describing engine parameters.
//!
//! Engines publish a schema (`ParamSpec`); frontends render it (CLI `-p key=value`,
//! GUI dynamic forms) and the core validates user input against it. Adding an
//! engine therefore never requires frontend changes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, Text};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl ParamValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ParamValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ParamValue::Float(v) => Some(*v),
            ParamValue::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ParamValue::Text(v) => Some(v),
            _ => None,
        }
    }
}

impl std::fmt::Display for ParamValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamValue::Bool(v) => write!(f, "{v}"),
            ParamValue::Int(v) => write!(f, "{v}"),
            ParamValue::Float(v) => write!(f, "{v}"),
            ParamValue::Text(v) => f.write_str(v),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumOption {
    pub value: String,
    pub label: Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamKind {
    Bool,
    Int { min: Option<i64>, max: Option<i64> },
    Float { min: Option<f64>, max: Option<f64> },
    Enum { options: Vec<EnumOption> },
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub key: String,
    pub label: Text,
    pub description: Text,
    pub kind: ParamKind,
    pub default: ParamValue,
    /// Hidden behind an "advanced" toggle in the GUI.
    #[serde(default)]
    pub advanced: bool,
}

/// Validated parameter values with defaults filled in for every spec.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamValues(BTreeMap<String, ParamValue>);

impl ParamValues {
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.0.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &ParamValue)> {
        self.0.iter()
    }

    /// Validates `input` against `specs`. Unknown keys are rejected, missing keys
    /// take the spec default, and values are coerced to the declared kind.
    pub fn resolve(specs: &[ParamSpec], input: &BTreeMap<String, ParamValue>) -> Result<Self> {
        if let Some(unknown) = input.keys().find(|k| !specs.iter().any(|s| &s.key == *k)) {
            let known: Vec<_> = specs.iter().map(|s| s.key.as_str()).collect();
            return Err(Error::InvalidParam {
                key: unknown.clone(),
                reason: format!("unknown parameter (expected one of: {})", known.join(", ")),
            });
        }
        let mut out = BTreeMap::new();
        for spec in specs {
            let value = match input.get(&spec.key) {
                Some(v) => spec.coerce(v)?,
                None => spec.default.clone(),
            };
            out.insert(spec.key.clone(), value);
        }
        Ok(Self(out))
    }

    /// Parses `key=value` strings (CLI form) into typed values using `specs`.
    pub fn parse_pairs<S: AsRef<str>>(specs: &[ParamSpec], pairs: &[S]) -> Result<BTreeMap<String, ParamValue>> {
        let mut out = BTreeMap::new();
        for pair in pairs {
            let pair = pair.as_ref();
            let (key, raw) = pair.split_once('=').ok_or_else(|| Error::InvalidParam {
                key: pair.to_string(),
                reason: "expected KEY=VALUE".into(),
            })?;
            let key = key.trim();
            let spec = specs.iter().find(|s| s.key == key).ok_or_else(|| Error::InvalidParam {
                key: key.to_string(),
                reason: "unknown parameter".into(),
            })?;
            out.insert(key.to_string(), spec.parse(raw.trim())?);
        }
        Ok(out)
    }
}

impl ParamSpec {
    fn invalid(&self, reason: impl Into<String>) -> Error {
        Error::InvalidParam { key: self.key.clone(), reason: reason.into() }
    }

    /// Parses a textual value according to this spec's kind.
    pub fn parse(&self, raw: &str) -> Result<ParamValue> {
        let value = match &self.kind {
            ParamKind::Bool => match raw.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => ParamValue::Bool(true),
                "false" | "0" | "no" | "off" => ParamValue::Bool(false),
                _ => return Err(self.invalid(format!("`{raw}` is not a boolean"))),
            },
            ParamKind::Int { .. } => {
                ParamValue::Int(raw.parse().map_err(|_| self.invalid(format!("`{raw}` is not an integer")))?)
            }
            ParamKind::Float { .. } => {
                ParamValue::Float(raw.parse().map_err(|_| self.invalid(format!("`{raw}` is not a number")))?)
            }
            ParamKind::Enum { .. } | ParamKind::Text => ParamValue::Text(raw.to_string()),
        };
        self.coerce(&value)
    }

    /// Checks a typed value against this spec, normalising compatible types.
    pub fn coerce(&self, value: &ParamValue) -> Result<ParamValue> {
        match (&self.kind, value) {
            (ParamKind::Bool, ParamValue::Bool(_)) => Ok(value.clone()),
            (ParamKind::Int { min, max }, ParamValue::Int(v)) => {
                if min.is_some_and(|m| *v < m) || max.is_some_and(|m| *v > m) {
                    return Err(self.invalid(format!("{v} is outside {}", range_text(min, max))));
                }
                Ok(value.clone())
            }
            (ParamKind::Float { min, max }, ParamValue::Float(_) | ParamValue::Int(_)) => {
                let v = value.as_f64().unwrap_or_default();
                if min.is_some_and(|m| v < m) || max.is_some_and(|m| v > m) {
                    return Err(self.invalid(format!("{v} is outside {}", range_text(min, max))));
                }
                Ok(ParamValue::Float(v))
            }
            (ParamKind::Enum { options }, ParamValue::Text(v)) => {
                if options.iter().any(|o| &o.value == v) {
                    Ok(value.clone())
                } else {
                    let allowed: Vec<_> = options.iter().map(|o| o.value.as_str()).collect();
                    Err(self.invalid(format!("`{v}` is not one of: {}", allowed.join(", "))))
                }
            }
            (ParamKind::Text, ParamValue::Text(_)) => Ok(value.clone()),
            (ParamKind::Enum { .. } | ParamKind::Text, other) => self.coerce(&ParamValue::Text(other.to_string())),
            (_, other) => Err(self.invalid(format!("unexpected value `{other}`"))),
        }
    }
}

fn range_text<T: std::fmt::Display>(min: &Option<T>, max: &Option<T>) -> String {
    match (min, max) {
        (Some(a), Some(b)) => format!("[{a}, {b}]"),
        (Some(a), None) => format!("[{a}, ∞)"),
        (None, Some(b)) => format!("(-∞, {b}]"),
        (None, None) => "range".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specs() -> Vec<ParamSpec> {
        vec![
            ParamSpec {
                key: "tile".into(),
                label: "Tile".into(),
                description: Text::default(),
                kind: ParamKind::Int { min: Some(0), max: Some(4096) },
                default: ParamValue::Int(0),
                advanced: false,
            },
            ParamSpec {
                key: "tta".into(),
                label: "TTA".into(),
                description: Text::default(),
                kind: ParamKind::Bool,
                default: ParamValue::Bool(false),
                advanced: false,
            },
            ParamSpec {
                key: "gpu".into(),
                label: "GPU".into(),
                description: Text::default(),
                kind: ParamKind::Enum {
                    options: vec![
                        EnumOption { value: "auto".into(), label: "Auto".into() },
                        EnumOption { value: "0".into(), label: "GPU 0".into() },
                    ],
                },
                default: ParamValue::Text("auto".into()),
                advanced: true,
            },
        ]
    }

    #[test]
    fn cli_pairs_are_typed_and_defaults_fill_the_rest() {
        let specs = specs();
        let parsed = ParamValues::parse_pairs(&specs, &["tile=256", "tta=yes"]).unwrap();
        let values = ParamValues::resolve(&specs, &parsed).unwrap();
        assert_eq!(values.get("tile"), Some(&ParamValue::Int(256)));
        assert_eq!(values.get("tta"), Some(&ParamValue::Bool(true)));
        assert_eq!(values.get("gpu"), Some(&ParamValue::Text("auto".into())));
    }

    #[test]
    fn out_of_range_int_is_rejected_with_key() {
        let err = ParamValues::parse_pairs(&specs(), &["tile=5000"]).unwrap_err();
        assert!(matches!(err, Error::InvalidParam { ref key, .. } if key == "tile"), "{err}");
    }

    #[test]
    fn unknown_key_is_rejected() {
        let mut input = BTreeMap::new();
        input.insert("denoise".to_string(), ParamValue::Float(0.5));
        let err = ParamValues::resolve(&specs(), &input).unwrap_err();
        assert!(matches!(err, Error::InvalidParam { ref key, .. } if key == "denoise"), "{err}");
    }

    #[test]
    fn enum_value_outside_options_is_rejected() {
        assert!(ParamValues::parse_pairs(&specs(), &["gpu=7"]).is_err());
    }

    #[test]
    fn json_numbers_from_gui_coerce_into_enum_text() {
        // The GUI may send `0` (number) for an enum whose option value is "0".
        let input: BTreeMap<String, ParamValue> = serde_json::from_str(r#"{"gpu": 0, "tile": 128}"#).unwrap();
        let values = ParamValues::resolve(&specs(), &input).unwrap();
        assert_eq!(values.get("gpu"), Some(&ParamValue::Text("0".into())));
        assert_eq!(values.get("tile"), Some(&ParamValue::Int(128)));
    }
}
