//! A level stem naming a biome source a mod registered, as the noise generator reads it: the
//! codec takes it by id, its possible biomes are the mod's list (what structures and decoration
//! read), and its biome at a quart cell is the mod's answer for the world's seed. No other test
//! of this binary installs the process-wide table.
use horizon_world::worldgen::{
    biome_source::{BiomeSourceCatalog, BiomeSourceDefinition, ResolvedBiomeSource},
    mod_biome_sources::{
        ModBiomeMapper, ModBiomeMapperType, ModBiomeMapperTypes, ModBiomeSource, ModBiomeSources,
        install_mod_biome_mapper_types, install_mod_biome_sources,
    },
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Desert, plains, desert again, then the badlands, along quart x from the seed.
struct Bands(Vec<String>);
impl ModBiomeSource for Bands {
    fn possible_biomes(&self) -> &[String] {
        &self.0
    }
    fn noise_biome(&self, [x, _, _]: [i32; 3], seed: i64) -> u16 {
        (i64::from(x) + seed).rem_euclid(4) as u16
    }
}

#[test]
fn the_noise_generators_codec_reads_a_registered_source() {
    let names = [
        "minecraft:desert",
        "minecraft:plains",
        "minecraft:desert",
        "minecraft:badlands",
    ];
    let bands = Bands(names.iter().map(|b| (*b).to_owned()).collect());
    let sources = ModBiomeSources::from([("test:bands".to_owned(), Arc::new(bands) as _)]);
    install_mod_biome_sources(sources).map_err(drop).unwrap();
    let world = [
        "minecraft:badlands",
        "minecraft:desert",
        "minecraft:ocean",
        "minecraft:plains",
    ];
    let catalog = BiomeSourceCatalog::new(
        &world.iter().map(|b| (*b).to_owned()).collect(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    let definition = catalog.decode(r#"{"type":"test:bands"}"#).unwrap();
    assert_eq!(
        definition,
        BiomeSourceDefinition::Registered("test:bands".into())
    );
    assert_eq!(
        catalog.encode(&definition).unwrap(),
        r#"{"type":"test:bands"}"#
    );

    let source = definition.resolve(&BTreeMap::new(), 3).unwrap();
    assert_eq!(
        source.possible_biomes(),
        BTreeSet::from(["minecraft:badlands", "minecraft:desert", "minecraft:plains"])
    );
    let ResolvedBiomeSource::Registered(level) = &source else {
        panic!("not the registered source: {source:?}");
    };
    // Seed 3 starts quart x 0 on the fourth band.
    let row: Vec<&str> = (0..4)
        .map(|x| level.noise_biome([x, 0, 0]).as_str())
        .collect();
    assert_eq!(
        row,
        [
            "minecraft:badlands",
            "minecraft:desert",
            "minecraft:plains",
            "minecraft:desert"
        ]
    );

    // A world without the badlands refuses the stem, naming the source and the biome.
    let catalog = BiomeSourceCatalog::new(
        &world[1..].iter().map(|b| (*b).to_owned()).collect(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        catalog
            .decode(r#"{"type":"test:bands"}"#)
            .unwrap_err()
            .to_string(),
        "biome source test:bands names the unknown biome minecraft:badlands"
    );

    // A source nobody registered is refused naming its id.
    assert_eq!(
        catalog
            .decode(r#"{"type":"test:gone"}"#)
            .unwrap_err()
            .to_string(),
        "unknown biome source test:gone"
    );
}

struct MapperFactory;
struct Mapper(String);

impl ModBiomeMapperType for MapperFactory {
    fn decode(&self, config: &str) -> Result<Arc<dyn ModBiomeMapper>, String> {
        let config: Value = serde_json::from_str(config).map_err(|error| error.to_string())?;
        let biome = config["replacement"]
            .as_str()
            .ok_or("missing replacement")?;
        Ok(Arc::new(Mapper(biome.to_owned())))
    }
}

impl ModBiomeMapper for Mapper {
    fn additional_biomes(&self) -> Vec<String> {
        vec![self.0.clone()]
    }

    fn map(&self, parent: &str, [x, _, _]: [i32; 3], seed: i64) -> Result<Option<u16>, String> {
        Ok((parent == "minecraft:plains" && (i64::from(x) ^ seed) & 1 == 1).then_some(0))
    }
}

fn install_mappers() {
    let _ = install_mod_biome_mapper_types(ModBiomeMapperTypes::from([(
        "test:mapper".to_owned(),
        Arc::new(MapperFactory) as _,
    )]));
}

fn mapped_source(replacement: &str) -> Value {
    json!({
        "type": "test:mapper",
        "base": {
            "type": "minecraft:multi_noise",
            "biomes": [{
                "parameters": {
                    "temperature": 0, "humidity": 0, "continentalness": 0,
                    "erosion": 0, "depth": 0, "weirdness": 0, "offset": 0
                },
                "biome": "minecraft:plains"
            }]
        },
        "config": { "replacement": replacement }
    })
}

fn mapper_catalog() -> BiomeSourceCatalog {
    install_mappers();
    BiomeSourceCatalog::new(
        &["minecraft:plains".to_owned(), "minecraft:desert".to_owned()].into(),
        &BTreeMap::new(),
        &BTreeSet::new(),
    )
    .unwrap()
}

#[test]
fn mapper_load_rejects_bad_settings_and_unknown_additions() {
    let catalog = mapper_catalog();
    let mut source = mapped_source("test:unknown");
    let error = catalog
        .decode(&source.to_string())
        .unwrap()
        .resolve(&BTreeMap::new(), 0)
        .unwrap_err();
    assert!(error.to_string().contains("unknown biome test:unknown"));
    source["config"] = json!({ "replacement": false });
    let error = catalog
        .decode(&source.to_string())
        .unwrap()
        .resolve(&BTreeMap::new(), 0)
        .unwrap_err();
    assert!(error.to_string().contains("missing replacement"));
    source["base"] = mapped_source("minecraft:desert");
    let error = catalog.decode(&source.to_string()).unwrap_err();
    assert!(error.to_string().contains("nested biome mapper"));
}

#[test]
fn configured_mapper_preserves_parent_and_uses_prepared_climate() {
    use horizon_core::density::{ConstantSampler, DensitySampler, SamplerContext};
    use horizon_world::worldgen::climate::ClimateSampler;

    let catalog = mapper_catalog();
    let definition = catalog
        .decode(&mapped_source("minecraft:desert").to_string())
        .unwrap();
    assert_eq!(
        catalog
            .decode(&catalog.encode(&definition).unwrap())
            .unwrap(),
        definition
    );
    let source = definition.resolve(&BTreeMap::new(), 77).unwrap();
    assert_eq!(
        source.possible_biomes(),
        ["minecraft:desert", "minecraft:plains"].into()
    );
    let climate = ClimateSampler::new(std::array::from_fn(|_| {
        Arc::new(ConstantSampler(0.)) as Arc<dyn DensitySampler>
    }));
    let mut resolver = source.resolver(&climate, &ConstantSampler(0.));
    let mut context = SamplerContext::default();
    let volume = resolver
        .prepare_volume(&mut context, [-1, 0, 0], [2, 1, 1])
        .unwrap()
        .expect("multi-noise prepared volume");
    for (x, expected) in [(-1, "minecraft:plains"), (0, "minecraft:desert")] {
        let quart = [x, 0, 0];
        assert_eq!(resolver.sample(&mut context, quart).unwrap(), expected);
        assert_eq!(
            resolver
                .sample_prepared(&mut context, Some(&volume), quart)
                .unwrap(),
            expected
        );
    }
}

#[test]
#[ignore = "daily"]
fn biome_mapper_reads_each_saved_stem_in_real_chunk_generation() {
    use horizon_core::position::ChunkPos;
    use horizon_world::{
        level::data::load_registry,
        worldgen::generator::{GeneratorData, OverworldGenerator},
    };
    use std::path::PathBuf;

    install_mappers();
    let root = std::env::var_os("HORIZON_TEST_RESOURCES")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/horizon-client/26.3")
        });
    let registry = Box::leak(Box::new(load_registry(&root).unwrap()));
    let data = GeneratorData::load(&root.join("worldgen"), registry).unwrap();
    let make_world = |replacement: &str| {
        let stem = json!({
            "type": "minecraft:overworld",
            "generator": {
                "type": "minecraft:noise", "settings": "minecraft:overworld",
                "biome_source": mapped_source(replacement)
            }
        });
        let saved = serde_json::to_vec(&stem).unwrap();
        OverworldGenerator::for_world_with_settings(
            data.clone(),
            77,
            "minecraft:normal",
            "minecraft:overworld",
            false,
            Some(&serde_json::from_slice(&saved).unwrap()),
            None,
        )
        .unwrap()
    };
    let desert = make_world("minecraft:desert");
    let badlands = make_world("minecraft:badlands");
    for (world, expected) in [
        (desert, "minecraft:desert"),
        (badlands, "minecraft:badlands"),
        (make_world("minecraft:desert"), "minecraft:desert"),
    ] {
        let chunk = world.worker().generate_proto(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(chunk.biome_at(0, 64, 0), expected);
        assert_eq!(chunk.biome_at(4, 64, 0), "minecraft:plains");
        println!("saved mapper {expected}: chunk 0,0 has {expected} at x=0 and plains at x=4");
    }
}
