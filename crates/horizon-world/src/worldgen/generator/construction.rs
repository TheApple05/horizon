//! World-bound pipeline construction and optional native extensions.
use super::*;

impl<R: RandomSource + 'static> Pipeline<R>
where
    R::Positional: Clone + Send + Sync,
{
    pub(super) fn new(
        data: Arc<GeneratorData>,
        seed: i64,
        random: R,
        generator: &Value,
        settings_json: &Value,
        with_structures: bool,
        level: Option<(i32, i32)>,
    ) -> Result<Self, GeneratorError> {
        Self::new_with_biome_mapper(
            data,
            seed,
            random,
            generator,
            settings_json,
            with_structures,
            level,
            None,
        )
    }

    /// Native source modules may supply a mapper before catalog construction.
    /// The ordinary constructor retains its unwrapped source and behavior.
    pub(super) fn new_with_biome_mapper(
        data: Arc<GeneratorData>,
        seed: i64,
        random: R,
        generator: &Value,
        settings_json: &Value,
        with_structures: bool,
        level: Option<(i32, i32)>,
        biome_mapper: Option<Arc<dyn crate::worldgen::biome_source::BiomeMapper>>,
    ) -> Result<Self, GeneratorError> {
        Self::new_with_extensions(
            data,
            seed,
            random,
            generator,
            settings_json,
            with_structures,
            level,
            biome_mapper,
            None,
        )
    }
    pub(super) fn new_with_extensions(
        data: Arc<GeneratorData>,
        seed: i64,
        mut random: R,
        generator: &Value,
        settings_json: &Value,
        with_structures: bool,
        level: Option<(i32, i32)>,
        biome_mapper: Option<Arc<dyn crate::worldgen::biome_source::BiomeMapper>>,
        decoder: Option<Arc<dyn crate::worldgen::external_feature::ExternalFeatureDecoder>>,
    ) -> Result<Self, GeneratorError> {
        let registry = data.registry;
        let noise = &settings_json["noise"];
        let min_y = noise["min_y"]
            .as_i64()
            .ok_or_else(|| resource("noise min_y"))? as i32;
        let height = noise["height"]
            .as_i64()
            .ok_or_else(|| resource("noise height"))? as i32;
        let (level_min_y, level_height) = level.unwrap_or((min_y, height));
        if min_y < level_min_y || min_y + height > level_min_y + level_height {
            return Err(resource("noise range outside the dimension's build height"));
        }
        let codec = crate::registry::BlockStateJsonCodec::new(registry, data.integers.clone())
            .map_err(|e| resource(e.to_string()))?;
        let settings = Settings {
            noise_min_y: min_y,
            noise_height: height,
            level_min_y,
            level_height,
            sea_level: settings_json["sea_level"]
                .as_i64()
                .ok_or_else(|| resource("sea_level"))? as i32,
            default_block: state(&codec, &settings_json["default_block"])?,
            default_fluid: state(&codec, &settings_json["default_fluid"])?,
            legacy: settings_json["legacy_random_source"]
                .as_bool()
                .unwrap_or(false),
            disable_mob_generation: settings_json["disable_mob_generation"]
                .as_bool()
                .unwrap_or(false),
        };
        let root = random.fork_positional();
        let mut densities = DensityRegistry::from_json(&data.noises, &data.densities)?;
        let router =
            NoiseRouter::from_json(&settings_json["noise_router"].to_string(), &mut densities)?;
        // NoiseGeneratorSettings.debugFunctions: a registered density or an
        // inline one, each with its label.
        let debug_functions = settings_json
            .get("debug_functions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| {
                let label = entry["label"]
                    .as_str()
                    .ok_or_else(|| resource("debug label"))?;
                let id = match &entry["function"] {
                    Value::String(name) => densities
                        .reference(name)
                        .ok_or_else(|| resource(format!("debug function {name}")))?,
                    inline => densities.decode_functions(&[&inline.to_string()])?[0],
                };
                Ok((label.to_owned(), id))
            })
            .collect::<Result<Vec<_>, GeneratorError>>()?;
        let aquifers = settings_json
            .get("aquifers")
            .filter(|v| !v.is_null())
            .map(|v| AquiferConfig::from_json(&v.to_string(), &mut densities))
            .transpose()?;
        let mut library = MaterialRuleLibrary::from_json(
            &data.rules,
            &data.conditions,
            &codec,
            densities,
            seed,
            settings.legacy,
        )?;
        let router = library.compile_router(router)?;
        let debug_density = debug_functions
            .into_iter()
            .map(|(label, id)| Ok((label, library.function_sampler(id)?)))
            .collect::<Result<Vec<_>, GeneratorError>>()?;
        let aquifer = aquifers
            .map(|config| library.compile_aquifer(config))
            .transpose()?
            .map(|samplers| AquiferField::ALL.map(|field| samplers.get(field).clone()));
        let rule_name = settings_json["material_rule"]
            .as_str()
            .ok_or_else(|| resource("material_rule"))?;
        let rule = library.rule(rule_name)?;
        let preliminary = router.get(RouterField::ChunkSurfaceLevel).clone();
        let spawn_target = match settings_json.get("spawn_target") {
            Some(Value::Array(points)) => points
                .iter()
                .map(|point| {
                    let Value::Object(fields) = point else {
                        return Err(resource("spawn_target point"));
                    };
                    let parameters = fields
                        .iter()
                        .map(|(name, interval)| {
                            Ok((
                                library.named_density(name)?,
                                decode_parameter(&interval.to_string())?,
                            ))
                        })
                        .collect::<Result<_, GeneratorError>>()?;
                    Ok(SpawnTargetPoint { parameters })
                })
                .collect::<Result<_, GeneratorError>>()?,
            _ => Vec::new(),
        };
        let source_json = generator["biome_source"].to_string();
        let source = data
            .source_catalog
            .decode(&source_json)?
            .resolve(&data.presets)
            .map_err(|e| resource(format!("biome source: {e}")))?;
        let source = match biome_mapper {
            Some(mapper) => {
                let mapped = source
                    .with_mapper(mapper)
                    .map_err(|e| resource(format!("biome mapper: {e}")))?;
                for name in mapped.possible_biomes() {
                    if data.biome_id(name).is_none() {
                        return Err(resource(format!("mapped biome has no definition: {name}")));
                    }
                }
                mapped
            }
            None => source,
        };
        let climate = router.climate_sampler();
        let mut noises = BTreeMap::new();
        for (name, json) in &data.noises {
            let noise: Arc<dyn Noise> = Arc::new(
                decode_noise(json)?
                    .create(&mut root.from_hash_of(name))
                    .map_err(DecodeError::from)?,
            );
            noises.insert(name.clone(), noise);
        }
        let noise = |name: &str| -> Result<Arc<dyn Noise>, GeneratorError> {
            noises
                .get(name)
                .cloned()
                .ok_or_else(|| resource(format!("missing noise {name}")))
        };
        let surface = MaterialSurface::new(
            root.clone(),
            ClayBandBlocks::from_registry(registry).ok_or_else(|| resource("clay bands"))?,
            noise("minecraft:clay_bands_offset")?,
            noise("minecraft:surface")?,
            noise("minecraft:surface_secondary")?,
        );
        let default = |name: &str| {
            registry
                .default_state(name)
                .ok_or_else(|| resource(format!("missing block {name}")))
        };
        let extensions = MaterialExtensions::new(
            root.clone(),
            ExtensionBlocks {
                default: settings.default_block,
                water: default("minecraft:water")?,
                packed_ice: default("minecraft:packed_ice")?,
                snow: default("minecraft:snow_block")?,
            },
            ExtensionNoises {
                badlands_surface: noise("minecraft:badlands_surface")?,
                badlands_pillar: noise("minecraft:badlands_pillar")?,
                badlands_roof: noise("minecraft:badlands_pillar_roof")?,
                iceberg_surface: noise("minecraft:iceberg_surface")?,
                iceberg_pillar: noise("minecraft:iceberg_pillar")?,
                iceberg_roof: noise("minecraft:iceberg_pillar_roof")?,
            },
            settings.sea_level,
        );
        let picker = GlobalFluidPicker::new(
            settings.sea_level,
            settings.default_fluid,
            default("minecraft:lava")?,
            default("minecraft:air")?,
        );
        let mut pipeline = Self {
            zoom: BiomeZoom::new(obfuscate_seed(seed)),
            data,
            seed,
            settings,
            root,
            router,
            climate,
            aquifer,
            rule,
            preliminary,
            source,
            surface,
            noises,
            extensions,
            picker,
            biome_cache: Mutex::new(HashMap::new()),
            lattice_cache: Mutex::new(HashMap::new()),
            biome_work: ChunkWork::new(),
            carver_cache: Mutex::new(HashMap::new()),
            decoration: None,
            structures: None,
            manual_structures: std::sync::OnceLock::new(),
            spawn_target,
            old_world: std::sync::OnceLock::new(),
            debug_density,
            spawn_position: std::sync::OnceLock::new(),
            flat: None,
            dimension: "minecraft:overworld".to_owned(),
        };
        pipeline.decoration = if let Some(decoder) = decoder {
            // The same source JSON can have another world's decoder/owners.
            // Retain these bindings locally, including lazy named features.
            crate::worldgen::generator_decoration::Decoration::new_with_decoder(
                &pipeline.data,
                &pipeline.source,
                None,
                Some(decoder),
            )?
            .map(Arc::new)
        } else {
            pipeline
                .data
                .decoration_for(&source_json, &pipeline.source)?
        };
        if with_structures {
            let possible = pipeline
                .source
                .possible_biomes()
                .into_iter()
                .filter_map(|name| pipeline.data.biome_id(name))
                .collect();
            let rings = pipeline.data.rings_for(seed, generator);
            pipeline.structures = crate::worldgen::generator_structures::StructureState::new(
                &pipeline.data,
                &possible,
                rings,
            )?;
            if let Some(state) = pipeline.structures.as_mut() {
                state.fixed_biome = generator["biome_source"]["type"] == "minecraft:fixed";
            }
            // ChunkGenerator.createState reads getOrigin: a noise generator
            // answers the chunk of its NoiseSpawnFinder position, others
            // chunk 0 0. Only a dimension-origin structure set reads it.
            if pipeline
                .structures
                .as_ref()
                .is_some_and(|state| state.needs_dimension_origin())
                && let Ok([x, z]) = pipeline.spawn_column()
            {
                let origin = ChunkPos::new(x >> 4, z >> 4);
                if let Some(state) = pipeline.structures.as_mut() {
                    state.set_dimension_origin(origin);
                }
            }
        }
        Ok(pipeline)
    }
}
