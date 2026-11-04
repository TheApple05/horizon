use horizon_core::{
    density::{
        ConstantSampler, DensitySampler, GradientSampler, SampleError, SamplerContext, TilingMode,
    },
    geometry::Axis,
};
use horizon_world::worldgen::{
    biome::MultiNoiseBiomeSource,
    biome_source::{BiomeMapper, BiomeSourceError, FixedBiomeSource, ResolvedBiomeSource},
    climate::ClimateSampler,
};
use std::sync::Arc;

#[derive(Debug)]
struct Variants(Vec<String>);
impl BiomeMapper for Variants {
    fn additional_biomes(&self) -> &[String] {
        &self.0
    }
    fn map<'a>(&'a self, parent: &'a str, quart: [i32; 3]) -> Result<&'a str, SampleError> {
        if quart[1] < 0 {
            return Ok(parent);
        }
        match quart[0] {
            8 => Ok("example:undeclared"),
            9 => Err(SampleError::External("mapper failed")),
            _ if parent == "minecraft:left" => Ok(&self.0[0]),
            _ => Ok(&self.0[1]),
        }
    }
}
fn mapper() -> Arc<Variants> {
    Arc::new(Variants(vec![
        "example:left_variant".into(),
        "example:right_variant".into(),
    ]))
}
fn source() -> ResolvedBiomeSource {
    ResolvedBiomeSource::MultiNoise(
        MultiNoiseBiomeSource::from_quantized_json(
            r#"[
        {"coordinates":[[-10,-10],[0,0],[0,0],[0,0],[0,0],[0,0]],"offset":0,"biome":"left"},
        {"coordinates":[[10,10],[0,0],[0,0],[0,0],[0,0],[0,0]],"offset":0,"biome":"right"}
    ]"#,
        )
        .unwrap(),
    )
}
fn climate() -> ClimateSampler {
    ClimateSampler::new(std::array::from_fn(|i| -> Arc<dyn DensitySampler> {
        if i == 0 {
            Arc::new(GradientSampler::new(
                Axis::X,
                TilingMode::ClampToEdge,
                -4,
                4,
                -0.001,
                0.001,
            ))
        } else {
            Arc::new(ConstantSampler(0.))
        }
    }))
}

#[test]
fn mapped_scalar_and_prepared_queries_preserve_parent_tie_state_and_cave_choice() {
    let mapped = source().with_mapper(mapper()).unwrap();
    assert_eq!(
        mapped.possible_biomes().into_iter().collect::<Vec<_>>(),
        [
            "example:left_variant",
            "example:right_variant",
            "minecraft:left",
            "minecraft:right"
        ]
    );
    let climate = climate();
    let erosion = ConstantSampler(0.);
    let mut resolver = mapped.resolver(&climate, &erosion);
    let mut context = SamplerContext::cached();
    assert_eq!(
        resolver.sample(&mut context, [1, 0, 0]).unwrap(),
        "example:right_variant"
    );
    let volume = resolver
        .prepare_volume(&mut context, [-1, 0, 0], [3, 2, 2])
        .unwrap()
        .unwrap();
    assert_eq!(
        resolver
            .sample_prepared(&mut context, Some(&volume), [0, 0, 0])
            .unwrap(),
        "example:right_variant"
    );
    assert_eq!(
        mapped
            .resolver(&climate, &erosion)
            .sample_prepared(&mut context, Some(&volume), [0, 0, 0])
            .unwrap(),
        "example:left_variant"
    );
    assert_eq!(
        resolver
            .sample_prepared(&mut context, Some(&volume), [-1, 1, 1])
            .unwrap(),
        "example:left_variant"
    );
    assert_eq!(
        resolver.sample(&mut context, [0, -1, 0]).unwrap(),
        "minecraft:left"
    );
    assert!(matches!(
        resolver.sample_prepared(&mut context, Some(&volume), [2, 0, 0]),
        Err(BiomeSourceError::Biome(_))
    ));
}

#[test]
fn mapper_failures_and_undeclared_results_propagate_and_next_query_recovers() {
    let original = ResolvedBiomeSource::Fixed(FixedBiomeSource {
        biome: "minecraft:left".into(),
    });
    let owner = mapper();
    let weak = Arc::downgrade(&owner);
    let mapped = original.with_mapper(owner).unwrap();
    let climate = climate();
    let erosion = ConstantSampler(0.);
    {
        let mut resolver = mapped.resolver(&climate, &erosion);
        let mut context = SamplerContext::cached();
        assert!(
            resolver
                .prepare_volume(&mut context, [0; 3], [2; 3])
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            resolver.sample(&mut context, [9, 0, 0]),
            Err(BiomeSourceError::Sample(SampleError::External(
                "mapper failed"
            )))
        ));
        assert!(
            matches!(resolver.sample_prepared(&mut context, None, [8, 0, 0]), Err(BiomeSourceError::UndeclaredMappedBiome(name)) if name == "example:undeclared")
        );
        assert_eq!(
            resolver
                .sample_prepared(&mut context, None, [0; 3])
                .unwrap(),
            "example:left_variant"
        );
    }
    assert!(weak.upgrade().is_some());
    drop(mapped);
    assert!(weak.upgrade().is_none());
}

#[test]
fn malformed_mapper_catalogs_are_rejected_before_sampling() {
    for name in ["UPPER:biome", "relative", "example:"] {
        assert!(
            source()
                .with_mapper(Arc::new(Variants(vec![name.into()])))
                .is_err()
        );
    }
    // An unwrapped source keeps its original biome catalog and search behavior.
    let source = source();
    let climate = climate();
    let erosion = ConstantSampler(0.);
    assert_eq!(source.possible_biomes().len(), 2);
    assert_eq!(
        source
            .resolver(&climate, &erosion)
            .sample(&mut SamplerContext::cached(), [1, 0, 0])
            .unwrap(),
        "minecraft:right"
    );
}
