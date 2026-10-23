//! Pictures of sites for the Sites screen and the WordPress view.
//!
//! A picture is a screenshot of the site's home page, taken by a Chromium
//! browser already on the computer (Edge, which every Windows has, Chrome,
//! Chromium or Brave) running headless, shrunk to a small JPEG and kept in
//! `cache/thumbnails`. Without such a browser a WordPress site shows its
//! theme's own `screenshot.png`. Nothing is downloaded for this.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use image::imageops::FilterType;
use image::ImageReader;

use crate::{Error, Result};

/// How big the kept pictures are, in pixels: 16:10, like the screenshot.
const WIDTH: u32 = 480;
const HEIGHT: u32 = 300;
/// A picture older than this is taken again when it is next shown.
pub const FRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// A browser that takes longer than this is stopped.
const TIMEOUT: Duration = Duration::from_secs(45);

pub fn dir(home: &Path) -> PathBuf {
    home.join("cache").join("thumbnails")
}

/// Where the picture of the site with `domain` is kept.
pub fn file(home: &Path, domain: &str) -> PathBuf {
    let safe: String = domain
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir(home).join(format!("{safe}.jpg"))
}

/// The kept picture, if there is one, and whether it is still fresh.
pub fn cached(home: &Path, domain: &str) -> Option<(PathBuf, bool)> {
    let path = file(home, domain);
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();
    Some((path, age < FRESH))
}

/// A Chromium browser on this computer: `STACKYARD_BROWSER` when set.
pub fn browser() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("STACKYARD_BROWSER") {
        return Some(PathBuf::from(path)).filter(|p| p.is_file());
    }
    let mut places: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let roots = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from);
        for root in roots {
            places.push(root.join(r"Microsoft\Edge\Application\msedge.exe"));
            places.push(root.join(r"Google\Chrome\Application\chrome.exe"));
            places.push(root.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"));
            places.push(root.join(r"Chromium\Application\chrome.exe"));
        }
    } else if cfg!(target_os = "macos") {
        for app in [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Chromium.app/Contents/MacOS/Chromium",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
        ] {
            places.push(Path::new("/Applications").join(app));
            if let Some(home) = dirs::home_dir() {
                places.push(home.join("Applications").join(app));
            }
        }
    } else {
        let path = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            for name in [
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "microsoft-edge",
                "brave-browser",
            ] {
                places.push(dir.join(name));
            }
        }
    }
    places.into_iter().find(|p| p.is_file())
}

/// Takes a screenshot of `url` with `browser` and keeps it as the
/// picture of `domain`. A page that comes out blank (still loading, or
/// white with next to nothing on it) is tried once more with longer to
/// load; if it is still blank no picture is kept, so the site's logo shows.
pub fn capture(home: &Path, browser: &Path, domain: &str, url: &str) -> Result<PathBuf> {
    let dir = dir(home);
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let dest = file(home, domain);
    let mut bytes = shoot(&dir, browser, url, 8_000)?;
    if is_blank(&bytes) {
        bytes = shoot(&dir, browser, url, 20_000)?;
        if is_blank(&bytes) {
            forget(home, domain);
            return Err(Error::Shell("the page came out blank".into()));
        }
    }
    save_small(&bytes, &dest)?;
    Ok(dest)
}

