//! A small PNG encoder for the `computer` tool's screenshots (ADR-0039 section 2, slice 3): 8-bit RGB, no interlace,
//! one zlib stream over `miniz_oxide` (already in the tree through `flate2`), so screenshots add no crate.

/// The PNG file signature.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// Compression level: fast enough for a screenshot per step, small enough for the store's 512 KiB.
const LEVEL: u8 = 6;

/// The PNG of an RGB image, `width × height` pixels of 3 bytes each, row by row.
///
/// # Panics
/// Never for a well-formed image: `rgb` must hold exactly `width × height × 3` bytes, which the caller's raster keeps.
#[must_use]
pub fn encode_rgb(width: u32, height: u32, rgb: &[u8]) -> Vec<u8> {
    let row = width as usize * 3;
    assert_eq!(rgb.len(), row * height as usize, "an RGB raster's size");
    // Each row starts with its filter type; "Sub" suits screens' flat runs of colour.
    let mut filtered = Vec::with_capacity((row + 1) * height as usize);
    for line in rgb.chunks_exact(row.max(1)).take(height as usize) {
        filtered.push(1);
        for (index, byte) in line.iter().enumerate() {
            let left = if index >= 3 { line[index - 3] } else { 0 };
            filtered.push(byte.wrapping_sub(left));
        }
    }
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, colour type 2 (RGB), deflate, adaptive filtering, no interlace.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&filtered, LEVEL);
    let mut png = Vec::with_capacity(data.len() + 64);
    png.extend_from_slice(&SIGNATURE);
    chunk(&mut png, *b"IHDR", &header);
    chunk(&mut png, *b"IDAT", &data);
    chunk(&mut png, *b"IEND", &[]);
    png
}

fn chunk(png: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    png.extend_from_slice(&length.to_be_bytes());
    png.extend_from_slice(&kind);
    png.extend_from_slice(data);
    let mut crc = Crc32::default();
    crc.update(&kind);
    crc.update(data);
    png.extend_from_slice(&crc.finish().to_be_bytes());
}

/// CRC-32 (ISO 3309), as PNG's chunks use it.
struct Crc32 {
    value: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self { value: 0xffff_ffff }
    }
}

impl Crc32 {
    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.value ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = (self.value & 1).wrapping_neg();
                self.value = (self.value >> 1) ^ (0xedb8_8320 & mask);
            }
        }
    }

    const fn finish(&self) -> u32 {
        !self.value
    }
}

#[cfg(test)]
pub(super) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = Crc32::default();
    crc.update(bytes);
    crc.finish()
}
