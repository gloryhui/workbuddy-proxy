//! Headless decoder contract/performance probe; never shipped in the app.
use gpui_kit::{AssetSource, Image, ImageFormat, SharedString, SvgRenderer};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{borrow::Cow, path::PathBuf, sync::Arc, time::Instant};

struct EmptyAssets;
impl AssetSource for EmptyAssets {
    fn load(&self, _: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(None)
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

fn main() -> anyhow::Result<()> {
    let directory = std::env::args().nth(1).expect("fixture directory");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    paths.sort();
    let renderer = SvgRenderer::new(Arc::new(EmptyAssets));
    let mut rows = Vec::new();
    for path in paths {
        let format = match path.extension().and_then(|s| s.to_str()) {
            Some("png") => ImageFormat::Png,
            Some("jpg") => ImageFormat::Jpeg,
            Some("gif") => ImageFormat::Gif,
            Some("webp") => ImageFormat::Webp,
            Some("bmp") => ImageFormat::Bmp,
            Some("tiff") => ImageFormat::Tiff,
            Some("ico") => ImageFormat::Ico,
            Some("svg") => ImageFormat::Svg,
            Some("ppm") => ImageFormat::Pnm,
            _ => continue,
        };
        let bytes = std::fs::read(&path)?;
        let input_sha256 = format!("{:x}", Sha256::digest(&bytes));
        let image = Image::from_bytes(format, bytes);
        let decoded = image.to_image_data(renderer.clone())?;
        let mut digest = Sha256::new();
        let mut frames = Vec::new();
        for index in 0..decoded.frame_count() {
            let size = decoded.size(index);
            let delay = decoded.delay(index).numer_denom_ms();
            let pixels = decoded.as_bytes(index).expect("frame bytes");
            digest.update(pixels);
            frames.push(json!({"width": size.width.0, "height": size.height.0,
                               "delay_ms": delay, "bytes": pixels.len()}));
        }
        for _ in 0..5 {
            std::hint::black_box(image.to_image_data(renderer.clone())?);
        }
        let mut micros = Vec::new();
        for _ in 0..100 {
            let start = Instant::now();
            std::hint::black_box(image.to_image_data(renderer.clone())?);
            micros.push(start.elapsed().as_secs_f64() * 1_000_000.);
        }
        micros.sort_by(f64::total_cmp);
        rows.push(json!({"file": path.file_name().unwrap().to_string_lossy(),
                        "input_sha256": input_sha256,
                        "decoded_sha256": format!("{:x}", digest.finalize()),
                        "frames": frames, "iterations": micros.len(),
                        "p50_us": micros[micros.len() / 2],
                        "p95_us": micros[micros.len() * 95 / 100]}));
    }
    anyhow::ensure!(rows.len() >= 9, "need all nine exposed image formats");
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
