//! Original RomMFS cartridge mark, shared by executable resources and tray.
pub fn rgba(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (x, y) = (x * 32 / size, y * 32 / size);
            let color = if (4..28).contains(&x) && (3..27).contains(&y) {
                if (8..24).contains(&x) && (7..17).contains(&y) {
                    [28, 30, 46, 255]
                } else if (10..22).contains(&x) && (23..27).contains(&y) {
                    [226, 229, 243, 255]
                } else {
                    [96, 165, 250, 255]
                }
            } else {
                [0, 0, 0, 0]
            };
            pixels.extend_from_slice(&color);
        }
    }
    pixels
}

/// Uncompressed 32-bit Windows ICO, including its AND transparency mask.
#[allow(dead_code)]
pub fn ico(size: u32) -> Vec<u8> {
    let rgba = rgba(size);
    let mask_stride = size.div_ceil(32) * 4;
    let image_bytes = 40 + size * size * 4 + mask_stride * size;
    let mut icon = vec![0, 0, 1, 0, 1, 0, size as u8, size as u8, 0, 0, 1, 0, 32, 0];
    icon.extend_from_slice(&image_bytes.to_le_bytes());
    icon.extend_from_slice(&22u32.to_le_bytes());
    icon.extend_from_slice(&40u32.to_le_bytes());
    icon.extend_from_slice(&(size as i32).to_le_bytes());
    icon.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    icon.extend_from_slice(&1u16.to_le_bytes());
    icon.extend_from_slice(&32u16.to_le_bytes());
    icon.extend_from_slice(&[0; 24]);
    for y in (0..size).rev() {
        for x in 0..size {
            let start = ((y * size + x) * 4) as usize;
            let pixel = &rgba[start..start + 4];
            icon.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    for y in (0..size).rev() {
        let mut row = vec![0; mask_stride as usize];
        for x in 0..size {
            if rgba[((y * size + x) * 4 + 3) as usize] == 0 {
                row[(x / 8) as usize] |= 1 << (7 - x % 8);
            }
        }
        icon.extend_from_slice(&row);
    }
    icon
}
