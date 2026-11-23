pub use world::biome_source::{BiomeMapper, BiomeMapperType, BiomeSource};
mod __private {
    /// The biome sources the mods registered, for the executable to hand to
    /// the engine before any world opens.
    pub use crate::world::biome_source::biome_source_declarations;
    pub use crate::world::biome_source::biome_mapper_type_declarations;
}
