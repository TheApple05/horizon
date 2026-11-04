use horizon_client_lib::save::{
    self, WorldGen, WorldPreset,
    custom_presets::{self, CustomPreset},
};
use horizon_world::nbt::Tag;

fn string(value: &str) -> Tag {
    Tag::String(value.into())
}
fn stem() -> Result<Tag, String> {
    // Reentrant catalog reads must work: callbacks run outside its lock.
    assert!(custom_presets::by_id("example:terrain").is_some());
    Ok(Tag::compound([
        ("type", string("minecraft:overworld")),
        (
            "generator",
            Tag::compound([
                ("type", string("minecraft:noise")),
                ("settings", string("example:terrain_settings")),
            ]),
        ),
    ]))
}
fn recognizes(generator: &Tag) -> bool {
    assert!(custom_presets::by_id("example:terrain").is_some());
    generator.get("settings") == Some(&string("example:terrain_settings"))
}

/// This integration target has its own process/catalog, isolated from the
/// library's vanilla-only cycle tests and every other integration executable.
#[test]
fn a_registered_preset_cycles_writes_and_reopens_without_changing_vanilla() {
    assert_eq!(WorldPreset::all(), WorldPreset::ALL);
    assert_eq!(WorldPreset::SingleBiomeSurface.next(), WorldPreset::Normal);
    let registered = custom_presets::register(CustomPreset {
        id: "example:terrain",
        label: "Example terrain",
        overworld: stem,
        recognizes,
    })
    .unwrap();
    let custom = WorldPreset::Custom(registered);
    assert_eq!(WorldPreset::of_id("example:terrain"), Some(custom));
    assert_eq!(WorldPreset::SingleBiomeSurface.next(), custom);
    assert_eq!(custom.next(), WorldPreset::Normal);
    assert_eq!(WorldPreset::Normal.prev(), custom);
    assert_eq!(custom.prev(), WorldPreset::SingleBiomeSurface);
    for builtin in WorldPreset::ALL.into_iter().chain([WorldPreset::Debug]) {
        assert_eq!(WorldPreset::of_id(builtin.id()), Some(builtin));
    }
    let work = std::env::temp_dir().join(format!(
        "horizon-preset-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&work).unwrap();
    save::write_world_gen_settings(
        &work,
        8675309,
        WorldGen {
            preset: custom,
            structures: false,
            bonus_chest: true,
        },
    )
    .unwrap();
    let saved = save::world_gen_settings(&work).unwrap();
    assert_eq!(saved.get("seed"), Some(&Tag::Long(8675309)));
    assert_eq!(saved.get("generate_structures"), Some(&Tag::Byte(0)));
    assert_eq!(saved.get("bonus_chest"), Some(&Tag::Byte(1)));
    let dimensions = saved.get("dimensions").unwrap();
    let generator = dimensions
        .get("minecraft:overworld")
        .unwrap()
        .get("generator")
        .unwrap();
    assert!(recognizes(generator));
    assert_eq!(WorldPreset::of_generator(generator), custom);
    assert_eq!(WorldGen::read(&work).unwrap().preset, custom);
    for (dimension, settings) in [
        ("minecraft:the_nether", "minecraft:nether"),
        ("minecraft:the_end", "minecraft:end"),
    ] {
        assert_eq!(
            dimensions
                .get(dimension)
                .unwrap()
                .get("generator")
                .unwrap()
                .get("settings"),
            Some(&string(settings))
        );
    }
    let file = work.join("data/minecraft/world_gen_settings.dat");
    let before = std::fs::read(&file).unwrap();
    let bad = custom_presets::register(CustomPreset {
        id: "example:broken",
        label: "Broken",
        overworld: || Err("bad native config".into()),
        recognizes: |_| false,
    })
    .unwrap();
    assert!(
        save::write_world_gen_settings(
            &work,
            9,
            WorldGen {
                preset: WorldPreset::Custom(bad),
                ..WorldGen::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("bad native config")
    );
    assert_eq!(std::fs::read(&file).unwrap(), before);
    let malformed = custom_presets::register(CustomPreset {
        id: "example:malformed",
        label: "Malformed",
        overworld: || Ok(string("not a stem")),
        recognizes: |_| false,
    })
    .unwrap();
    assert!(
        save::write_world_gen_settings(
            &work,
            9,
            WorldGen {
                preset: WorldPreset::Custom(malformed),
                ..WorldGen::default()
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&file).unwrap(), before);
    custom_presets::register(CustomPreset {
        id: "example:greedy",
        label: "Greedy recognizer",
        overworld: stem,
        recognizes: |_| true,
    })
    .unwrap();
    for (settings, expected) in [
        ("minecraft:overworld", WorldPreset::Normal),
        ("minecraft:large_biomes", WorldPreset::LargeBiomes),
        ("minecraft:amplified", WorldPreset::Amplified),
    ] {
        let generator = Tag::compound([
            ("type", string("minecraft:noise")),
            ("settings", string(settings)),
        ]);
        assert_eq!(WorldPreset::of_generator(&generator), expected);
    }
    // This fresh tiny fixture contains only the test's own metadata. Leave it
    // for inspection rather than recursively deleting a computed Windows path.
}
