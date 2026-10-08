//! Parameter schema helpers with English and Traditional Chinese text.

use ivsr_core::{ParamKind, ParamSpec, ParamValue, ParamValues, Text};

type Pair = (&'static str, &'static str);

fn text((en, zh): Pair) -> Text {
    Text::en(en).with("zh-TW", zh)
}

pub fn int(key: &str, label: Pair, description: Pair, (min, max): (i64, i64), default: i64) -> ParamSpec {
    ParamSpec {
        key: key.into(),
        label: text(label),
        description: text(description),
        kind: ParamKind::Int { min: Some(min), max: Some(max) },
        default: ParamValue::Int(default),
        advanced: false,
    }
}

pub fn float(key: &str, label: Pair, description: Pair, (min, max): (f64, f64), default: f64) -> ParamSpec {
    ParamSpec {
        key: key.into(),
        label: text(label),
        description: text(description),
        kind: ParamKind::Float { min: Some(min), max: Some(max) },
        default: ParamValue::Float(default),
        advanced: false,
    }
}

pub fn advanced(spec: ParamSpec) -> ParamSpec {
    ParamSpec { advanced: true, ..spec }
}

/// Values were validated against the schema, so every key is present.
pub fn int_param(params: &ParamValues, key: &str) -> i64 {
    params.get(key).and_then(ParamValue::as_i64).unwrap_or_default()
}

pub fn float_param(params: &ParamValues, key: &str) -> f32 {
    params.get(key).and_then(ParamValue::as_f64).unwrap_or_default() as f32
}
