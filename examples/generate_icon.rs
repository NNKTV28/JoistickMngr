//! Writes `assets/app.ico` from the logo renderer used by the application.
//! Run with `cargo run --example generate_icon` after changing the logo.

#[path = "../src/app_icon.rs"]
mod app_icon;

use std::io::Write;

const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
const ICON_DIRECTORY_HEADER_SIZE: u32 = 6;
const ICON_DIRECTORY_ENTRY_SIZE: u32 = 16;
const BITMAP_INFO_HEADER_SIZE: u32 = 40;

fn main() -> std::io::Result<()> {
    let encoded_images: Vec<Vec<u8>> = ICON_SIZES.iter().map(|size| encode_bitmap_image(*size)).collect();
    let mut icon_file = std::fs::File::create("assets/app.ico")?;
    icon_file.write_all(&build_icon_directory(&encoded_images))?;
    for image in &encoded_images {
        icon_file.write_all(image)?;
    }
    println!("assets/app.ico written with {} sizes", ICON_SIZES.len());
    Ok(())
}

fn build_icon_directory(encoded_images: &[Vec<u8>]) -> Vec<u8> {
    let mut directory = Vec::new();
    directory.extend_from_slice(&0u16.to_le_bytes());
    directory.extend_from_slice(&1u16.to_le_bytes());
    directory.extend_from_slice(&(encoded_images.len() as u16).to_le_bytes());

    let mut image_offset = ICON_DIRECTORY_HEADER_SIZE + ICON_DIRECTORY_ENTRY_SIZE * encoded_images.len() as u32;
    for (size, image) in ICON_SIZES.iter().zip(encoded_images) {
        let size_byte = if *size >= 256 { 0 } else { *size as u8 };
        directory.extend_from_slice(&[size_byte, size_byte, 0, 0]);
        directory.extend_from_slice(&1u16.to_le_bytes());
        directory.extend_from_slice(&32u16.to_le_bytes());
        directory.extend_from_slice(&(image.len() as u32).to_le_bytes());
        directory.extend_from_slice(&image_offset.to_le_bytes());
        image_offset += image.len() as u32;
    }
    directory
}

fn encode_bitmap_image(size: u32) -> Vec<u8> {
    let rgba_pixels = app_icon::render_logo_rgba(size);
    let mask_row_bytes = size.div_ceil(32) * 4;
    let mut image = Vec::new();

    image.extend_from_slice(&BITMAP_INFO_HEADER_SIZE.to_le_bytes());
    image.extend_from_slice(&(size as i32).to_le_bytes());
    image.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    image.extend_from_slice(&1u16.to_le_bytes());
    image.extend_from_slice(&32u16.to_le_bytes());
    image.extend_from_slice(&[0u8; 24]);

    for row in (0..size).rev() {
        for column in 0..size {
            let pixel_start = ((row * size + column) * 4) as usize;
            let rgba = &rgba_pixels[pixel_start..pixel_start + 4];
            let bgra = [rgba[2], rgba[1], rgba[0], rgba[3]];
            image.extend_from_slice(&bgra);
        }
    }
    image.extend(std::iter::repeat_n(0u8, (mask_row_bytes * size) as usize));
    image
}
