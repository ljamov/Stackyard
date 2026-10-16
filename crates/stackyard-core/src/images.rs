//! Image optimizer: makes WebP and AVIF copies of JPG and PNG images, and
//! smaller JPGs and PNGs, for a project's files or a whole folder.
//!
//! Originals are kept unless asked otherwise. New files go next to each
//! original (`photo.jpg` gets `photo.webp`, `photo.avif`, `photo.min.jpg`)
//! or into another folder, keeping the folder layout.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const MODULE: &str = "images";

/// Folders that hold other people's files, never a project's own images.
const SKIPPED_FOLDERS: [&str; 5] = ["node_modules", "vendor", ".git", ".svn", "dist"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Webp,
    Avif,
    /// A smaller copy in the image's own format (JPG or PNG).
    Same,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub formats: Vec<Format>,
    /// 1 to 100, like a JPG quality setting.
    pub quality: u8,
    /// Wider images are scaled down to this width.
    pub max_width: Option<u32>,
    /// Where new files go; `None` puts them next to the originals.
    pub out_dir: Option<PathBuf>,
    /// For [`Format::Same`]: overwrite the original when the new file is
    /// smaller, instead of writing `name.min.jpg`.
    pub replace: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            formats: vec![Format::Webp, Format::Avif],
            quality: 80,
            max_width: None,
            out_dir: None,
            replace: false,
        }
    }
}

/// One image to work on, and the folder it was found in (kept as the
/// layout under [`Options::out_dir`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub base: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct Made {
    pub format: Format,
    pub path: PathBuf,
    pub size: u64,
    /// Why nothing was written, e.g. the copy would not be smaller.
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileResult {
    pub source: PathBuf,
    pub size: u64,
    pub made: Vec<Made>,
    pub error: Option<String>,
}

/// The JPG and PNG images in the given files and folders (searched all the
/// way down, past other people's folders like `node_modules`).
pub fn find(paths: &[PathBuf]) -> Vec<Source> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, path, &mut found);
        } else if is_image(path) {
            let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
            found.push(Source {
                path: path.clone(),
                base,
            });
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);
    found
}

fn walk(dir: &Path, base: &Path, found: &mut Vec<Source>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let name = entry.file_name();
            if !SKIPPED_FOLDERS.iter().any(|s| name == *s) {
                walk(&path, base, found);
            }
        } else if kind.is_file() && is_image(&path) {
            found.push(Source {
                path,
                base: base.to_owned(),
            });
        }
    }
}

/// A JPG or PNG that is not one of this module's own `.min` copies.
fn is_image(path: &Path) -> bool {
    let ext = extension(path);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase());
    matches!(ext.as_str(), "jpg" | "jpeg" | "png") && !stem.is_some_and(|s| s.ends_with(".min"))
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Where a new file for `source` goes.
pub fn output_path(source: &Source, format: Format, options: &Options) -> PathBuf {
    let dir = match &options.out_dir {
        Some(out) => {
            let relative = source
                .path
                .parent()
                .and_then(|p| p.strip_prefix(&source.base).ok())
                .unwrap_or(Path::new(""));
            out.join(relative)
        }
        None => source
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    };
    let stem = source
        .path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = extension(&source.path);
    match format {
        Format::Webp => dir.join(format!("{stem}.webp")),
        Format::Avif => dir.join(format!("{stem}.avif")),
        Format::Same if options.replace => source.path.clone(),
        Format::Same if options.out_dir.is_some() => dir.join(format!("{stem}.{ext}")),
        Format::Same => dir.join(format!("{stem}.min.{ext}")),
    }
}

/// Works through every image, reporting each one as it is done.
pub fn optimize_all(
    sources: &[Source],
    options: &Options,
    progress: &mut dyn FnMut(usize, &FileResult),
) -> Vec<FileResult> {
    let mut results = Vec::with_capacity(sources.len());
    for (i, source) in sources.iter().enumerate() {
        let result = optimize(source, options);
        progress(i + 1, &result);
        results.push(result);
    }
    results
}

