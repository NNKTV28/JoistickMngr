//! Product photos for the device cards.
//!
//! The photos belong to their manufacturers, so they are not shipped with the app. They are
//! downloaded once from the manufacturers' product pages and kept in the AppData folder; until then
//! the cards show a plain glow. Photos taken on a white background have that background removed so
//! every device sits on the dark cards the same way.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;

use crate::known_devices::DeviceKind;
use crate::storage::application_data_directory;

const IMAGE_FOLDER_NAME: &str = "product-images";
const LOGITECH_IMAGE_BASE_URL: &str =
    "https://resource.logitechg.com/w_640,c_fill,q_auto,f_png,dpr_1.0/d_transparent.gif/content/dam/gaming/en/products";
/// Channels at least this bright count as white background.
const WHITE_BACKGROUND_THRESHOLD: u8 = 236;

/// A device with a photo of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProductPhoto {
    X56Stick,
    X56Throttle,
    RudderPedals,
    LogitechG309,
    RazerKrakenV4X,
    HyperXQuadCastS,
    VietaProAstra,
    NewskillSerikeV2,
}

impl ProductPhoto {
    pub const ALL: [ProductPhoto; 8] = [
        ProductPhoto::X56Stick,
        ProductPhoto::X56Throttle,
        ProductPhoto::RudderPedals,
        ProductPhoto::LogitechG309,
        ProductPhoto::RazerKrakenV4X,
        ProductPhoto::HyperXQuadCastS,
        ProductPhoto::VietaProAstra,
        ProductPhoto::NewskillSerikeV2,
    ];

    pub fn of_device_kind(device_kind: DeviceKind) -> Self {
        match device_kind {
            DeviceKind::Joystick => ProductPhoto::X56Stick,
            DeviceKind::Throttle => ProductPhoto::X56Throttle,
            DeviceKind::RudderPedals => ProductPhoto::RudderPedals,
        }
    }

    /// The photo for a device Windows or the device itself names, such as "Razer Kraken V4 X".
    pub fn matching_name(device_name: &str) -> Option<Self> {
        let lowercase_name = device_name.to_lowercase();
        ProductPhoto::ALL.into_iter().find(|photo| {
            photo
                .name_marker()
                .is_some_and(|marker| lowercase_name.contains(marker))
        })
    }

    /// Part of the device's own name that identifies it.
    fn name_marker(self) -> Option<&'static str> {
        match self {
            ProductPhoto::X56Stick | ProductPhoto::X56Throttle | ProductPhoto::RudderPedals => None,
            ProductPhoto::LogitechG309 => Some("g309"),
            ProductPhoto::RazerKrakenV4X => Some("kraken v4 x"),
            ProductPhoto::HyperXQuadCastS => Some("quadcast s"),
            ProductPhoto::VietaProAstra => Some("vieta pro astra"),
            ProductPhoto::NewskillSerikeV2 => Some("serike v2"),
        }
    }

    fn image_url(self) -> String {
        match self {
            ProductPhoto::X56Stick => format!("{LOGITECH_IMAGE_BASE_URL}/x56/2025/gallery/x56-3qtr-left-angle-gallery-4.png"),
            ProductPhoto::X56Throttle => format!("{LOGITECH_IMAGE_BASE_URL}/x56/2025/gallery/x56-3qtr-left-angle-gallery-5.png"),
            ProductPhoto::RudderPedals => {
                format!("{LOGITECH_IMAGE_BASE_URL}/flight-rudder-pedals/flight-rudder-pedals-gallery-2.png")
            }
            ProductPhoto::LogitechG309 => format!(
                "{LOGITECH_IMAGE_BASE_URL}/G309-Lightspeed-Gaming-Mouse/gallery/g309-lightspeed-wireless-mouse-black-gallery-1.png"
            ),
            ProductPhoto::RazerKrakenV4X => "https://medias-p1.phoenix.razer.com/sys-master-phoenix-images-container/h94/hab/9806610366494/kraken-v4-x-500x500.png".to_owned(),
            ProductPhoto::HyperXQuadCastS => "https://hyperx.com/cdn/shop/files/hyperx_quadcast_s_1_front_f8ac522c-c5f8-4769-9890-17da065f6540_1024x1024.jpg".to_owned(),
            ProductPhoto::VietaProAstra => "https://assets.mmsrg.com/isr/166325/c1/-/ASSET_MMS_164295958/fee_786_587_png".to_owned(),
            ProductPhoto::NewskillSerikeV2 => {
                "https://cdn.shopify.com/s/files/1/0986/3498/9907/files/SERIKE_V2_1.png?width=800".to_owned()
            }
        }
    }

    fn cache_file_name(self) -> &'static str {
        match self {
            ProductPhoto::X56Stick => "x56-stick.png",
            ProductPhoto::X56Throttle => "x56-throttle.png",
            ProductPhoto::RudderPedals => "rudder-pedals.png",
            ProductPhoto::LogitechG309 => "logitech-g309.png",
            ProductPhoto::RazerKrakenV4X => "razer-kraken-v4-x.png",
            ProductPhoto::HyperXQuadCastS => "hyperx-quadcast-s.jpg",
            ProductPhoto::VietaProAstra => "vieta-pro-astra.png",
            ProductPhoto::NewskillSerikeV2 => "newskill-serike-v2.png",
        }
    }

    fn cached_image_path(self) -> PathBuf {
        application_data_directory()
            .join(IMAGE_FOLDER_NAME)
            .join(self.cache_file_name())
    }
}

