use horizon_client_lib::save::{
    self, WorldGen, WorldPreset,
    custom_presets::{self, CustomPreset, CustomPresetEditor, PresetSetting, SettingKind},
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

fn custom_stem(value: &serde_json::Value) -> Result<Tag, String> {
    if !value["scale"]
        .as_i64()
        .is_some_and(|n| (1..=4).contains(&n))
    {
        return Err("scale must be an integer in 1..4".into());
    }
    let mut stem = stem()?;
    let Some(Tag::Compound(generator)) = stem.get_mut("generator") else {
        unreachable!()
    };
    generator.insert("example:settings".into(), string(&value.to_string()));
    Ok(stem)
}
fn custom_settings(stem: &Tag) -> Result<Option<serde_json::Value>, String> {
    match stem
        .get("generator")
        .and_then(|g| g.get("example:settings"))
    {
        None => Ok(None),
        Some(Tag::String(text)) => {
            serde_json::from_str(&text.to_string().map_err(|e| e.to_string())?)
                .map(Some)
                .map_err(|e| e.to_string())
        }
        _ => Err("settings must be a string".into()),
    }
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
    custom_presets::register_editor(
        registered,
        CustomPresetEditor {
            fields: || {
                vec![PresetSetting {
                    path: "/scale",
                    label: "example.scale",
                    kind: SettingKind::Number {
                        min: 1.0,
                        max: 4.0,
                        step: 1.0,
                        integer: true,
                    },
                }]
            },
            defaults: || Ok(serde_json::json!({"scale":1})),
            overworld: custom_stem,
            settings: custom_settings,
        },
    )
    .unwrap();
    let settings = serde_json::json!({"scale":3});
    let configured = custom_presets::generate_custom(registered, &settings).unwrap();
    save::write_world_gen_settings_customized(
        &work,
        8675309,
        WorldGen {
            preset: custom,
            ..WorldGen::default()
        },
        None,
        None,
        Some(configured.clone()),
    )
    .unwrap();
    let saved = save::world_gen_settings(&work).unwrap();
    let reopened = saved
        .get("dimensions")
        .unwrap()
        .get("minecraft:overworld")
        .unwrap();
    assert_eq!(custom_settings(reopened).unwrap(), Some(settings));
    assert_eq!(WorldGen::read(&work).unwrap().preset, custom);
    let mut options = horizon_client_lib::title::WorldOptions {
        world_type: custom,
        custom_overworld: Some(configured.clone()),
        ..Default::default()
    };
    assert_eq!(options.custom_overworld(), Some(configured));
    options.world_type = WorldPreset::Normal;
    assert!(options.custom_overworld().is_none());
    let stable_bytes = std::fs::read(&file).unwrap();
    let mut broken = reopened.clone();
    let Some(Tag::Compound(generator)) = broken.get_mut("generator") else {
        unreachable!()
    };
    generator.insert("example:settings".into(), string("{"));
    assert!(
        save::write_world_gen_settings_customized(
            &work,
            1,
            WorldGen {
                preset: custom,
                ..WorldGen::default()
            },
            None,
            None,
            Some(broken)
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&file).unwrap(), stable_bytes);
    assert!(custom_presets::generate_custom(registered, &serde_json::json!({"scale":99})).is_err());
    assert!(
        custom_presets::generate_custom(
            registered,
            &serde_json::json!({"scale":3,"large":"x".repeat(60*1024)})
        )
        .is_err()
    );
    let other = work.join("other");
    save::write_world_gen_settings(&other, 17, WorldGen::default()).unwrap();
    assert_eq!(WorldGen::read(&other).unwrap().preset, WorldPreset::Normal);
    assert_eq!(std::fs::read(&file).unwrap(), stable_bytes);
    use horizon_client_lib::title::{Mode, Title, WorldOptions};
    let size = (640.0, 360.0);
    let mut title = Title::new(work.join("ui-saves"), &work);
    title.mode = Mode::Create {
        name: "Custom test".into(),
        seed: "9".into(),
        creative: false,
        focus: 2,
        options: WorldOptions {
            tab: 1,
            world_type: custom,
            ..Default::default()
        },
    };
    title.click(400.0, 65.0, size);
    assert!(matches!(&title.mode, Mode::CreateCustom { state, .. } if state.value["scale"]==1));
    title.tab_dir(false, size);
    title.arrow_key(winit::keyboard::KeyCode::ArrowRight, size, false);
    assert!(matches!(&title.mode, Mode::CreateCustom { state, .. } if state.value["scale"]==2));
    title.escape();
    assert!(
        matches!(&title.mode, Mode::Create { options, .. } if options.custom_overworld.is_none())
    );
    title.click(400.0, 65.0, size);
    title.tab_dir(false, size);
    title.arrow_key(winit::keyboard::KeyCode::ArrowRight, size, false);
    title.arrow_key(winit::keyboard::KeyCode::ArrowRight, size, false);
    title.click(240.0, 343.0, size);
    let Mode::Create { options, .. } = &title.mode else {
        panic!("Done must return to creation")
    };
    assert_eq!(
        custom_settings(options.custom_overworld.as_ref().unwrap()).unwrap(),
        Some(serde_json::json!({"scale":3}))
    );
    title.click(400.0, 65.0, size);
    assert!(matches!(&title.mode, Mode::CreateCustom { state, .. } if state.value["scale"]==3));
    title.click(320.0, 319.0, size);
    assert!(matches!(&title.mode, Mode::CreateCustom { state, .. } if state.value["scale"]==1));
    title.click(400.0, 343.0, size);
    let Mode::Create { options, .. } = &title.mode else {
        panic!("Cancel must return to creation")
    };
    assert_eq!(
        custom_settings(options.custom_overworld.as_ref().unwrap()).unwrap(),
        Some(serde_json::json!({"scale":3}))
    );
    title.click(400.0, 65.0, size);
    title.click(310.0, 52.0, size);
    title.release();
    assert!(matches!(&title.mode, Mode::CreateCustom { state, .. } if state.value["scale"]==4));
    title.escape();
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
