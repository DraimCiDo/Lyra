//! Developer asset conversion; the distributed application never runs this helper.
use std::io::Write;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("assets/app.svg")?;
    let image = egui_extras::image::load_svg_bytes(&bytes, &Default::default())
        .map_err(std::io::Error::other)?;
    if image.size != [256, 256] {
        return Err("Application icon must render at 256×256".into());
    }
    let pixels: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|c| c.to_srgba_unmultiplied())
        .collect();
    std::io::stdout().lock().write_all(&pixels)?;
    Ok(())
}
