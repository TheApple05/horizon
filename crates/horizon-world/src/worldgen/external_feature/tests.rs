use super::{ExternalFeature, ExternalFeatureDecoder, ExternalFeatureWorld};
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
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

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

#[derive(Debug)]
struct Paint(Arc<u8>);
impl ExternalFeature for Paint {
    fn place(
        &self,
        world: &mut dyn ExternalFeatureWorld,
        origin: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        world.set_chunk_block(origin, BlockStateId(u32::from(*self.0)))?;
        Ok(true)
    }
}
#[derive(Debug)]
struct Decoder(Arc<u8>);
impl ExternalFeatureDecoder for Decoder {
    fn decode(
        &self,
        kind: &str,
        _: &str,
        _: &crate::worldgen::block_feature::FeatureBlockBindings<'_>,
    ) -> Option<Result<Arc<dyn ExternalFeature>, crate::worldgen::DecodeError>> {
        assert!(
            !kind.starts_with("minecraft:"),
            "built-in decoder priority was lost"
        );
        match kind {
            "example:paint" => Some(Ok(Arc::new(Paint(self.0.clone())))),
            "example:broken" => Some(Err(crate::worldgen::DecodeError::Field(
                "invalid native configuration",
            ))),
            _ => None,
        }
    }
}
fn registry() -> crate::registry::BlockRegistry {
    use crate::nbt::Tag;
    let names = [
        "minecraft:air",
        "minecraft:cave_air",
        "minecraft:void_air",
        "minecraft:stone",
    ];
    crate::registry::BlockRegistry::from_nbt(&Tag::compound([
        ("DataVersion", Tag::Int(crate::DATA_VERSION)),
        (
            "states",
            Tag::List(
                names
                    .iter()
                    .map(|name| Tag::compound([("id", Tag::String((*name).into()))]))
                    .collect(),
            ),
        ),
        (
            "defaults",
            Tag::Compound(
                names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| ((*name).into(), Tag::Int(i as i32)))
                    .collect(),
            ),
        ),
    ]))
    .unwrap()
}

#[test]
fn lazy_catalog_decodes_keep_their_own_world_binding_and_release_owners() {
    use crate::worldgen::{
        block_feature::FeatureBlockBindings, feature_catalog::FeatureCatalog, tags::TagDefinitions,
    };
    let registry = registry();
    let owner = Arc::new(5_u8);
    let weak = Arc::downgrade(&owner);
    let bindings = FeatureBlockBindings::new(&registry, BTreeSet::new())
        .unwrap()
        .with_external_decoder(Arc::new(Decoder(owner)));
    let other = FeatureBlockBindings::new(&registry, BTreeSet::new())
        .unwrap()
        .with_external_decoder(Arc::new(Decoder(Arc::new(9))));
    let catalog = FeatureCatalog::from_resources(
        &BTreeMap::from([("example:paint".into(), r#"{"type":"example:paint"}"#.into())]),
        &BTreeMap::from([(
            "example:root".into(),
            r#"{"feature":"example:paint","placement":[]}"#.into(),
        )]),
        &TagDefinitions::default(),
    )
    .unwrap();
    let first = catalog
        .placed_feature_with_blocks(r#""example:root""#, &bindings)
        .unwrap();
    let second = catalog
        .placed_feature_with_blocks(r#""example:root""#, &other)
        .unwrap();
    drop(bindings);
    assert!(weak.upgrade().is_some());
    let mut a = World::new();
    let mut b = World::new();
    first
        .place(&mut a, &mut LegacyRandom::new(0), [0, 0, 0], blocks())
        .unwrap();
    second
        .place(&mut b, &mut LegacyRandom::new(0), [0, 0, 0], blocks())
        .unwrap();
    assert_eq!(a.blocks.get(&[0, 0, 0]), Some(&BlockStateId(5)));
    assert_eq!(b.blocks.get(&[0, 0, 0]), Some(&BlockStateId(9)));
    drop(first);
    assert!(weak.upgrade().is_none());
}

#[test]
fn decoder_errors_and_unknown_types_remain_errors_and_builtins_keep_priority() {
    use crate::worldgen::block_feature::FeatureBlockBindings;
    let registry = registry();
    let bindings = FeatureBlockBindings::new(&registry, BTreeSet::new())
        .unwrap()
        .with_external_decoder(Arc::new(Decoder(Arc::new(5))));
    assert!(
        FeatureDefinition::from_json_with_blocks(r#"{"type":"example:broken"}"#, &bindings)
            .is_err()
    );
    assert!(
        FeatureDefinition::from_json_with_blocks(r#"{"type":"example:unknown"}"#, &bindings)
            .is_err()
    );
    assert!(FeatureDefinition::from_json(r#"{"type":"example:paint"}"#).is_err());
    assert!(matches!(
        FeatureDefinition::from_json_with_blocks(r#"{"type":"minecraft:no_op"}"#, &bindings)
            .unwrap(),
        FeatureDefinition::NoOp
    ));
}
