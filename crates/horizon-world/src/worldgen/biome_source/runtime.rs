//! World-bound biome sources and per-worker resolver state.
use super::{
    BiomeSourceDefinition, CheckerboardBiomeSource, CheckerboardError, EndBiomeSource,
    FixedBiomeSource,
    region_selector::{RegionBiomeSource, RegionError, RegionResolver},
    regions::RegionLayout,
};
use crate::worldgen::{
    DecodeError,
    biome::{BiomeError, BiomeResolver, MultiNoiseBiomeSource},
    climate::{ClimateSampler, ClimateVolume},
    mod_biome_sources::{LevelBiomeMapper, LevelBiomeSource, mod_biome_sources},
};
use horizon_core::{
    density::{DensitySampler, SampleError, SamplerContext},
    random::{PositionalRandom, RandomSource},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub enum BiomeSourceError {
    Decode(DecodeError),
    Biome(BiomeError),
    Checkerboard(CheckerboardError),
    Sample(SampleError),
    Regions(RegionError),
    MissingPreset,
    /// A mod's biome source that is gone or no longer fits the world's biomes.
    Registered(String),
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
impl From<RegionError> for BiomeSourceError {
    fn from(e: RegionError) -> Self {
        Self::Regions(e)
    }
}

#[derive(Debug)]
pub enum ResolvedBiomeSource {
    Fixed(FixedBiomeSource<String>),
    Checkerboard(CheckerboardBiomeSource<String>),
    End(EndBiomeSource<String>),
    MultiNoise(MultiNoiseBiomeSource),
    /// A multi-noise preset with a world's mod regions.
    Regions(RegionBiomeSource),
    /// A biome source a mod registered, over one world's seed.
    Registered(LevelBiomeSource<String>),
    /// A biome source whose results pass through a level's native mapper.
    Mapped(Box<ResolvedBiomeSource>, LevelBiomeMapper),
}
impl BiomeSourceDefinition {
    pub(in crate::worldgen) fn uses_regions(&self) -> bool {
        match self {
            Self::Preset { .. } => true,
            Self::Mapped(source) => source.base.uses_regions(),
            _ => false,
        }
    }

    /// Resolve a decoded definition using immutable, authenticated quantized
    /// preset tables. This does not execute vanilla's built-in preset builders.
    /// Only a mod's registered source or configured mapper reads the world's `seed`.
    pub fn resolve(
        &self,
        presets: &BTreeMap<String, String>,
        seed: i64,
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
            Self::Registered(id) => ResolvedBiomeSource::Registered(
                LevelBiomeSource::resolve(id, mod_biome_sources(), seed, |name| {
                    Some(name.to_owned())
                })
                .ok_or_else(|| BiomeSourceError::Registered(format!("unknown biome source {id}")))?
                .map_err(BiomeSourceError::Registered)?,
            ),
            Self::Mapped(source) => ResolvedBiomeSource::Mapped(
                Box::new(source.base.resolve(presets, seed)?),
                LevelBiomeMapper::bind(&source.id, &source.config, &source.known_biomes, seed)
                    .map_err(BiomeSourceError::Registered)?,
            ),
        })
    }
}
impl ResolvedBiomeSource {
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
            Self::Regions(source) => source.biomes().collect(),
            Self::Registered(source) => source.biomes().iter().map(String::as_str).collect(),
            Self::Mapped(base, mapper) => {
                let mut biomes = base.possible_biomes();
                biomes.extend(mapper.additions().iter().map(String::as_str));
                biomes
            }
        }
    }
    /// A multi-noise source with `layout`'s regions over it, read from the generator's
    /// positional random `root` without advancing it. Without regions, or for any other
    /// source, `self` unchanged: no region stream is drawn.
    pub fn with_regions<P>(
        self,
        layout: Option<&RegionLayout>,
        root: &P,
    ) -> Result<Self, BiomeSourceError>
    where
        P: PositionalRandom,
        <P::Source as RandomSource>::Positional: Send + Sync + 'static,
    {
        Ok(match (self, layout) {
            (Self::MultiNoise(vanilla), Some(layout)) if !layout.regions.is_empty() => {
                Self::Regions(RegionBiomeSource::new(vanilla, layout, root)?)
            }
            (Self::Mapped(base, mapper), layout) => {
                Self::Mapped(Box::new(base.with_regions(layout, root)?), mapper)
            }
            (source, _) => source,
        })
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
            Self::Regions(source) => BiomeSourceResolver::Regions(source.resolver(climate)),
            Self::Registered(source) => BiomeSourceResolver::Registered(source),
            Self::Mapped(base, mapper) => {
                BiomeSourceResolver::Mapped(Box::new(base.resolver(climate, erosion)), mapper)
            }
        }
    }
}
#[derive(Debug)]
pub enum BiomeSourceResolver<'a> {
    Fixed(&'a FixedBiomeSource<String>),
    Checkerboard(&'a CheckerboardBiomeSource<String>),
    End(&'a EndBiomeSource<String>, &'a dyn DensitySampler),
    MultiNoise(BiomeResolver<'a>),
    Regions(RegionResolver<'a>),
    Registered(&'a LevelBiomeSource<String>),
    Mapped(Box<BiomeSourceResolver<'a>>, &'a LevelBiomeMapper),
}
impl<'a> BiomeSourceResolver<'a> {
    pub fn sample(
        &mut self,
        context: &mut SamplerContext,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeSourceError> {
        self.sample_prepared(context, None, quart)
    }
    #[inline]
    fn sample_base(
        &mut self,
        context: &mut SamplerContext,
        volume: Option<&ClimateVolume>,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeSourceError> {
        Ok(match (self, volume) {
            (Self::Fixed(source), _) => source.biome.as_str(),
            (Self::Checkerboard(source), _) => source.noise_biome(quart)?.as_str(),
            (Self::End(source, erosion), _) => {
                source.noise_biome(quart, *erosion, context)?.as_str()
            }
            (Self::MultiNoise(source), Some(volume)) => source.sample_prepared(volume, quart)?,
            (Self::Regions(source), Some(volume)) => source.sample_prepared(volume, quart)?,
            (Self::MultiNoise(source), None) => source.sample(context, quart)?,
            (Self::Regions(source), None) => source.sample(context, quart)?,
            (Self::Registered(source), _) => source.noise_biome(quart).as_str(),
            (Self::Mapped(..), _) => {
                return Err(BiomeSourceError::Registered("nested biome mapper".into()));
            }
        })
    }
    /// Native fixed/checkerboard/End and registered sources do not pre-sample a climate volume.
    /// Multi-noise preparation samples the full supplied quart extent once.
    pub fn prepare_volume(
        &self,
        context: &mut SamplerContext,
        minimum: [i32; 3],
        extents: [i32; 3],
    ) -> Result<Option<ClimateVolume>, BiomeSourceError> {
        match self {
            Self::MultiNoise(source) => Ok(Some(source.prepare_volume(context, minimum, extents)?)),
            Self::Regions(source) => Ok(Some(source.prepare_volume(context, minimum, extents)?)),
            Self::Mapped(base, _) => base.prepare_volume(context, minimum, extents),
            _ => Ok(None),
        }
    }
    #[inline]
    pub fn sample_prepared(
        &mut self,
        context: &mut SamplerContext,
        volume: Option<&ClimateVolume>,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeSourceError> {
        match self {
            Self::Mapped(base, mapper) => mapper
                .sample(base.sample_base(context, volume, quart)?, quart)
                .map_err(BiomeSourceError::Registered),
            base => base.sample_base(context, volume, quart),
        }
    }
}
