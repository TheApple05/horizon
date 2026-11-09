//! Built-in and optional native world-type catalog.
use super::{custom_presets, text};
use horizon_world::nbt::Tag;

/// The world presets of 26.3's `worldgen/world_preset` that the create
/// screen offers, in the order of `WorldPresetTags.NORMAL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldPreset {
    Normal,
    Flat,
    LargeBiomes,
    Amplified,
    SingleBiomeSurface,
    /// `minecraft:debug_all_block_states`: a grid of every block state
    /// (`DebugLevelSource`). The create screen does not offer it; a data
    /// pack, `level-type` or a saved world names it.
    Debug,
    /// A registered native preset; its metadata contains no world state.
    Custom(custom_presets::RegisteredPreset),
}
impl WorldPreset {
    /// Built-in choices, in world-type button order before native additions.
    pub const ALL: [Self; 5] = [
        Self::Normal,
        Self::Flat,
        Self::LargeBiomes,
        Self::Amplified,
        Self::SingleBiomeSurface,
    ];
    /// Built-ins followed by native registrations in their encounter order.
    pub fn all() -> Vec<Self> {
        Self::ALL
            .into_iter()
            .chain(custom_presets::presets().into_iter().map(Self::Custom))
            .collect()
    }
    /// The preset's registry id (`minecraft:normal`).
    pub const fn id(self) -> &'static str {
        match self {
            Self::Normal => "minecraft:normal",
            Self::Flat => "minecraft:flat",
            Self::LargeBiomes => "minecraft:large_biomes",
            Self::Amplified => "minecraft:amplified",
            Self::SingleBiomeSurface => "minecraft:single_biome_surface",
            Self::Debug => "minecraft:debug_all_block_states",
            Self::Custom(preset) => preset.id(),
        }
    }
    /// The preset an id names, if it is one of them.
    pub fn of_id(id: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .chain([Self::Debug])
            .find(|preset| preset.id() == id)
    }
    /// The language key the world-type button shows
    /// (`Identifier.toLanguageKey("generator")`).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "generator.minecraft.normal",
            Self::Flat => "generator.minecraft.flat",
            Self::LargeBiomes => "generator.minecraft.large_biomes",
            Self::Amplified => "generator.minecraft.amplified",
            Self::SingleBiomeSurface => "generator.minecraft.single_biome_surface",
            Self::Debug => "generator.minecraft.debug_all_block_states",
            Self::Custom(preset) => preset.label(),
        }
    }
    /// The preset the world-type button cycles to next.
    pub fn next(self) -> Self {
        let all = Self::all();
        let index = all.iter().position(|preset| *preset == self).unwrap_or(0);
        all[(index + 1) % all.len()]
    }
    /// The preset the world-type button cycles to previously.
    pub fn prev(self) -> Self {
        let all = Self::all();
        let index = all.iter().position(|preset| *preset == self).unwrap_or(0);
        all[(index + all.len() - 1) % all.len()]
    }
    /// The preset a saved overworld generator is: the noise settings name
    /// the preset, and the single-biome preset shares the overworld's noise
    /// settings, so its fixed biome source tells the two apart.
    pub fn of_generator(generator: &Tag) -> Self {
        if text(generator.get("type")).as_deref() == Some("minecraft:flat") {
            return Self::Flat;
        }
        if text(generator.get("type")).as_deref() == Some("minecraft:debug") {
            return Self::Debug;
        }
        let source = generator.get("biome_source");
        let fixed = text(source.and_then(|s| s.get("type"))).as_deref() == Some("minecraft:fixed");
        match text(generator.get("settings")).as_deref() {
            Some("minecraft:large_biomes") => Self::LargeBiomes,
            Some("minecraft:amplified") => Self::Amplified,
            Some("minecraft:overworld") if fixed => Self::SingleBiomeSurface,
            Some("minecraft:overworld") => Self::Normal,
            _ => custom_presets::recognize(generator).map_or(Self::Normal, Self::Custom),
        }
    }
}
