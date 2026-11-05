use super::{ExternalFeature, ExternalFeatureWorld};
use crate::{
    heightmap::HeightmapType,
    registry::BlockStateId,
    worldgen::{
        anchor::WorldGenerationContext,
        feature::{FeatureBlocks, FeatureWorld},
        feature_graph::{FeatureDefinition, FeatureExecutionError, PlacedFeatureDefinition},
        placement::{PlacementContext, PlacementQueryError},
    },
};
use horizon_core::random::{LegacyRandom, RandomSource};
use std::{collections::BTreeMap, sync::Arc};

struct World {
    blocks: BTreeMap<[i32; 3], BlockStateId>,
    fail_y: Option<i32>,
}
impl World {
    fn new() -> Self {
        Self {
            blocks: BTreeMap::from([([-5, 4, 12], BlockStateId(1))]),
            fail_y: None,
        }
    }
}
impl PlacementContext for World {
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-4, 20, 4)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(-4)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(15)
    }
    fn world_seed(&mut self) -> Result<i64, PlacementQueryError> {
        Ok(i64::MIN + 4)
    }
    fn height(&mut self, _: HeightmapType, x: i32, z: i32) -> Result<i32, PlacementQueryError> {
        Ok(self
            .blocks
            .iter()
            .filter_map(|(p, s)| (p[0] == x && p[2] == z && s.0 != 0).then_some(p[1] + 1))
            .max()
            .unwrap_or(-4))
    }
    fn noise_biome_is(&mut self, quart: [i32; 3], name: &str) -> Result<bool, PlacementQueryError> {
        Ok(quart == [-2, 0, 3] && name == "example:plateau")
    }
    fn block_state(&mut self, p: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        Ok(*self.blocks.get(&p).unwrap_or(&BlockStateId(0)))
    }
    fn set_chunk_block(
        &mut self,
        p: [i32; 3],
        state: BlockStateId,
    ) -> Result<(), PlacementQueryError> {
        if self.fail_y == Some(p[1]) {
            return Err(PlacementQueryError::Failed("write refused".into()));
        }
        self.blocks.insert(p, state);
        Ok(())
    }
}
impl FeatureWorld for World {
    type Error = PlacementQueryError;
    fn block(&mut self, p: [i32; 3]) -> Result<BlockStateId, Self::Error> {
        PlacementContext::block_state(self, p)
    }
    fn set_block(&mut self, _: [i32; 3], _: BlockStateId, _: i32) -> Result<bool, Self::Error> {
        panic!("external writes must retain chunk heightmap semantics")
    }
}
#[derive(Debug)]
struct Column;
impl ExternalFeature for Column {
    fn place(
        &self,
        world: &mut dyn ExternalFeatureWorld,
        origin: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        if !world.noise_biome_is(
            [origin[0] >> 2, origin[1] >> 2, origin[2] >> 2],
            "example:plateau",
        )? {
            return Ok(false);
        }
        assert_eq!(world.world_seed()?, i64::MIN + 4);
        let y = world.height(HeightmapType::OceanFloorWg, origin[0], origin[2])?;
        if y < world.min_y()? || y + 1 > world.max_y()? {
            return Ok(false);
        }
        world.set_chunk_block([origin[0], y, origin[2]], BlockStateId(5))?;
        world.set_chunk_block([origin[0], y + 1, origin[2]], BlockStateId(6))?;
        Ok(true)
    }
}
fn blocks() -> FeatureBlocks {
    FeatureBlocks {
        air: BlockStateId(0),
        stone: BlockStateId(1),
        cobblestone: BlockStateId(2),
        end_stone: BlockStateId(3),
        obsidian: BlockStateId(4),
    }
}

#[test]
fn external_feature_places_in_composition_without_drawing_decoration_random() {
    let mut world = World::new();
    let mut random = LegacyRandom::new(93);
    let mut untouched = random.clone();
    let feature = FeatureDefinition::Sequence(vec![PlacedFeatureDefinition {
        feature: Box::new(FeatureDefinition::External(Arc::new(Column))),
        placement: vec![],
    }]);
    assert!(
        feature
            .place(&mut world, &mut random, [-5, 0, 12], blocks())
            .unwrap()
    );
    assert_eq!(world.blocks.get(&[-5, 5, 12]), Some(&BlockStateId(5)));
    assert_eq!(world.blocks.get(&[-5, 6, 12]), Some(&BlockStateId(6)));
    assert_eq!(
        PlacementContext::height(&mut world, HeightmapType::OceanFloorWg, -5, 12).unwrap(),
        7
    );
    assert_eq!(random.next_long(), untouched.next_long());
    assert!(
        !feature
            .place(&mut world, &mut random, [-4, 0, 12], blocks())
            .unwrap()
    );
    assert_eq!(world.blocks.len(), 3);
}

#[test]
fn external_write_failure_is_not_silently_reported_as_success_or_no_op() {
    let mut world = World::new();
    world.fail_y = Some(6);
    let feature = FeatureDefinition::External(Arc::new(Column));
    let error = feature
        .place(&mut world, &mut LegacyRandom::new(0), [-5, 0, 12], blocks())
        .unwrap_err();
    assert!(
        matches!(error,FeatureExecutionError::Query(PlacementQueryError::Failed(message)) if message=="write refused")
    );
    assert_eq!(world.blocks.get(&[-5, 5, 12]), Some(&BlockStateId(5)));
    assert_eq!(world.blocks.get(&[-5, 6, 12]), None);
}

#[test]
fn native_feature_owners_follow_the_last_graph_clone() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<FeatureDefinition>();
    let owner = Arc::new(Column);
    let weak = Arc::downgrade(&owner);
    let feature = FeatureDefinition::External(owner);
    let retained = feature.clone();
    drop(feature);
    assert!(weak.upgrade().is_some());
    drop(retained);
    assert!(weak.upgrade().is_none());
}
