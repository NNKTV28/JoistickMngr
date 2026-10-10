//! Draws the application logo (an amber crosshair on a dark rounded tile) into an RGBA buffer,
//! so the window, the tray and the executable all share one icon without image files.

const TILE_COLOR: [f32; 3] = [17.0, 21.0, 26.0];
const ACCENT_COLOR: [f32; 3] = [242.0, 163.0, 58.0];
const SUBSAMPLES_PER_AXIS: u32 = 4;
const TILE_CORNER_RADIUS: f32 = 0.2;
const RING_RADIUS: f32 = 0.31;
const RING_HALF_THICKNESS: f32 = 0.045;
const CENTER_DOT_RADIUS: f32 = 0.085;
const TICK_HALF_WIDTH: f32 = 0.04;
const TICK_START: f32 = 0.16;
const TICK_END: f32 = 0.42;

/// Returns `size * size` pixels in RGBA order, top row first.
pub fn render_logo_rgba(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for pixel_y in 0..size {
        for pixel_x in 0..size {
            let coverage = measure_pixel_coverage(pixel_x, pixel_y, size);
            pixels.extend_from_slice(&shade_pixel(coverage));
        }
    }
    pixels
}

struct PixelCoverage {
    tile: f32,
    accent: f32,
}

fn measure_pixel_coverage(pixel_x: u32, pixel_y: u32, size: u32) -> PixelCoverage {
    let mut tile_hits = 0u32;
    let mut accent_hits = 0u32;
    for subsample_y in 0..SUBSAMPLES_PER_AXIS {
        for subsample_x in 0..SUBSAMPLES_PER_AXIS {
            let u = to_unit_coordinate(pixel_x, subsample_x, size);
            let v = to_unit_coordinate(pixel_y, subsample_y, size);
            if is_inside_rounded_tile(u, v) {
                tile_hits += 1;
                if is_on_crosshair(u, v) {
                    accent_hits += 1;
                }
            }
        }
    }
    let total_samples = (SUBSAMPLES_PER_AXIS * SUBSAMPLES_PER_AXIS) as f32;
    PixelCoverage {
        tile: tile_hits as f32 / total_samples,
        accent: accent_hits as f32 / total_samples,
    }
}

fn to_unit_coordinate(pixel: u32, subsample: u32, size: u32) -> f32 {
    let subsample_offset = (subsample as f32 + 0.5) / SUBSAMPLES_PER_AXIS as f32;
    (pixel as f32 + subsample_offset) / size as f32
}

fn is_inside_rounded_tile(u: f32, v: f32) -> bool {
    let inner_half_extent = 0.5 - TILE_CORNER_RADIUS;
    let overshoot_x = ((u - 0.5).abs() - inner_half_extent).max(0.0);
    let overshoot_y = ((v - 0.5).abs() - inner_half_extent).max(0.0);
    overshoot_x * overshoot_x + overshoot_y * overshoot_y <= TILE_CORNER_RADIUS * TILE_CORNER_RADIUS
}

fn is_on_crosshair(u: f32, v: f32) -> bool {
    let offset_x = u - 0.5;
    let offset_y = v - 0.5;
    let distance_from_center = (offset_x * offset_x + offset_y * offset_y).sqrt();
    let tick_range = TICK_START..=TICK_END;

    let is_on_ring = (distance_from_center - RING_RADIUS).abs() <= RING_HALF_THICKNESS;
    let is_on_center_dot = distance_from_center <= CENTER_DOT_RADIUS;
    let is_on_vertical_tick = offset_x.abs() <= TICK_HALF_WIDTH && tick_range.contains(&offset_y.abs());
    let is_on_horizontal_tick = offset_y.abs() <= TICK_HALF_WIDTH && tick_range.contains(&offset_x.abs());

    is_on_ring || is_on_center_dot || is_on_vertical_tick || is_on_horizontal_tick
}

fn shade_pixel(coverage: PixelCoverage) -> [u8; 4] {
    if coverage.tile == 0.0 {
        return [0, 0, 0, 0];
    }
    let accent_share = coverage.accent / coverage.tile;
    let mix_channel = |channel: usize| {
        let mixed = TILE_COLOR[channel] + (ACCENT_COLOR[channel] - TILE_COLOR[channel]) * accent_share;
        mixed.round() as u8
    };
    [
        mix_channel(0),
        mix_channel(1),
        mix_channel(2),
        (coverage.tile * 255.0).round() as u8,
    ]
}