/// One headless screenshot of `url`, letting the page load for `budget_ms`.
fn shoot(dir: &Path, browser: &Path, url: &str, budget_ms: u32) -> Result<Vec<u8>> {
    let shot = dir.join(format!("{}.png", std::process::id()));
    let _ = std::fs::remove_file(&shot);
    let mut command = Command::new(browser);
    command
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--hide-scrollbars")
        .arg("--mute-audio")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        // Sites use Stackyard's own local certificate authority, which a
        // separate profile may not trust yet.
        .arg("--ignore-certificate-errors")
        .arg(format!("--user-data-dir={}", dir.join("profile").display()))
        .arg("--window-size=1280,800")
        .arg(format!("--virtual-time-budget={budget_ms}"))
        .arg(format!("--screenshot={}", shot.display()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Chromium refuses to run as root with its sandbox on (some Linux
    // setups and containers run Stackyard as root).
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        command.arg("--no-sandbox");
    }
    crate::platform::hide_console(&mut command);
    let mut child = command.spawn().map_err(|source| Error::Spawn {
        program: browser.display().to_string(),
        source,
    })?;
    let started = Instant::now();
    loop {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let bytes = std::fs::read(&shot).map_err(|source| Error::Io {
        path: shot.clone(),
        source,
    })?;
    let _ = std::fs::remove_file(&shot);
    Ok(bytes)
}

/// Whether a screenshot is one colour all over, give or take a few specks:
/// in a thumbnail that is only a white (or grey) box. Unreadable counts as blank.
pub fn is_blank(bytes: &[u8]) -> bool {
    let Ok(img) = image::load_from_memory(bytes) else {
        return true;
    };
    let img = img.to_rgb8();
    let Some(&first) = img.pixels().next() else {
        return true;
    };
    let differs = |p: &image::Rgb<u8>| {
        p.0.iter()
            .zip(first.0)
            .map(|(a, b)| a.abs_diff(b) as u32)
            .sum::<u32>()
            > 24
    };
    let other = img.pixels().filter(|p| differs(p)).count();
    // Under half a percent: a word or two of text on an empty page.
    other * 200 < img.pixels().len()
}

/// Shrinks an image (PNG, JPEG or WebP) to the picture size, cropping to
/// 16:10 from the top, and saves it as a JPEG.
pub fn save_small(bytes: &[u8], dest: &Path) -> Result<()> {
    let jpeg = small_jpeg(bytes, WIDTH, HEIGHT)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(dest, jpeg).map_err(|source| Error::Io {
        path: dest.to_path_buf(),
        source,
    })
}

/// `bytes` as a JPEG of `width` × `height`, cropped from the top so a page's
/// header stays in the picture.
pub fn small_jpeg(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let bad = |e: String| Error::Shell(format!("could not read the picture: {e}"));
    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| bad(e.to_string()))?
        .decode()
        .map_err(|e| bad(e.to_string()))?;
    let scale = (width as f32 / img.width() as f32).max(height as f32 / img.height() as f32);
    let resized = img.resize_exact(
        ((img.width() as f32 * scale).ceil() as u32).max(width),
        ((img.height() as f32 * scale).ceil() as u32).max(height),
        FilterType::Triangle,
    );
    let x = (resized.width() - width) / 2;
    let cropped = resized.crop_imm(x, 0, width, height).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&cropped)
        .map_err(|e| bad(e.to_string()))?;
    Ok(out)
}

/// Forgets a site's picture, e.g. when the site is removed.
pub fn forget(home: &Path, domain: &str) {
    let _ = std::fs::remove_file(file(home, domain));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_and_crops_from_the_top() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(1280, 2000, image::Rgb([200, 30, 30]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let jpeg = small_jpeg(&png, WIDTH, HEIGHT).unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (WIDTH, HEIGHT));
    }

    #[test]
    fn tells_a_blank_page_from_a_real_one() {
        let png = |img: image::RgbImage| {
            let mut out = Vec::new();
            img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
                .unwrap();
            out
        };
        let white = image::RgbImage::from_pixel(400, 250, image::Rgb([255, 255, 255]));
        assert!(is_blank(&png(white.clone())));
        // A header bar across the top of a white page is a page.
        let mut page = white;
        for y in 0..40 {
            for x in 0..400 {
                page.put_pixel(x, y, image::Rgb([30, 60, 120]));
            }
        }
        assert!(!is_blank(&png(page)));
        assert!(is_blank(b"not a picture"));
    }

    #[test]
    fn names_files_after_the_domain() {
        let home = Path::new("/h");
        assert!(file(home, "shop.stack").ends_with("cache/thumbnails/shop.stack.jpg"));
        assert!(file(home, "../x").ends_with("cache/thumbnails/.._x.jpg"));
    }
}
