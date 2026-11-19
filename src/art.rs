//! Album art: fetching, decoding, pre-blurring and color extraction.
//!
//! All heavy work runs on a worker thread. The UI polls [`ArtLoader::snapshot`] each frame.

use ::image::{RgbImage, imageops};
use iced::widget::image::Handle;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// On-screen size of the largest background copy we pre-blur for, in logical px.
/// (88% of a container 1.5× a ~1440px window.)
const REFERENCE_COPY_PX: f32 = 1900.0;
/// Background art saturation boost.
const ART_SATURATION: f32 = 1.35;
/// Ambient: fixed blur and saturation.
const AMBIENT_BLUR_PX: f32 = 90.0;
const AMBIENT_SATURATION: f32 = 1.3;
const AMBIENT_TEXTURE: u32 = 128;
const MAX_DOWNLOAD: u64 = 10 * 1024 * 1024;

/// The four ambient circles: center (x, y), diameter, all as fractions of the container.
const AMBIENT_CIRCLES: [(f32, f32, f32); 4] = [
    (0.32, 0.36, 0.52),
    (0.70, 0.30, 0.46),
    (0.62, 0.72, 0.44),
    (0.28, 0.72, 0.40),
];
/// Mockup colors; also used when there's no cover to take colors from.
pub const DEFAULT_AMBIENT: [[u8; 3]; 4] = [
    [0x3B, 0x3F, 0x92],
    [0x7A, 0x55, 0xA3],
    [0xF2, 0xA6, 0x7F],
    [0x2F, 0x6F, 0x8F],
];

/// Everything derived from one cover.
pub struct Art {
    /// 512px cover for Spotlight.
    pub cover: Handle,
    /// 128px cover for the header and Minimal mode.
    pub thumb: Handle,
    /// 96px round cover for the accent swatch (canvas images can't be clipped to a circle).
    pub swatch: Handle,
    /// 384px round cover for the center of the Radial style.
    pub disc: Handle,
    /// Suggested accent, if the cover has a suitable color.
    pub accent: Option<[u8; 3]>,
    /// Bar colors for the Album palette, dark to light.
    pub gradient: [[u8; 3]; 3],
    /// Ambient-mode texture built from the cover's colors.
    pub ambient: Handle,
}

/// The cover blurred and saturated for the Album art background.
pub struct Blurred {
    pub url: String,
    pub blur_px: u8,
    pub handle: Handle,
}

#[derive(Default)]
struct Shared {
    art: Option<Arc<Art>>,
    blurred: Option<Arc<Blurred>>,
}

enum Request {
    Load(Option<String>),
    Blur(u8),
}

pub struct ArtLoader {
    shared: Arc<Mutex<Shared>>,
    requests: Sender<Request>,
    url: Option<String>,
    blur_px: Option<u8>,
}

impl ArtLoader {
    pub fn start() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = mpsc::channel();
        let worker_shared = shared.clone();
        let _ = std::thread::Builder::new()
            .name("undavi-art".into())
            .spawn(move || worker(rx, worker_shared));
        Self {
            shared,
            requests: tx,
            url: None,
            blur_px: None,
        }
    }

    /// Ask for the art at `url` (or none). Cheap to call every frame; only changes are sent.
    pub fn want(&mut self, url: Option<&str>, blur_px: u8) {
        if self.url.as_deref() != url {
            self.url = url.map(str::to_string);
            let _ = self.requests.send(Request::Load(self.url.clone()));
        }
        if self.blur_px != Some(blur_px) {
            self.blur_px = Some(blur_px);
            let _ = self.requests.send(Request::Blur(blur_px));
        }
    }

    pub fn snapshot(&self) -> (Option<Arc<Art>>, Option<Arc<Blurred>>) {
        match self.shared.lock() {
            Ok(s) => (s.art.clone(), s.blurred.clone()),
            Err(_) => (None, None),
        }
    }
}