pub fn optimize(source: &Source, options: &Options) -> FileResult {
    let size = file_size(&source.path);
    let mut result = FileResult {
        source: source.path.clone(),
        size,
        made: Vec::new(),
        error: None,
    };
    let image = match load(&source.path, options.max_width) {
        Ok(image) => image,
        Err(e) => {
            result.error = Some(e);
            return result;
        }
    };
    for &format in &options.formats {
        let path = output_path(source, format, options);
        match encode(&image, &source.path, format, options.quality) {
            Ok(bytes) => {
                let new_size = bytes.len() as u64;
                // A "smaller copy" that is not smaller is not worth a file.
                if format == Format::Same && new_size >= size {
                    result.made.push(Made {
                        format,
                        path,
                        size: new_size,
                        skipped: Some("already as small as it gets".into()),
                    });
                    continue;
                }
                if let Err(e) = write(&path, &bytes) {
                    result.error = Some(e);
                    continue;
                }
                result.made.push(Made {
                    format,
                    path,
                    size: new_size,
                    skipped: None,
                });
            }
            Err(e) => result.error = Some(e),
        }
    }
    result
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

fn write(path: &Path, bytes: &[u8]) -> std::result::Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Written beside it first, so a failed write never leaves half a file.
    let part = path.with_extension("part");
    std::fs::write(&part, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::rename(&part, path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("{}: {e}", path.display())
    })
}

/// Decodes the image the right way up (as phone cameras record it), scaled
/// down to `max_width` when it is wider.
fn load(path: &Path, max_width: Option<u32>) -> std::result::Result<image::DynamicImage, String> {
    use image::ImageDecoder;
    let fail = |e: &dyn std::fmt::Display| format!("could not read {}: {e}", path.display());
    let mut decoder = image::ImageReader::open(path)
        .map_err(|e| fail(&e))?
        .with_guessed_format()
        .map_err(|e| fail(&e))?
        .into_decoder()
        .map_err(|e| fail(&e))?;
    let orientation = decoder.orientation().map_err(|e| fail(&e))?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| fail(&e))?;
    image.apply_orientation(orientation);
    if let Some(max) = max_width.filter(|m| *m > 0 && image.width() > *m) {
        image = image.resize(max, u32::MAX, image::imageops::FilterType::Lanczos3);
    }
    Ok(image)
}

fn has_alpha(image: &image::DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|p| p[3] < 255)
}

fn encode(
    image: &image::DynamicImage,
    source: &Path,
    format: Format,
    quality: u8,
) -> std::result::Result<Vec<u8>, String> {
    let quality = quality.clamp(1, 100);
    let (width, height) = (image.width(), image.height());
    let alpha = has_alpha(image);
    match format {
        Format::Webp => {
            let encoded = if alpha {
                webp::Encoder::from_rgba(image.to_rgba8().as_raw(), width, height)
                    .encode(f32::from(quality))
            } else {
                webp::Encoder::from_rgb(image.to_rgb8().as_raw(), width, height)
                    .encode(f32::from(quality))
            };
            Ok(encoded.to_vec())
        }
        Format::Avif => {
            // AVIF looks as good as JPG or WebP at a lower setting.
            let q = (f32::from(quality) * 0.85).max(1.0);
            let encoder = ravif::Encoder::new()
                .with_quality(q)
                .with_alpha_quality(q)
                .with_speed(6);
            let (w, h) = (width as usize, height as usize);
            let encoded = if alpha {
                let pixels: Vec<ravif::RGBA8> = image
                    .to_rgba8()
                    .pixels()
                    .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
                    .collect();
                encoder.encode_rgba(ravif::Img::new(&pixels[..], w, h))
            } else {
                let pixels: Vec<ravif::RGB8> = image
                    .to_rgb8()
                    .pixels()
                    .map(|p| ravif::RGB8::new(p[0], p[1], p[2]))
                    .collect();
                encoder.encode_rgb(ravif::Img::new(&pixels[..], w, h))
            };
            encoded
                .map(|e| e.avif_file)
                .map_err(|e| format!("could not make AVIF: {e}"))
        }
        Format::Same if extension(source) == "png" => {
            let mut png = Vec::new();
            image
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|e| format!("could not make PNG: {e}"))?;
            let mut options = oxipng::Options::from_preset(2);
            options.strip = oxipng::StripChunks::Safe;
            oxipng::optimize_from_memory(&png, &options)
                .map_err(|e| format!("could not shrink PNG: {e}"))
        }
        Format::Same => {
            let mut jpg = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, quality)
                .encode_image(&image.to_rgb8())
                .map_err(|e| format!("could not make JPG: {e}"))?;
            Ok(jpg)
        }
    }
}

