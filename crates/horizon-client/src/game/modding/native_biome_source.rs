//! Native biome source adapter: the mods' `BiomeSource`s as the engine's.
use horizon_world::worldgen::mod_biome_sources::{
    ModBiomeMapper, ModBiomeMapperType, ModBiomeMapperTypes, ModBiomeSource, ModBiomeSources,
    install_mod_biome_mapper_types, install_mod_biome_sources,
};
use mod_api::{BiomeMapper, BiomeMapperType, BiomeSource};
use std::sync::Arc;

/// A mod's source with the possible biomes it listed at registration.
struct Adapter {
    biomes: Vec<String>,
    source: Arc<dyn BiomeSource>,
}
impl ModBiomeSource for Adapter {
    fn possible_biomes(&self) -> &[String] {
        &self.biomes
    }
    fn noise_biome(&self, quart: [i32; 3], seed: i64) -> u16 {
        self.source.noise_biome(quart, seed)
    }
}

fn installed_biome_sources() -> ModBiomeSources {
    mod_api::__private::biome_source_declarations()
        .into_iter()
        .map(|(id, biomes, source)| {
            (
                id,
                Arc::new(Adapter { biomes, source }) as Arc<dyn ModBiomeSource>,
            )
        })
        .collect()
}

struct Mapper(Arc<dyn BiomeMapper>);
impl ModBiomeMapper for Mapper {
    fn additional_biomes(&self) -> Vec<String> {
        self.0.additional_biomes()
    }
    fn map(&self, parent: &str, quart: [i32; 3], seed: i64) -> Result<Option<u16>, String> {
        self.0.map(parent, quart, seed)
    }
}

struct MapperType(Arc<dyn BiomeMapperType>);
impl ModBiomeMapperType for MapperType {
    fn decode(&self, config: &str) -> Result<Arc<dyn ModBiomeMapper>, String> {
        self.0
            .decode(config)
            .map(|mapper| Arc::new(Mapper(mapper)) as Arc<dyn ModBiomeMapper>)
    }
}

fn installed_biome_mapper_types() -> ModBiomeMapperTypes {
    mod_api::__private::biome_mapper_type_declarations()
        .into_iter()
        .map(|(id, mapper)| (id, Arc::new(MapperType(mapper)) as Arc<dyn ModBiomeMapperType>))
        .collect()
}