fn worker(rx: Receiver<Request>, shared: Arc<Mutex<Shared>>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent(crate::about::user_agent())
        .build()
        .into();
    let mut source: Option<(String, RgbImage)> = None;
    let mut blur_px = 80u8;

    while let Ok(first) = rx.recv() {
        // Coalesce bursts: only the latest load and blur matter.
        let mut load = None;
        let mut new_blur = None;
        for req in std::iter::once(first).chain(rx.try_iter()) {
            match req {
                Request::Load(url) => load = Some(url),
                Request::Blur(px) => new_blur = Some(px),
            }
        }
        if let Some(px) = new_blur {
            blur_px = px;
        }

        if let Some(url) = load {
            source = None;
            let art = url
                .as_ref()
                .and_then(|url| match fetch(&agent, url).and_then(decode) {
                    Ok(img) => {
                        let art = Art::from_image(&img);
                        source = Some((url.clone(), img));
                        Some(Arc::new(art))
                    }
                    Err(e) => {
                        eprintln!("undavi: album art unavailable ({url}): {e}");
                        None
                    }
                });
            if let Ok(mut s) = shared.lock() {
                s.art = art;
                s.blurred = None;
            }
        }

        if let Some((url, img)) = &source {
            let stale = shared
                .lock()
                .map(|s| {
                    s.blurred
                        .as_ref()
                        .is_none_or(|b| b.url != *url || b.blur_px != blur_px)
                })
                .unwrap_or(false);
            if stale {
                let handle = blur_cover(img, blur_px);
                if let Ok(mut s) = shared.lock() {
                    s.blurred = Some(Arc::new(Blurred {
                        url: url.clone(),
                        blur_px,
                        handle,
                    }));
                }
            }
        }
    }
}

fn fetch(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(percent_decode(path)).map_err(|e| e.to_string());
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
        return resp
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .read_to_vec()
            .map_err(|e| e.to_string());
    }
    Err("unsupported URL scheme".into())
}

fn decode(bytes: Vec<u8>) -> Result<RgbImage, String> {
    ::image::load_from_memory(&bytes)
        .map(|i| i.to_rgb8())
        .map_err(|e| e.to_string())
}

pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Art {
    pub fn from_image(img: &RgbImage) -> Self {
        let square = center_square(img);
        let cover = imageops::resize(&square, 512, 512, imageops::FilterType::Triangle);
        let thumb = imageops::resize(&square, 128, 128, imageops::FilterType::Triangle);
        let colors = dominant_colors(&square);
        let picked = pick_accent(&square);
        let accent = Some(picked);
        let gradient = album_gradient(&colors, picked);
        let ambient_colors = ambient_palette(&colors);
        let swatch = imageops::resize(&square, 96, 96, imageops::FilterType::Triangle);
        let disc = imageops::resize(&square, 384, 384, imageops::FilterType::Triangle);
        Self {
            disc: round_handle(&disc),
            swatch: round_handle(&swatch),
            cover: to_handle(&cover),
            thumb: to_handle(&thumb),
            accent,
            gradient,
            ambient: ambient_texture(&ambient_colors),
        }
    }
}

fn center_square(img: &RgbImage) -> RgbImage {
    let side = img.width().min(img.height()).max(1);
    let x = (img.width() - side) / 2;
    let y = (img.height() - side) / 2;
    imageops::crop_imm(img, x, y, side, side).to_image()
}

fn to_handle(img: &RgbImage) -> Handle {
    let rgba: Vec<u8> = img.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    Handle::from_rgba(img.width(), img.height(), rgba)
}

/// The image cut to an anti-aliased circle.
fn round_handle(img: &RgbImage) -> Handle {
    let r = img.width().min(img.height()) as f32 / 2.0;
    let rgba: Vec<u8> = img
        .enumerate_pixels()
        .flat_map(|(x, y, p)| {
            let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            [p[0], p[1], p[2], (a * 255.0).round() as u8]
        })
        .collect();
    Handle::from_rgba(img.width(), img.height(), rgba)
}

fn saturate(img: &mut RgbImage, amount: f32) {
    for p in img.pixels_mut() {
        let [r, g, b] = p.0.map(|c| c as f32);
        let gray = 0.299 * r + 0.587 * g + 0.114 * b;
        p.0 = [r, g, b].map(|c| (gray + (c - gray) * amount).round().clamp(0.0, 255.0) as u8);
    }
}

