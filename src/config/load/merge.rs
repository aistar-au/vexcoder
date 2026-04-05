use anyhow::{Context, Result};
use figment::{providers::Serialized, Figment};

use super::{AutoMemoryConfigLayer, ConfigLayer, DoctorConfigLayer};
use crate::config::AutoMemoryConfig;

pub(super) fn merge_layers<I>(layers: I) -> Result<ConfigLayer>
where
    I: IntoIterator<Item = ConfigLayer>,
{
    layers
        .into_iter()
        .fold(Figment::new(), |figment, layer| {
            figment.merge(Serialized::defaults(layer))
        })
        .extract()
        .context("failed to merge config layers")
}

pub(super) fn merge_doctor_layers<I>(layers: I) -> Result<DoctorConfigLayer>
where
    I: IntoIterator<Item = DoctorConfigLayer>,
{
    layers
        .into_iter()
        .fold(Figment::new(), |figment, layer| {
            figment.merge(Serialized::defaults(layer))
        })
        .extract()
        .context("failed to merge doctor config layers")
}

pub(super) fn resolve_auto_memory_config(layer: Option<AutoMemoryConfigLayer>) -> AutoMemoryConfig {
    let layer = layer.unwrap_or_default();
    let max_notes = layer.max_notes_per_turn.unwrap_or(3).clamp(1, 10);
    AutoMemoryConfig {
        enabled: layer.enabled.unwrap_or(false),
        max_notes_per_turn: max_notes,
    }
}