/// Checks options from the app or the terminal.
pub fn check(options: &Options) -> Result<()> {
    if options.formats.is_empty() {
        return Err(Error::Images(
            "pick at least one of WebP, AVIF or smaller JPG/PNG".into(),
        ));
    }
    if !(1..=100).contains(&options.quality) {
        return Err(Error::Images("quality goes from 1 to 100".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A photo-like image, with an alpha channel when asked.
    fn sample(path: &Path, width: u32, height: u32, alpha: bool) {
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            let v = ((x * 7 + y * 3) % 256) as u8;
            image::Rgba([
                v,
                (x % 256) as u8,
                (y % 256) as u8,
                if alpha && x < 8 { 0 } else { 255 },
            ])
        });
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        if extension(path) == "png" {
            image.save(path).unwrap();
        } else {
            image::DynamicImage::ImageRgba8(image)
                .to_rgb8()
                .save(path)
                .unwrap();
        }
    }

    #[test]
    fn finds_images_but_not_dependencies_or_own_copies() {
        let dir = crate::testutil::tempdir();
        for file in [
            "a.jpg",
            "img/b.PNG",
            "img/c.jpeg",
            "img/c.min.jpeg",
            "node_modules/x/d.png",
            "notes.txt",
        ] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let found: Vec<PathBuf> = find(std::slice::from_ref(&dir))
            .into_iter()
            .map(|s| s.path.strip_prefix(&dir).unwrap().to_path_buf())
            .collect();
        assert_eq!(
            found,
            [
                PathBuf::from("a.jpg"),
                Path::new("img").join("b.PNG"),
                Path::new("img").join("c.jpeg")
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_new_files() {
        let source = Source {
            path: PathBuf::from("/site/img/hero.jpg"),
            base: PathBuf::from("/site"),
        };
        let mut options = Options::default();
        assert_eq!(
            output_path(&source, Format::Webp, &options),
            PathBuf::from("/site/img/hero.webp")
        );
        assert_eq!(
            output_path(&source, Format::Same, &options),
            PathBuf::from("/site/img/hero.min.jpg")
        );
        options.out_dir = Some(PathBuf::from("/out"));
        assert_eq!(
            output_path(&source, Format::Avif, &options),
            PathBuf::from("/out/img/hero.avif")
        );
        assert_eq!(
            output_path(&source, Format::Same, &options),
            PathBuf::from("/out/img/hero.jpg")
        );
        options.out_dir = None;
        options.replace = true;
        assert_eq!(output_path(&source, Format::Same, &options), source.path);
    }

    #[test]
    fn makes_webp_avif_and_smaller_copies_and_keeps_originals() {
        let dir = crate::testutil::tempdir();
        let photo = dir.join("photo.png");
        sample(&photo, 300, 200, false);
        let logo = dir.join("logo.png");
        sample(&logo, 64, 64, true);
        let jpg = dir.join("pic.jpg");
        sample(&jpg, 120, 80, false);
        let before = std::fs::read(&photo).unwrap();
        let options = Options {
            formats: vec![Format::Webp, Format::Avif, Format::Same],
            max_width: Some(150),
            ..Options::default()
        };
        let mut seen = 0;
        let results = optimize_all(
            &find(std::slice::from_ref(&dir)),
            &options,
            &mut |done, _| seen = done,
        );
        assert_eq!(seen, 3);
        for result in &results {
            assert_eq!(result.error, None, "{result:?}");
        }
        assert_eq!(std::fs::read(&photo).unwrap(), before, "original changed");
        let webp = image::open(dir.join("photo.webp")).unwrap();
        assert_eq!((webp.width(), webp.height()), (150, 100), "scaled down");
        assert!(std::fs::read(dir.join("photo.avif")).unwrap().len() > 100);
        assert!(image::open(dir.join("logo.webp"))
            .unwrap()
            .color()
            .has_alpha());
        assert_eq!(&std::fs::read(dir.join("pic.avif")).unwrap()[4..8], b"ftyp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_image_is_reported_not_fatal() {
        let dir = crate::testutil::tempdir();
        std::fs::write(dir.join("broken.jpg"), "not an image").unwrap();
        let results = optimize_all(
            &find(std::slice::from_ref(&dir)),
            &Options::default(),
            &mut |_, _| {},
        );
        assert!(results[0]
            .error
            .as_deref()
            .unwrap()
            .contains("could not read"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checks_options() {
        assert!(check(&Options::default()).is_ok());
        assert!(check(&Options {
            formats: Vec::new(),
            ..Options::default()
        })
        .is_err());
        assert!(check(&Options {
            quality: 0,
            ..Options::default()
        })
        .is_err());
    }
}
