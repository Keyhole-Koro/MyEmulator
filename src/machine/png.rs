// Minimal PNG writer: 8-bit RGB, one IDAT holding a zlib stream of *stored*
// (uncompressed) deflate blocks. No image crate is vendored and stored blocks
// need neither a compressor nor a Huffman table -- just the block framing plus
// CRC-32 (chunks) and Adler-32 (zlib). Files are ~3 bytes/pixel; fine for a
// debugging screenshot, and every viewer opens them.
use std::io::Write;

fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    table
}

fn crc32(table: &[u32; 256], data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn write_chunk<W: Write>(out: &mut W, table: &[u32; 256], kind: &[u8; 4], body: &[u8]) -> std::io::Result<()> {
    out.write_all(&(body.len() as u32).to_be_bytes())?;
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.write_all(&crc_input)?;
    out.write_all(&crc32(table, &crc_input).to_be_bytes())
}

// zlib stream of stored deflate blocks (max 65535 bytes each).
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    z.extend_from_slice(&[0x78, 0x01]); // CM=8, no preset dict, fastest
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        z.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]); // one empty final block
    }
    while let Some(block) = blocks.next() {
        let last = blocks.peek().is_none();
        z.push(if last { 0x01 } else { 0x00 });
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    z
}

// Encode `pixels` (0x00RRGGBB, row-major, `width` per row) as a PNG.
pub fn write_png<W: Write>(out: &mut W, pixels: &[u32], width: usize, height: usize) -> std::io::Result<()> {
    let table = crc32_table();
    out.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])?;

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, no filter, no interlace
    write_chunk(out, &table, b"IHDR", &ihdr)?;

    // Each scanline is prefixed by its filter byte (0 = None).
    let mut raw = Vec::with_capacity(height * (1 + width * 3));
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            let p = pixels.get(y * width + x).copied().unwrap_or(0);
            raw.push(((p >> 16) & 0xFF) as u8);
            raw.push(((p >> 8) & 0xFF) as u8);
            raw.push((p & 0xFF) as u8);
        }
    }
    write_chunk(out, &table, b"IDAT", &zlib_stored(&raw))?;
    write_chunk(out, &table, b"IEND", &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_match_reference_values() {
        let table = crc32_table();
        assert_eq!(crc32(&table, b"123456789"), 0xCBF4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn png_has_signature_and_chunks() {
        let mut buf = Vec::new();
        write_png(&mut buf, &[0xFF0000, 0x00FF00, 0x0000FF, 0xFFFFFF], 2, 2).unwrap();
        assert_eq!(&buf[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert_eq!(&buf[12..16], b"IHDR");
        assert_eq!(&buf[buf.len() - 8..buf.len() - 4], b"IEND");
        // 2 rows * (1 filter byte + 2 px * 3) = 14 raw bytes, stored in one block.
        let idat_len = u32::from_be_bytes([buf[33], buf[34], buf[35], buf[36]]) as usize;
        assert_eq!(idat_len, 2 + 5 + 14 + 4);
    }
}
