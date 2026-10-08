//! The image filter contract: pre-processing before the engine and
//! post-processing after it.
//!
//! A `Filter` describes itself (stages, parameter schema) the same way an
//! `Engine` does, so frontends list, order and configure filters without
//! knowing any of them. Each job starts a fresh `FilterRun` per filter and
//! feeds it every picture in display order, so filters may keep state across
//! video frames (temporal smoothing).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{Error, Frame, MediaKind, ParamSpec, ParamValue, ParamValues, Result, Text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterStage {
    /// On the source, before the engine.
    Pre,
    /// On the engine output at its final size, before encoding.
    Post,
}

impl FilterStage {
    pub fn label(self) -> &'static str {
        match self {
            FilterStage::Pre => "pre",
            FilterStage::Post => "post",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterInfo {
    /// Stable identifier used in config files and on the command line.
    pub id: String,
    pub name: Text,
    pub description: Text,
    /// Stages the filter may run in.
    pub stages: Vec<FilterStage>,
    /// Whether the filter reads the reference picture when one is given.
    #[serde(default)]
    pub uses_reference: bool,
}

/// What one run of a filter is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterSetup {
    pub stage: FilterStage,
    /// Video runs receive every frame in display order.
    pub kind: MediaKind,
}

/// An image filter. Implementations hold no per-job state; `start` creates it.
pub trait Filter: Send + Sync {
    fn info(&self) -> FilterInfo;

    fn params(&self) -> Vec<ParamSpec>;

    /// Starts processing one image or one video. `params` were validated
    /// against `params()`.
    fn start(&self, params: &ParamValues, setup: FilterSetup) -> Result<Box<dyn FilterRun>>;
}

/// One filter applied to the pictures of one job, in display order.
pub trait FilterRun: Send {
    /// Processes `frame` in place, keeping its size and alpha channel.
    /// `reference` is the picture the engine received for this frame, at its
    /// own (source) resolution; it is `None` for pre-processing and for
    /// stand-alone use without a reference. Fully transparent pixels must be
    /// left untouched unless recolouring them is the filter's purpose.
    fn apply(&mut self, frame: &mut Frame, reference: Option<&Frame>) -> Result<()>;
}

/// One configured step of a chain, before validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterStep {
    pub id: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Overrides of the filter's parameter defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
}

fn enabled_by_default() -> bool {
    true
}

impl FilterStep {
    pub fn new(id: &str) -> Self {
        Self { id: id.into(), enabled: true, params: BTreeMap::new() }
    }

    pub fn with(mut self, key: &str, value: ParamValue) -> Self {
        self.params.insert(key.into(), value);
        self
    }
}

/// One stage's chain as configured: a master switch plus ordered steps.
/// `steps: None` follows the built-in default order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterChain {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<FilterStep>>,
}

/// A validated step ready to run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterSpec {
    pub id: String,
    pub params: ParamValues,
}

pub fn find<'a>(filters: &'a [Arc<dyn Filter>], id: &str) -> Option<&'a Arc<dyn Filter>> {
    filters.iter().find(|f| f.info().id == id)
}

/// Validates the enabled `steps` for `stage`: every id must name a filter
/// that supports the stage, and parameters must fit its schema.
pub fn resolve(stage: FilterStage, steps: &[FilterStep], filters: &[Arc<dyn Filter>]) -> Result<Vec<FilterSpec>> {
    steps
        .iter()
        .filter(|s| s.enabled)
        .map(|step| {
            let filter = find(filters, &step.id).ok_or_else(|| {
                let known: Vec<_> = filters.iter().map(|f| f.info().id).collect();
                Error::Invalid(format!("unknown filter `{}` (available: {})", step.id, known.join(", ")))
            })?;
            if !filter.info().stages.contains(&stage) {
                return Err(Error::Invalid(format!("filter `{}` cannot run as {}-processing", step.id, stage.label())));
            }
            let params = ParamValues::resolve(&filter.params(), &step.params).map_err(|e| match e {
                Error::InvalidParam { key, reason } => Error::InvalidParam { key: format!("{}.{key}", step.id), reason },
                other => other,
            })?;
            Ok(FilterSpec { id: step.id.clone(), params })
        })
        .collect()
}

/// Running filters of one stage, applied in order.
pub struct Runner {
    runs: Vec<Box<dyn FilterRun>>,
}

