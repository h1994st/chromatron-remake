//! `CHROMATRON_SNAPSHOT=path.png`: the first presented canvas as an RGB PNG, for checks without a
//! person looking at the screen.

use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

/// Writes `pixels` (`width × height`, row-major, `0x00RRGGBB`) as an 8-bit RGB PNG.
pub fn write_png(path: &Path, pixels: &[u32], width: usize, height: usize) -> io::Result<()> {
    assert_eq!(
        pixels.len(),
        width * height,
        "pixels must be width × height"
    );
    let mut encoder = png::Encoder::new(
        BufWriter::new(File::create(path)?),
        width as u32,
        height as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgb))
        .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_round_trip_through_the_png() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shot.png");
        write_png(&path, &[0xa4a4a4, 0x123456, 0x000000, 0xffffff], 2, 2).unwrap();
        let decoder = png::Decoder::new(io::BufReader::new(File::open(&path).unwrap()));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(
            &buf[..12],
            [
                0xa4, 0xa4, 0xa4, 0x12, 0x34, 0x56, 0, 0, 0, 0xff, 0xff, 0xff
            ]
        );
    }
}
