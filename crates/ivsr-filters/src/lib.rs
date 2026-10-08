//! Built-in pre- and post-processing filters.
//!
//! The post-processing set is a native port of `prototype/enhance_sr_sprites.py`,
//! split into steps that can be switched and reordered independently. Each
//! filter works on 8-bit RGBA frames and leaves fully transparent pixels to
//! `alpha-bleed`, so the same code serves cut-out sprites and opaque video.

mod bleed;
mod lab;
mod param;
mod plane;
mod saturation;
mod sharpen;
mod tone;

use std::sync::Arc;

use ivsr_core::{Filter, FilterStage, FilterStep, ParamValue};

pub use bleed::{AlphaBleed, ID as ALPHA_BLEED};
pub use saturation::{ID as SATURATION, Saturation};
pub use sharpen::{DetailSharpen, ID as DETAIL_SHARPEN};
pub use tone::{ID as TONE_RESTORE, ToneRestore};

/// Every filter compiled into this build. Adding a filter means adding a line here.
pub fn builtin() -> Vec<Arc<dyn Filter>> {
    vec![Arc::new(AlphaBleed), Arc::new(ToneRestore), Arc::new(DetailSharpen), Arc::new(Saturation)]
}

/// The built-in order for each stage, used until a user lists their own steps.
pub fn default_steps(stage: FilterStage) -> Vec<FilterStep> {
    match stage {
        // Fill only fully transparent pixels so the engine does not see black
        // around cut-out edges; visible pixels stay as they are.
        FilterStage::Pre => vec![FilterStep::new(ALPHA_BLEED).with("threshold", ParamValue::Int(0))],
        // The order of the original script: de-fringe, restore tones, sharpen, saturate.
        FilterStage::Post => vec![
            FilterStep::new(ALPHA_BLEED),
            FilterStep::new(TONE_RESTORE),
            FilterStep::new(DETAIL_SHARPEN),
            FilterStep::new(SATURATION),
        ],
    }
}

#[cfg(test)]
mod tests {
    use ivsr_core::filter::resolve;

    use super::*;

    #[test]
    fn default_chains_resolve_against_the_builtin_filters() {
        let filters = builtin();
        for stage in [FilterStage::Pre, FilterStage::Post] {
            let specs = resolve(stage, &default_steps(stage), &filters).unwrap();
            assert_eq!(specs.len(), default_steps(stage).len());
        }
    }

    #[test]
    fn ids_are_unique() {
        let ids: Vec<String> = builtin().iter().map(|f| f.info().id).collect();
        let unique: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }
}