impl Runner {
    pub fn start(specs: &[FilterSpec], filters: &[Arc<dyn Filter>], setup: FilterSetup) -> Result<Self> {
        let runs = specs
            .iter()
            .map(|spec| {
                let filter = find(filters, &spec.id).ok_or_else(|| Error::Invalid(format!("unknown filter `{}`", spec.id)))?;
                filter.start(&spec.params, setup)
            })
            .collect::<Result<_>>()?;
        Ok(Self { runs })
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn apply(&mut self, frame: &mut Frame, reference: Option<&Frame>) -> Result<()> {
        for run in &mut self.runs {
            run.apply(frame, reference)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParamKind;

    /// Adds `amount` to every red value and counts the frames it saw.
    struct Brighten;

    struct BrightenRun {
        amount: u8,
        seen: u32,
    }

    impl Filter for Brighten {
        fn info(&self) -> FilterInfo {
            FilterInfo {
                id: "brighten".into(),
                name: "Brighten".into(),
                description: Text::default(),
                stages: vec![FilterStage::Post],
                uses_reference: false,
            }
        }
        fn params(&self) -> Vec<ParamSpec> {
            vec![ParamSpec {
                key: "amount".into(),
                label: "Amount".into(),
                description: Text::default(),
                kind: ParamKind::Int { min: Some(0), max: Some(50) },
                default: ParamValue::Int(10),
                advanced: false,
            }]
        }
        fn start(&self, params: &ParamValues, _: FilterSetup) -> Result<Box<dyn FilterRun>> {
            let amount = params.get("amount").and_then(ParamValue::as_i64).unwrap_or(0) as u8;
            Ok(Box::new(BrightenRun { amount, seen: 0 }))
        }
    }

    impl FilterRun for BrightenRun {
        fn apply(&mut self, frame: &mut Frame, _: Option<&Frame>) -> Result<()> {
            self.seen += 1;
            for px in frame.pixels.as_chunks_mut::<4>().0 {
                px[0] = px[0].saturating_add(self.amount) + (self.seen as u8 - 1);
            }
            Ok(())
        }
    }

    fn filters() -> Vec<Arc<dyn Filter>> {
        vec![Arc::new(Brighten)]
    }

    #[test]
    fn disabled_steps_are_dropped_and_params_take_defaults() {
        let steps = vec![
            FilterStep::new("brighten"),
            FilterStep { enabled: false, ..FilterStep::new("nonexistent") },
            FilterStep::new("brighten").with("amount", ParamValue::Int(3)),
        ];
        let specs = resolve(FilterStage::Post, &steps, &filters()).unwrap();
        let amounts: Vec<_> = specs.iter().map(|s| s.params.get("amount").cloned()).collect();
        assert_eq!(amounts, vec![Some(ParamValue::Int(10)), Some(ParamValue::Int(3))]);
    }

    #[test]
    fn unknown_filter_wrong_stage_and_bad_params_are_rejected() {
        let err = resolve(FilterStage::Post, &[FilterStep::new("blur")], &filters()).unwrap_err();
        assert!(err.to_string().contains("unknown filter `blur` (available: brighten)"), "{err}");
        let err = resolve(FilterStage::Pre, &[FilterStep::new("brighten")], &filters()).unwrap_err();
        assert!(err.to_string().contains("cannot run as pre-processing"), "{err}");
        let step = FilterStep::new("brighten").with("amount", ParamValue::Int(99));
        let err = resolve(FilterStage::Post, &[step], &filters()).unwrap_err();
        assert!(matches!(err, Error::InvalidParam { ref key, .. } if key == "brighten.amount"), "{err}");
    }

    #[test]
    fn runner_applies_steps_in_order_and_keeps_state_between_frames() {
        let filters = filters();
        let steps = [FilterStep::new("brighten").with("amount", ParamValue::Int(1))];
        let specs = resolve(FilterStage::Post, &steps, &filters).unwrap();
        let setup = FilterSetup { stage: FilterStage::Post, kind: MediaKind::Video };
        let mut runner = Runner::start(&specs, &filters, setup).unwrap();
        let mut a = Frame::filled(1, 1, [0, 0, 0, 255], false);
        let mut b = a.clone();
        runner.apply(&mut a, None).unwrap();
        runner.apply(&mut b, None).unwrap();
        assert_eq!((a.pixels[0], b.pixels[0]), (1, 2));
    }

    #[test]
    fn chain_round_trips_through_toml_with_default_steps_omitted() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrap {
            post: FilterChain,
        }
        let default = Wrap { post: FilterChain { enabled: true, steps: None } };
        let text = toml::to_string(&default).unwrap();
        assert_eq!(text.trim(), "[post]\nenabled = true");
        let custom: Wrap = toml::from_str(
            "[post]\nenabled = true\n[[post.steps]]\nid = \"a\"\n[[post.steps]]\nid = \"b\"\nenabled = false\nparams = { x = 1.5 }\n",
        )
        .unwrap();
        let steps = custom.post.steps.as_ref().unwrap();
        assert_eq!(steps[0], FilterStep::new("a"));
        assert_eq!(steps[1].params.get("x"), Some(&ParamValue::Float(1.5)));
        assert!(!steps[1].enabled);
        assert_eq!(toml::from_str::<Wrap>(&toml::to_string(&custom).unwrap()).unwrap(), custom);
    }
}