/// Loads the photos in the background and turns them into textures on the UI thread when asked.
pub struct ProductImages {
    decoded_images: Arc<Mutex<HashMap<ProductPhoto, ColorImage>>>,
    textures: HashMap<ProductPhoto, TextureHandle>,
}

impl ProductImages {
    pub fn start_loading(egui_context: egui::Context) -> Self {
        let decoded_images = Arc::new(Mutex::new(HashMap::new()));
        let thread_images = Arc::clone(&decoded_images);
        std::thread::Builder::new()
            .name("product-images".into())
            .spawn(move || load_all_images(&thread_images, &egui_context))
            .expect("product image thread starts");
        Self {
            decoded_images,
            textures: HashMap::new(),
        }
    }

    /// The photo of a device, once it has been loaded.
    pub fn texture(&mut self, egui_context: &egui::Context, photo: ProductPhoto) -> Option<TextureHandle> {
        if let Some(texture) = self.textures.get(&photo) {
            return Some(texture.clone());
        }
        let decoded_image = self.decoded_images.lock().unwrap().remove(&photo)?;
        let texture = egui_context.load_texture(
            format!("product-image-{photo:?}"),
            decoded_image,
            TextureOptions::LINEAR,
        );
        self.textures.insert(photo, texture.clone());
        Some(texture)
    }
}

fn load_all_images(decoded_images: &Mutex<HashMap<ProductPhoto, ColorImage>>, egui_context: &egui::Context) {
    for photo in ProductPhoto::ALL {
        if let Ok(image) = load_image(photo) {
            decoded_images.lock().unwrap().insert(photo, image);
            egui_context.request_repaint();
        }
    }
}

fn load_image(photo: ProductPhoto) -> anyhow::Result<ColorImage> {
    let cache_path = photo.cached_image_path();
    if !cache_path.exists() {
        download_image(&photo.image_url(), &cache_path)?;
    }
    let mut rgba_image = decode_image(&std::fs::read(&cache_path)?)?;
    remove_white_background(&mut rgba_image);
    Ok(to_color_image(&rgba_image))
}

fn download_image(url: &str, destination: &Path) -> anyhow::Result<()> {
    let image_bytes = ureq::get(url)
        .header("User-Agent", "PerifeMngr (+https://nikicoding.com)")
        .call()?
        .body_mut()
        .read_to_vec()?;
    decode_image(&image_bytes)?;
    if let Some(folder) = destination.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(destination, image_bytes)?;
    Ok(())
}

fn decode_image(image_bytes: &[u8]) -> anyhow::Result<RgbaImage> {
    Ok(image::load_from_memory(image_bytes)?.to_rgba8())
}

fn to_color_image(rgba_image: &RgbaImage) -> ColorImage {
    let size = [rgba_image.width() as usize, rgba_image.height() as usize];
    ColorImage::from_rgba_unmultiplied(size, rgba_image.as_raw())
}

/// Makes the white backdrop around the product transparent. Starts from the image border and only
/// spreads through white pixels, so white parts inside the product itself stay.
fn remove_white_background(rgba_image: &mut RgbaImage) {
    let (width, height) = rgba_image.dimensions();
    let mut pixels_to_visit: Vec<(u32, u32)> = border_pixels(width, height).collect();
    while let Some((x, y)) = pixels_to_visit.pop() {
        let pixel = rgba_image.get_pixel_mut(x, y);
        if !is_white(pixel.0) {
            continue;
        }
        pixel.0 = [0, 0, 0, 0];
        pixels_to_visit.extend(neighbours(x, y, width, height));
    }
}

fn is_white(rgba: [u8; 4]) -> bool {
    rgba[3] > 0 && rgba[..3].iter().all(|channel| *channel >= WHITE_BACKGROUND_THRESHOLD)
}

fn border_pixels(width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let top_and_bottom = (0..width).flat_map(move |x| [(x, 0), (x, height - 1)]);
    let left_and_right = (0..height).flat_map(move |y| [(0, y), (width - 1, y)]);
    top_and_bottom.chain(left_and_right)
}

fn neighbours(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let candidates = [
        x.checked_sub(1).map(|left| (left, y)),
        (x + 1 < width).then_some((x + 1, y)),
        y.checked_sub(1).map(|up| (x, up)),
        (y + 1 < height).then_some((x, y + 1)),
    ];
    candidates.into_iter().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn white_border_becomes_transparent_but_white_inside_the_product_stays() {
        let white = Rgba([255, 255, 255, 255]);
        let black = Rgba([10, 10, 10, 255]);
        let mut image = RgbaImage::from_pixel(5, 5, white);
        for x in 1..4 {
            for y in 1..4 {
                image.put_pixel(x, y, black);
            }
        }
        image.put_pixel(2, 2, white);
        remove_white_background(&mut image);
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        assert_eq!(image.get_pixel(1, 1), &black);
        assert_eq!(image.get_pixel(2, 2), &white);
    }

    #[test]
    fn device_names_find_their_photo() {
        assert_eq!(
            ProductPhoto::matching_name("Razer Kraken V4 X"),
            Some(ProductPhoto::RazerKrakenV4X)
        );
        assert_eq!(
            ProductPhoto::matching_name("HyperX QuadCast S"),
            Some(ProductPhoto::HyperXQuadCastS)
        );
        assert_eq!(
            ProductPhoto::matching_name("G309 LIGHTSPEED"),
            Some(ProductPhoto::LogitechG309)
        );
        assert_eq!(ProductPhoto::matching_name("NVIDIA High Definition Audio"), None);
    }
}
