//! World-owned native features with seed-derived randomness. No feature RNG is
//! exposed: algorithms that consume the decoration RNG need a separate API.
use super::placement::{PlacementContext, PlacementQueryError};
use crate::{fluid::FluidState, heightmap::HeightmapType, registry::BlockStateId};

pub trait ExternalFeature: std::fmt::Debug + Send + Sync {
    fn place(
        &self,
        world: &mut dyn ExternalFeatureWorld,
        origin: [i32; 3],
    ) -> Result<bool, PlacementQueryError>;
}

/// An object-safe subset of placement queries. Writes use generation chunks
/// and update their heightmaps; they do not perform live neighbor notifications.
pub trait ExternalFeatureWorld {
    fn block_state(&mut self, position: [i32; 3]) -> Result<BlockStateId, PlacementQueryError>;
    fn set_chunk_block(
        &mut self,
        position: [i32; 3],
        state: BlockStateId,
    ) -> Result<(), PlacementQueryError>;
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Result<i32, PlacementQueryError>;
    fn min_y(&mut self) -> Result<i32, PlacementQueryError>;
    /// Inclusive maximum block Y, matching PlacementContext.
    fn max_y(&mut self) -> Result<i32, PlacementQueryError>;
    fn sea_level(&mut self) -> Result<i32, PlacementQueryError>;
    fn world_seed(&mut self) -> Result<i64, PlacementQueryError>;
    fn fluid_state(&mut self, position: [i32; 3]) -> Result<FluidState, PlacementQueryError>;
    fn block_matches_tag(
        &mut self,
        position: [i32; 3],
        tag: &str,
    ) -> Result<bool, PlacementQueryError>;
    /// Exact noise-biome quart coordinates, without fuzzy block-biome lookup.
    fn noise_biome_is(&mut self, quart: [i32; 3], name: &str) -> Result<bool, PlacementQueryError>;
}

impl<T: PlacementContext> ExternalFeatureWorld for T {
    fn block_state(&mut self, p: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        PlacementContext::block_state(self, p)
    }
    fn set_chunk_block(
        &mut self,
        p: [i32; 3],
        state: BlockStateId,
    ) -> Result<(), PlacementQueryError> {
        PlacementContext::set_chunk_block(self, p, state)
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Result<i32, PlacementQueryError> {
        PlacementContext::height(self, kind, x, z)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        PlacementContext::min_y(self)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        PlacementContext::max_y(self)
    }
    fn sea_level(&mut self) -> Result<i32, PlacementQueryError> {
        PlacementContext::sea_level(self)
    }
    fn world_seed(&mut self) -> Result<i64, PlacementQueryError> {
        PlacementContext::world_seed(self)
    }
    fn fluid_state(&mut self, p: [i32; 3]) -> Result<FluidState, PlacementQueryError> {
        PlacementContext::fluid_state(self, p)
    }
    fn block_matches_tag(&mut self, p: [i32; 3], tag: &str) -> Result<bool, PlacementQueryError> {
        PlacementContext::block_matches_tag(self, p, tag)
    }
    fn noise_biome_is(&mut self, quart: [i32; 3], name: &str) -> Result<bool, PlacementQueryError> {
        PlacementContext::noise_biome_is(self, quart, name)
    }
}

#[cfg(test)]
mod tests;