/// Hand the biome sources the mods registered to the engine, before any world opens.
pub(in crate::game) fn install() {
    if install_mod_biome_sources(installed_biome_sources()).is_err() {
        log::warn!("Mod biome sources already installed; keeping the first table");
    }
    if install_mod_biome_mapper_types(installed_biome_mapper_types()).is_err() {
        log::warn!("Mod biome mapper types already installed; keeping the first table");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::api_values::install_parts;
    use mod_api::{DimensionSettings, Hooks, ModEntry, Registrar, RegistrationContext};

    /// Plains and desert in stripes one quart cell wide, shifted by the seed.
    struct Stripes;
    impl BiomeSource for Stripes {
        fn possible_biomes(&self) -> Vec<String> {
            vec!["minecraft:plains".into(), "minecraft:desert".into()]
        }
        fn noise_biome(&self, [x, _, _]: [i32; 3], seed: i64) -> u16 {
            ((i64::from(x) + seed) & 1) as u16
        }
    }

    struct MapperFactory;
    struct MapperFixture;

    impl BiomeMapperType for MapperFactory {
        fn decode(&self, config: &str) -> Result<Arc<dyn BiomeMapper>, String> {
            if config == r#"{"enabled":true}"# {
                Ok(Arc::new(MapperFixture))
            } else {
                Err("mapper is disabled".into())
            }
        }
    }

    impl BiomeMapper for MapperFixture {
        fn additional_biomes(&self) -> Vec<String> {
            vec!["minecraft:desert".into()]
        }

        fn map(&self, parent: &str, [x, _, _]: [i32; 3], seed: i64) -> Result<Option<u16>, String> {
            Ok((parent == "minecraft:plains" && i64::from(x) == seed).then_some(0))
        }
    }

    #[test]
    fn a_registered_mapper_type_reaches_the_engine() {
        struct Registers;
        #[mod_api::__subscriptions]
        impl Hooks for Registers {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                r.register_biome_mapper_type("mapper", MapperFactory);
            }
        }
        install_parts(&[ModEntry::new("test", Box::new(Registers))]).unwrap();
        let types = installed_biome_mapper_types();
        let factory = &types["test:mapper"];
        assert!(factory.decode(r#"{"enabled":false}"#).is_err());
        let mapper = factory.decode(r#"{"enabled":true}"#).unwrap();
        assert_eq!(mapper.additional_biomes(), ["minecraft:desert"]);
        assert_eq!(mapper.map("minecraft:plains", [7, 0, 0], 7).unwrap(), Some(0));
        assert_eq!(mapper.map("minecraft:forest", [7, 0, 0], 7).unwrap(), None);
    }

    /// The engine reads the mod's own code under its namespaced id, with the
    /// biomes it listed once, at registration.
    #[test]
    fn a_registered_source_reaches_the_engine_with_its_biomes_listed_once() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static LISTED: AtomicUsize = AtomicUsize::new(0);
        struct Counted;
        impl BiomeSource for Counted {
            fn possible_biomes(&self) -> Vec<String> {
                LISTED.fetch_add(1, Ordering::Relaxed);
                Stripes.possible_biomes()
            }
            fn noise_biome(&self, quart: [i32; 3], seed: i64) -> u16 {
                Stripes.noise_biome(quart, seed)
            }
        }
        struct Registers;
        #[mod_api::__subscriptions]
        impl Hooks for Registers {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                r.register_biome_source("stripes", Counted);
            }
        }
        install_parts(&[ModEntry::new("test", Box::new(Registers))]).unwrap();
        let sources = installed_biome_sources();
        let stripes = &sources["test:stripes"];
        for _ in 0..2 {
            assert_eq!(
                stripes.possible_biomes(),
                ["minecraft:plains", "minecraft:desert"]
            );
        }
        assert_eq!(LISTED.load(Ordering::Relaxed), 1);
        assert_eq!(stripes.noise_biome([3, 9, -4], 0), 1);
        assert_eq!(stripes.noise_biome([3, 9, -4], 1), 0);
    }

    /// Each source a mod may not register, and a dimension naming a source
    /// nobody registered: every one is refused at startup, naming the mod.
    #[test]
    fn invalid_biome_sources_refuse_registration() {
        struct Empty;
        impl BiomeSource for Empty {
            fn possible_biomes(&self) -> Vec<String> {
                Vec::new()
            }
            fn noise_biome(&self, _: [i32; 3], _: i64) -> u16 {
                0
            }
        }
        struct Invalid;
        #[mod_api::__subscriptions]
        impl Hooks for Invalid {
            fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
                r.register_biome_source("stripes", Stripes);
                r.register_biome_source("stripes", Stripes);
                r.register_biome_mapper_type("stripes", MapperFactory);
                r.register_biome_source("other:stripes", Stripes);
                r.register_biome_source("empty", Empty);
                r.register_dimension(
                    "pocket",
                    DimensionSettings {
                        mod_biome_source: Some("test:gone".into()),
                        ..Default::default()
                    },
                );
            }
        }
        let errors = install_parts(&[ModEntry::new("test", Box::new(Invalid))]).unwrap_err();
        let expected = [
            "test registered biome source test:stripes twice",
            "test registered biome source test:stripes twice",
            "test declares \"other:stripes\" in unowned namespace \"other\"",
            "test registered biome source test:empty, whose possible_biomes lists no biome",
            "test registered dimension test:pocket with the unknown biome source \"test:gone\"",
        ];
        assert_eq!(errors.len(), expected.len(), "{errors:?}");
        for (error, expected) in errors.iter().zip(expected) {
            assert!(error.contains(expected), "{error}");
        }
    }

    /// A wasm mod's registrar has only the dimension operations that cross to
    /// wasm; its registration of a biome source stops loading, naming the source.
    #[test]
    #[should_panic(expected = "biome source \"chaos\": a wasm mod cannot register a biome source")]
    fn a_wasm_mods_biome_source_is_refused_by_name() {
        struct Guest;
        impl mod_api::DimensionsRegistrar for Guest {
            fn add_dimension(&mut self, _: &str, _: DimensionSettings) {}
        }
        mod_api::DimensionsRegistrar::add_biome_source(&mut Guest, "chaos", Arc::new(Stripes));
    }
}
