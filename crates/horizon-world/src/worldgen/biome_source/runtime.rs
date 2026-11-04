//! World-bound biome sources and per-worker resolver state.
use super::{
    BiomeSourceDefinition, CheckerboardBiomeSource, CheckerboardError, EndBiomeSource,
    FixedBiomeSource,
};
use crate::worldgen::{
    DecodeError,
    biome::{BiomeError, BiomeResolver, MultiNoiseBiomeSource},
    climate::{ClimateSampler, ClimateVolume},
};
use horizon_core::density::{DensitySampler, SampleError, SamplerContext};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// A world-owned transformation of a resolved biome. Implementations must
/// declare every additional result before the pipeline builds its biome and
/// decoration catalogs. The parent resolver retains its normal search state.
pub trait BiomeMapper: std::fmt::Debug + Send + Sync {
    fn additional_biomes(&self) -> &[String];
    fn map<'a>(&'a self, parent: &'a str, quart: [i32; 3]) -> Result<&'a str, SampleError>;
}

#[derive(Debug)]
pub struct MappedBiomeSource {
    source: Box<ResolvedBiomeSource>,
    mapper: Arc<dyn BiomeMapper>,
    additional: Vec<String>,
    possible: BTreeSet<String>,
}
impl MappedBiomeSource {
    pub fn source(&self) -> &ResolvedBiomeSource {
        &self.source
    }
    pub fn additional_biomes(&self) -> &[String] {
        &self.additional
    }
}

#[derive(Debug)]
pub enum BiomeSourceError {
    Decode(DecodeError),
    Biome(BiomeError),
    Checkerboard(CheckerboardError),
    Sample(SampleError),
    MissingPreset,
    UndeclaredMappedBiome(String),
}
impl std::fmt::Display for BiomeSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for BiomeSourceError {}
impl From<DecodeError> for BiomeSourceError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}
impl From<BiomeError> for BiomeSourceError {
    fn from(e: BiomeError) -> Self {
        Self::Biome(e)
    }
}
impl From<CheckerboardError> for BiomeSourceError {
    fn from(e: CheckerboardError) -> Self {
        Self::Checkerboard(e)
    }
}
impl From<SampleError> for BiomeSourceError {
    fn from(e: SampleError) -> Self {
        Self::Sample(e)
    }
}

