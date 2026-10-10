//! Product photos for the device cards.
//!
//! The photos belong to Logitech, so they are not shipped with the app. They are downloaded once from
//! Logitech's own product pages and kept in the AppData folder; until then the cards show a plain glow.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

use crate::known_devices::DeviceKind;
use crate::storage::application_data_directory;

const IMAGE_FOLDER_NAME: &str = "product-images";
const LOGITECH_IMAGE_BASE_URL: &str =
    "https://resource.logitechg.com/w_640,c_fill,q_auto,f_png,dpr_1.0/d_transparent.gif/content/dam/gaming/en/products";

fn image_url(device_kind: DeviceKind) -> String {
    let image_path = match device_kind {
        DeviceKind::Joystick => "x56/2025/gallery/x56-3qtr-left-angle-gallery-4.png",
        DeviceKind::Throttle => "x56/2025/gallery/x56-3qtr-left-angle-gallery-5.png",
        DeviceKind::RudderPedals => "flight-rudder-pedals/flight-rudder-pedals-gallery-2.png",
    };
    format!("{LOGITECH_IMAGE_BASE_URL}/{image_path}")
}

fn cached_image_path(device_kind: DeviceKind) -> PathBuf {
    let file_name = match device_kind {
        DeviceKind::Joystick => "x56-stick.png",
        DeviceKind::Throttle => "x56-throttle.png",
        DeviceKind::RudderPedals => "rudder-pedals.png",
    };
    application_data_directory().join(IMAGE_FOLDER_NAME).join(file_name)
}

/// Loads the photos in the background and turns them into textures on the UI thread when asked.
pub struct ProductImages {
    decoded_images: Arc<Mutex<HashMap<DeviceKind, ColorImage>>>,
    textures: HashMap<DeviceKind, TextureHandle>,
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
    pub fn texture(&mut self, egui_context: &egui::Context, device_kind: DeviceKind) -> Option<TextureHandle> {
        if let Some(texture) = self.textures.get(&device_kind) {
            return Some(texture.clone());
        }
        let decoded_image = self.decoded_images.lock().unwrap().remove(&device_kind)?;
        let texture = egui_context.load_texture(
            format!("product-image-{device_kind:?}"),
            decoded_image,
            TextureOptions::LINEAR,
        );
        self.textures.insert(device_kind, texture.clone());
        Some(texture)
    }
}

fn load_all_images(decoded_images: &Mutex<HashMap<DeviceKind, ColorImage>>, egui_context: &egui::Context) {
    for device_kind in DeviceKind::ALL {
        if let Ok(image) = load_image(device_kind) {
            decoded_images.lock().unwrap().insert(device_kind, image);
            egui_context.request_repaint();
        }
    }
}

fn load_image(device_kind: DeviceKind) -> anyhow::Result<ColorImage> {
    let cache_path = cached_image_path(device_kind);
    if !cache_path.exists() {
        download_image(&image_url(device_kind), &cache_path)?;
    }
    decode_png(&std::fs::read(&cache_path)?)
}

fn download_image(url: &str, destination: &Path) -> anyhow::Result<()> {
    let image_bytes = ureq::get(url)
        .header("User-Agent", "JoystickManager (+https://nikicoding.com)")
        .call()?
        .body_mut()
        .read_to_vec()?;
    decode_png(&image_bytes)?;
    if let Some(folder) = destination.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(destination, image_bytes)?;
    Ok(())
}

fn decode_png(image_bytes: &[u8]) -> anyhow::Result<ColorImage> {
    let rgba_image = image::load_from_memory(image_bytes)?.to_rgba8();
    let size = [rgba_image.width() as usize, rgba_image.height() as usize];
    Ok(ColorImage::from_rgba_unmultiplied(size, rgba_image.as_raw()))
}