/// Blur once per track: shrink, gaussian-blur, saturate. Drawn later stretched with linear filtering.
pub fn blur_cover(img: &RgbImage, blur_px: u8) -> Handle {
    let square = center_square(img);
    // Pick a working size where the blur spans a couple of src pixels.
    let size = ((REFERENCE_COPY_PX * 2.0 / (blur_px.max(1) as f32)).round() as u32).clamp(64, 512);
    let mut small = imageops::resize(&square, size, size, imageops::FilterType::Triangle);
    let sigma = blur_px as f32 * size as f32 / REFERENCE_COPY_PX;
    if sigma >= 0.3 {
        small = imageops::blur(&small, sigma);
    }
    saturate(&mut small, ART_SATURATION);
    // Feather into a soft disc so overlapping copies blend (CSS blurs the composited
    // container; we blur each copy, so the edges need softening here).
    let half = size as f32 / 2.0;
    let rgba: Vec<u8> = small
        .enumerate_pixels()
        .flat_map(|(x, y, p)| {
            let d =
                ((x as f32 + 0.5 - half).powi(2) + (y as f32 + 0.5 - half).powi(2)).sqrt() / half;
            let t = ((d - 0.6) / 0.4).clamp(0.0, 1.0);
            let alpha = 1.0 - t * t * (3.0 - 2.0 * t);
            [p[0], p[1], p[2], (alpha * 255.0).round() as u8]
        })
        .collect();
    Handle::from_rgba(size, size, rgba)
}

// ---------------------------------------------------------------------------
// Colors

fn to_hsl(c: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|v| v as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (h, s, l)
}

/// Dominant colors via a small k-means, most common first.
pub fn dominant_colors(img: &RgbImage) -> Vec<([u8; 3], usize)> {
    const K: usize = 6;
    let small = imageops::resize(img, 48, 48, imageops::FilterType::Triangle);
    let px: Vec<[f32; 3]> = small.pixels().map(|p| p.0.map(|c| c as f32)).collect();
    if px.is_empty() {
        return Vec::new();
    }
    // Deterministic, spread-out seeds.
    let mut centers: Vec<[f32; 3]> = (0..K)
        .map(|i| px[(i * px.len()) / K + px.len() / (2 * K)])
        .collect();
    let mut assign = vec![0usize; px.len()];
    for _ in 0..10 {
        for (a, p) in assign.iter_mut().zip(&px) {
            *a = (0..K)
                .min_by(|&x, &y| dist2(p, &centers[x]).total_cmp(&dist2(p, &centers[y])))
                .unwrap_or(0);
        }
        let mut sums = [[0.0f32; 3]; K];
        let mut counts = [0usize; K];
        for (&a, p) in assign.iter().zip(&px) {
            for c in 0..3 {
                sums[a][c] += p[c];
            }
            counts[a] += 1;
        }
        for k in 0..K {
            if counts[k] > 0 {
                centers[k] = sums[k].map(|s| s / counts[k] as f32);
            }
        }
    }
    let mut counts = vec![0usize; K];
    for &a in &assign {
        counts[a] += 1;
    }
    let mut out: Vec<([u8; 3], usize)> = centers
        .iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(c, n)| (c.map(|v| v.round().clamp(0.0, 255.0) as u8), n))
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.1));
    out
}

fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn from_hsl(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r, g, b].map(|v| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Accent for black-and-white covers: matches them better than a hue would.
pub const NEUTRAL_ACCENT: [u8; 3] = [0xE4, 0xE4, 0xE7];
const HUE_BINS: usize = 36;

/// The cover's most prominent *vivid* hue, adjusted to a readable lightness.
///
/// Every pixel votes for its hue, weighted by its chroma (so grays, near-black and
/// near-white barely count). The winning hue's average color is then nudged to
/// 55–75% lightness and at least 45% saturation rather than being rejected.
/// Covers with almost no color get [`NEUTRAL_ACCENT`].
pub fn pick_accent(img: &RgbImage) -> [u8; 3] {
    let small = imageops::resize(img, 64, 64, imageops::FilterType::Triangle);
    let mut weight = [0.0f32; HUE_BINS];
    let mut sum = [[0.0f32; 3]; HUE_BINS];
    let mut colorful = 0usize;
    for p in small.pixels() {
        let (h, s, l) = to_hsl(p.0);
        let chroma = s * (1.0 - (2.0 * l - 1.0).abs());
        if chroma < 0.12 {
            continue;
        }
        colorful += 1;
        let bin = ((h / 360.0) * HUE_BINS as f32) as usize % HUE_BINS;
        let w = chroma.powf(1.5);
        weight[bin] += w;
        for c in 0..3 {
            sum[bin][c] += p.0[c] as f32 * w;
        }
    }
    // Less than 1% colorful pixels: treat the cover as black and white.
    if colorful * 100 < (small.width() * small.height()) as usize {
        return NEUTRAL_ACCENT;
    }
    // Merge each bin with its neighbors (hue wraps) so one hue split across bins still wins.
    let around = |b: usize| [(b + HUE_BINS - 1) % HUE_BINS, b, (b + 1) % HUE_BINS];
    let score = |b: usize| around(b).iter().map(|&k| weight[k]).sum::<f32>();
    let best = (0..HUE_BINS)
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
        .unwrap_or(0);
    let (mut total, mut rgb) = (0.0f32, [0.0f32; 3]);
    for k in around(best) {
        total += weight[k];
        for c in 0..3 {
            rgb[c] += sum[k][c];
        }
    }
    let avg = rgb.map(|v| (v / total.max(1e-6)).round().clamp(0.0, 255.0) as u8);
    let (h, s, l) = to_hsl(avg);
    from_hsl(h, s.clamp(0.45, 0.9), l.clamp(0.55, 0.75))
}

/// Three bar colors from the cover: the accent plus the cover's other main colors,
/// nudged to a readable lightness and ordered dark to light. A cover with only one
/// main color gets a ramp around its accent instead.
pub fn album_gradient(colors: &[([u8; 3], usize)], accent: [u8; 3]) -> [[u8; 3]; 3] {
    let readable = |c: [u8; 3]| {
        let (h, s, l) = to_hsl(c);
        // Grays stay gray; colors get enough saturation to read as color.
        let s = if s < 0.12 { s } else { s.clamp(0.4, 0.9) };
        from_hsl(h, s, l.clamp(0.45, 0.8))
    };
    let distinct = |a: [u8; 3], b: [u8; 3]| {
        let d: f32 = (0..3).map(|k| (a[k] as f32 - b[k] as f32).powi(2)).sum();
        d.sqrt() > 70.0
    };
    // Common, colorful, mid-tone colors first (as for the ambient circles).
    let mut scored: Vec<([u8; 3], f32)> = colors
        .iter()
        .map(|&(c, n)| {
            let (_, s, l) = to_hsl(c);
            let tone = if (0.12..=0.88).contains(&l) {
                1.0
            } else {
                0.25
            };
            (c, n as f32 * (0.3 + s) * tone)
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));

    let mut picks = vec![accent];
    for (c, _) in scored {
        let c = readable(c);
        if picks.iter().all(|&p| distinct(p, c)) {
            picks.push(c);
            if picks.len() == 3 {
                break;
            }
        }
    }
    let lightness = |c: &[u8; 3]| to_hsl(*c).2;
    picks.sort_by(|a, b| lightness(a).total_cmp(&lightness(b)));
    match picks[..] {
        [a, b, c] => [a, b, c],
        [a, c] => [
            a,
            [0, 1, 2].map(|k| ((a[k] as u16 + c[k] as u16) / 2) as u8),
            c,
        ],
        _ => ramp(accent),
    }
}

/// Three stops around one color: a deeper, slightly shifted shade, the color, and a
/// lighter one. Used for "Match accent color" and covers with a single color.
pub fn ramp(c: [u8; 3]) -> [[u8; 3]; 3] {
    let (h, s, l) = to_hsl(c);
    [
        from_hsl(h - 16.0, s, (l - 0.2).max(0.28)),
        c,
        from_hsl(h + 16.0, s * 0.85, (l + 0.14).min(0.92)),
    ]
}

/// Four colors for the ambient circles, favoring colorful, common ones.
fn ambient_palette(colors: &[([u8; 3], usize)]) -> [[u8; 3]; 4] {
    let mut scored: Vec<([u8; 3], f32)> = colors
        .iter()
        .map(|(c, n)| {
            let (_, s, l) = to_hsl(*c);
            // Near-black and near-white make muddy or glaring blobs.
            let tone = if (0.12..=0.88).contains(&l) {
                1.0
            } else {
                0.25
            };
            (*c, *n as f32 * (0.3 + s) * tone)
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out = DEFAULT_AMBIENT;
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some((c, _)) = scored.get(i % scored.len().max(1)) {
            *slot = *c;
        }
    }
    out
}

/// Soft circles on a square texture standing in for the (square) ambient container.
pub fn ambient_texture(colors: &[[u8; 3]; 4]) -> Handle {
    let n = AMBIENT_TEXTURE;
    let base = [0x14u8, 0x14, 0x16];
    let mut img = RgbImage::from_pixel(n, n, ::image::Rgb(base));
    for ((cx, cy, d), color) in AMBIENT_CIRCLES.iter().zip(colors) {
        let (cx, cy, r) = (cx * n as f32, cy * n as f32, d * n as f32 / 2.0);
        for (x, y, p) in img.enumerate_pixels_mut() {
            let dist = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            // Slightly soft edge before the big blur.
            let a = ((r - dist) / 2.0 + 0.5).clamp(0.0, 1.0);
            if a > 0.0 {
                for c in 0..3 {
                    p.0[c] = (p.0[c] as f32 * (1.0 - a) + color[c] as f32 * a).round() as u8;
                }
            }
        }
    }
    // Container ≈ 2400px on screen for a 1440×900 window.
    let sigma = AMBIENT_BLUR_PX * n as f32 / 2400.0;
    let mut img = imageops::blur(&img, sigma);
    saturate(&mut img, AMBIENT_SATURATION);
    to_handle(&img)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_tone(a: [u8; 3], b: [u8; 3]) -> RgbImage {
        RgbImage::from_fn(64, 64, |x, _| ::image::Rgb(if x < 40 { a } else { b }))
    }

    fn hue_of(c: [u8; 3]) -> f32 {
        to_hsl(c).0
    }

    #[test]
    fn accent_prefers_vivid_over_dark() {
        // Dark navy covers more area, but the peach is far more vivid.
        let img = two_tone([0x10, 0x14, 0x30], [0xF2, 0xA6, 0x7F]);
        let accent = pick_accent(&img);
        let (h, _, l) = to_hsl(accent);
        assert!((0.55..=0.75).contains(&l));
        assert!((10.0..=30.0).contains(&h), "{accent:?} hue {h}");
    }

    #[test]
    fn dark_colorful_cover_is_lightened_not_rejected() {
        // Deep red at 25% lightness used to fall back to blue.
        let img = two_tone([0x80, 0x10, 0x10], [0x60, 0x08, 0x08]);
        let accent = pick_accent(&img);
        let (h, s, l) = to_hsl(accent);
        assert!(!(15.0..345.0).contains(&h), "should stay red: hue {h}");
        assert!(s >= 0.45 && (0.55..=0.75).contains(&l), "{accent:?}");
    }

    #[test]
    fn small_vivid_detail_on_grey_wins() {
        // 6% neon green on a gray cover.
        let img = RgbImage::from_fn(100, 100, |x, y| {
            ::image::Rgb(if x < 25 && y < 25 {
                [0x30, 0xF0, 0x60]
            } else {
                [0x80, 0x80, 0x84]
            })
        });
        let h = hue_of(pick_accent(&img));
        assert!((120.0..=150.0).contains(&h), "hue {h}");
    }

    #[test]
    fn black_and_white_cover_gets_neutral() {
        let img = two_tone([0x10, 0x10, 0x10], [0xF0, 0xF0, 0xF0]);
        assert_eq!(pick_accent(&img), NEUTRAL_ACCENT);
    }

    #[test]
    fn album_gradient_uses_the_covers_colors() {
        // Orange accent; the cover is mostly deep blue with some pink.
        let colors = [
            ([20, 40, 140], 900),
            ([230, 110, 160], 300),
            ([235, 140, 60], 200),
        ];
        let g = album_gradient(&colors, [235, 140, 60]);
        assert!(g.contains(&[235, 140, 60]), "{g:?}");
        // Ordered dark to light, and all three differ.
        let l: Vec<f32> = g.iter().map(|c| to_hsl(*c).2).collect();
        assert!(l[0] <= l[1] && l[1] <= l[2], "{l:?}");
        assert!(g[0] != g[1] && g[1] != g[2]);
        // The blue was lifted to a readable lightness, not left near-black.
        assert!(l[0] >= 0.44, "{l:?}");
    }

    #[test]
    fn single_color_cover_gets_a_ramp() {
        let colors = [([200, 60, 60], 1000)];
        let accent = [210, 70, 70];
        let g = album_gradient(&colors, accent);
        assert_eq!(g, ramp(accent));
        assert!(to_hsl(g[0]).2 < to_hsl(g[1]).2 && to_hsl(g[1]).2 < to_hsl(g[2]).2);
    }

    #[test]
    fn hsl_round_trip() {
        for c in [
            [0x4B, 0x7B, 0xF5],
            [0xF2, 0xA6, 0x7F],
            [0x3D, 0xD6, 0xB5],
            [0x80, 0x10, 0x10],
        ] {
            let (h, s, l) = to_hsl(c);
            let back = from_hsl(h, s, l);
            assert!(
                c.iter()
                    .zip(back)
                    .all(|(a, b)| (*a as i16 - b as i16).abs() <= 1),
                "{c:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn decodes_file_urls() {
        assert_eq!(percent_decode("/tmp/My%20Song.jpg"), "/tmp/My Song.jpg");
        assert_eq!(percent_decode("/a%2"), "/a%2");
    }
}
