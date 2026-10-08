use std::{fs::File, io, io::BufWriter, path::Path};

/// Writes `pixels` (0x00RRGGBB, row-major) as an 8-bit RGB PNG.
pub fn write_rgb_png(path: &Path, width: u32, height: u32, pixels: &[u32]) -> io::Result<()> {
    assert_eq!(
        pixels.len(),
        width as usize * height as usize,
        "pixel count must be width × height"
    );
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    let mut writer = encoder.write_header().map_err(io::Error::other)?;
    writer.write_image_data(&rgb).map_err(io::Error::other)?;
    writer.finish().map_err(io::Error::other)
}
