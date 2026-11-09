//! Source-only observation tool: input TSV rows `id<TAB>info|positive<TAB>x<TAB>z`.
//! No game resources, snapshots or expected values are read.
use horizon_world::worldgen::biome_temperature::biome_info_noise_f64;
use std::{error::Error, fs};
fn main() -> Result<(), Box<dyn Error>> {
    let text = fs::read_to_string(std::env::args_os().nth(1).ok_or("input TSV required")?)?;
    for line in text.lines() {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() < 4 {
            return Err("four-column input required".into());
        }
        if !matches!(f[1], "info" | "positive") {
            continue;
        }
        let (x, z): (i32, i32) = (f[2].parse()?, f[3].parse()?);
        let value = biome_info_noise_f64(f64::from(x) * 0.25, f64::from(z) * 0.25);
        let result = if f[1] == "info" {
            format!("{:016x}", value.to_bits())
        } else {
            (value > 0.0).to_string()
        };
        println!("{}\t{}", f[0], result);
    }
    Ok(())
}