#[derive(Debug)]
pub enum ResolvedBiomeSource {
    Fixed(FixedBiomeSource<String>),
    Checkerboard(CheckerboardBiomeSource<String>),
    End(EndBiomeSource<String>),
    MultiNoise(MultiNoiseBiomeSource),
    Mapped(MappedBiomeSource),
}
impl BiomeSourceDefinition {
    /// Resolve a decoded definition using immutable, authenticated quantized
    /// preset tables. This does not execute vanilla's built-in preset builders.
    pub fn resolve(
        &self,
        presets: &BTreeMap<String, String>,
    ) -> Result<ResolvedBiomeSource, BiomeSourceError> {
        Ok(match self {
            Self::Fixed(biome) => ResolvedBiomeSource::Fixed(FixedBiomeSource {
                biome: biome.clone(),
            }),
            Self::Checkerboard { biomes, scale } => ResolvedBiomeSource::Checkerboard(
                CheckerboardBiomeSource::new(biomes.members().to_vec(), *scale)?,
            ),
            Self::End => ResolvedBiomeSource::End(EndBiomeSource {
                center: "minecraft:the_end".into(),
                highlands: "minecraft:end_highlands".into(),
                midlands: "minecraft:end_midlands".into(),
                islands: "minecraft:small_end_islands".into(),
                barrens: "minecraft:end_barrens".into(),
            }),
            Self::MultiNoise(entries) => {
                ResolvedBiomeSource::MultiNoise(MultiNoiseBiomeSource::new(entries.clone())?)
            }
            Self::Preset { name, .. } => {
                ResolvedBiomeSource::MultiNoise(MultiNoiseBiomeSource::from_quantized_json(
                    presets.get(name).ok_or(BiomeSourceError::MissingPreset)?,
                )?)
            }
        })
    }
}
impl ResolvedBiomeSource {
    /// Attach before building a pipeline's catalogs, never after filling chunks.
    pub fn with_mapper(self, mapper: Arc<dyn BiomeMapper>) -> Result<Self, BiomeSourceError> {
        let additional = mapper
            .additional_biomes()
            .iter()
            .map(|name| {
                let canonical = super::super::density::identifier(name)?;
                if &canonical != name {
                    return Err(DecodeError::Field("mapped biome must be namespaced"));
                }
                Ok(canonical)
            })
            .collect::<Result<Vec<_>, DecodeError>>()?;
        let mut possible: BTreeSet<String> = self
            .possible_biomes()
            .into_iter()
            .map(str::to_owned)
            .collect();
        possible.extend(additional.iter().cloned());
        Ok(Self::Mapped(MappedBiomeSource {
            source: Box::new(self),
            mapper,
            additional,
            possible,
        }))
    }
    pub(crate) fn is_end(&self) -> bool {
        match self {
            Self::End(_) => true,
            Self::Mapped(source) => source.source.is_end(),
            _ => false,
        }
    }
    pub fn possible_biomes(&self) -> BTreeSet<&str> {
        match self {
            Self::Fixed(source) => [source.biome.as_str()].into(),
            Self::Checkerboard(source) => source.biomes.iter().map(String::as_str).collect(),
            Self::End(source) => [
                source.center.as_str(),
                source.highlands.as_str(),
                source.midlands.as_str(),
                source.islands.as_str(),
                source.barrens.as_str(),
            ]
            .into(),
            Self::MultiNoise(source) => source
                .parameters()
                .entries()
                .iter()
                .map(|(_, name)| name.as_str())
                .collect(),
            Self::Mapped(source) => source.possible.iter().map(String::as_str).collect(),
        }
    }
    /// Keep one resolver per worker to retain native previous-leaf tie behavior.
    /// Climate and erosion roots must share the worker's compiler/context namespace.
    pub fn resolver<'a>(
        &'a self,
        climate: &'a ClimateSampler,
        erosion: &'a dyn DensitySampler,
    ) -> BiomeSourceResolver<'a> {
        match self {
            Self::Fixed(source) => BiomeSourceResolver::Fixed(source),
            Self::Checkerboard(source) => BiomeSourceResolver::Checkerboard(source),
            Self::End(source) => BiomeSourceResolver::End(source, erosion),
            Self::MultiNoise(source) => BiomeSourceResolver::MultiNoise(source.resolver(climate)),
            Self::Mapped(source) => BiomeSourceResolver::Mapped {
                source: Box::new(source.source.resolver(climate, erosion)),
                mapper: source.mapper.as_ref(),
                possible: &source.possible,
            },
        }
    }
}
#[derive(Debug)]
pub enum BiomeSourceResolver<'a> {
    Fixed(&'a FixedBiomeSource<String>),
    Checkerboard(&'a CheckerboardBiomeSource<String>),
    End(&'a EndBiomeSource<String>, &'a dyn DensitySampler),
    MultiNoise(BiomeResolver<'a>),
    Mapped {
        source: Box<BiomeSourceResolver<'a>>,
        mapper: &'a dyn BiomeMapper,
        possible: &'a BTreeSet<String>,
    },
}
impl<'a> BiomeSourceResolver<'a> {
    pub fn sample(
        &mut self,
        context: &mut SamplerContext,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeSourceError> {
        Ok(match self {
            Self::Fixed(source) => source.biome.as_str(),
            Self::Checkerboard(source) => source.noise_biome(quart)?.as_str(),
            Self::End(source, erosion) => source.noise_biome(quart, *erosion, context)?.as_str(),
            Self::MultiNoise(source) => source.sample(context, quart)?,
            Self::Mapped {
                source,
                mapper,
                possible,
            } => map_biome(*mapper, possible, source.sample(context, quart)?, quart)?,
        })
    }
    /// Native fixed/checkerboard/End sources do not pre-sample a climate volume.
    /// Multi-noise preparation samples the full supplied quart extent once.
    pub fn prepare_volume(
        &self,
        context: &mut SamplerContext,
        minimum: [i32; 3],
        extents: [i32; 3],
    ) -> Result<Option<ClimateVolume>, BiomeSourceError> {
        match self {
            Self::MultiNoise(source) => Ok(Some(source.prepare_volume(context, minimum, extents)?)),
            Self::Mapped { source, .. } => source.prepare_volume(context, minimum, extents),
            _ => Ok(None),
        }
    }
    pub fn sample_prepared(
        &mut self,
        context: &mut SamplerContext,
        volume: Option<&ClimateVolume>,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeSourceError> {
        if let Self::Mapped {
            source,
            mapper,
            possible,
        } = self
        {
            return map_biome(
                *mapper,
                possible,
                source.sample_prepared(context, volume, quart)?,
                quart,
            );
        }
        if let Self::MultiNoise(source) = self {
            if let Some(volume) = volume {
                return Ok(source.sample_prepared(volume, quart)?);
            }
        }
        self.sample(context, quart)
    }
}

fn map_biome<'a>(
    mapper: &'a dyn BiomeMapper,
    possible: &BTreeSet<String>,
    parent: &'a str,
    quart: [i32; 3],
) -> Result<&'a str, BiomeSourceError> {
    let mapped = mapper.map(parent, quart)?;
    if !possible.contains(mapped) {
        return Err(BiomeSourceError::UndeclaredMappedBiome(mapped.to_owned()));
    }
    Ok(mapped)
}
