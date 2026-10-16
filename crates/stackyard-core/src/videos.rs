//! Video optimizer: makes web-ready MP4 (H.264), WebM (VP9) and GIF copies
//! of videos and GIFs, optionally scaled down and without sound.
//!
//! The work is done by FFmpeg, run as a separate program. Stackyard does
//! not ship it, to keep the app small: it uses an FFmpeg already on the
//! computer, or downloads one into `tools/ffmpeg` when asked (from BtbN's
//! FFmpeg-Builds on Windows and Linux, from Martin Riedl's builds on macOS,
//! each checked against the checksum published next to it). Originals are
//! never changed.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::download::{self, Http};
use crate::images::Source;
use crate::install::{self, Release, Step};
use crate::platform::{Arch, Os, Platform};
use crate::{Error, Result};

pub const MODULE: &str = "videos";

const BTBN: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const RIEDL: &str = "https://ffmpeg.martin-riedl.de/redirect/latest/macos";
/// Folders that hold other people's files, never a project's own videos.
const SKIPPED_FOLDERS: [&str; 5] = ["node_modules", "vendor", ".git", ".svn", "dist"];
/// GIFs get large fast; without a width they are made at most this wide.
const GIF_WIDTH: u32 = 480;
/// Lines of FFmpeg's own output kept to explain a failure.
const KEEP_LINES: usize = 6;

static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// H.264 with AAC sound: plays everywhere.
    Mp4,
    /// VP9 with Opus sound: smaller, plays in every current browser.
    Webm,
    /// An animated GIF, without sound.
    Gif,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Format::Mp4 => "mp4",
            Format::Webm => "webm",
            Format::Gif => "gif",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// Smallest files; fine for backgrounds and previews.
    Small,
    #[default]
    Balanced,
    /// Looks like the original; larger files.
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    pub formats: Vec<Format>,
    #[serde(default)]
    pub quality: Quality,
    /// Wider videos are scaled down to this width; the height follows.
    pub max_width: Option<u32>,
    /// Leave the sound out (as background videos usually are).
    #[serde(default)]
    pub mute: bool,
    /// Where new files go; `None` puts them next to the originals.
    pub out_dir: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            formats: vec![Format::Mp4, Format::Webm],
            quality: Quality::Balanced,
            max_width: None,
            mute: false,
            out_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Made {
    pub format: Format,
    pub path: PathBuf,
    pub size: u64,
    /// Why no file was kept, e.g. the new one was not smaller.
    pub skipped: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileResult {
    pub source: PathBuf,
    pub size: u64,
    pub made: Vec<Made>,
    pub error: Option<String>,
}

/// How far along a run is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// Videos finished so far.
    pub done: usize,
    pub total: usize,
    /// The video being worked on.
    pub current: Option<PathBuf>,
    pub format: Option<Format>,
    /// 0 to 100 for the current video and format, when its length is known.
    pub percent: Option<f32>,
    /// Set when a video was just finished.
    pub result: Option<FileResult>,
}

/// Where FFmpeg comes from, as the Videos screen shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// The FFmpeg that will be used, if any.
    pub path: Option<PathBuf>,
    /// Stackyard downloaded it (and can remove it).
    pub downloaded: bool,
    /// Its first line of `-version`, e.g. `ffmpeg version n8.1`.
    pub version: Option<String>,
}

pub fn dir(home: &Path) -> PathBuf {
    home.join("tools").join("ffmpeg")
}

fn program_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// The FFmpeg Stackyard downloaded, if any.
fn downloaded(home: &Path) -> Option<PathBuf> {
    let dir = dir(home);
    [
        dir.join("bin").join(program_name()),
        dir.join(program_name()),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// An FFmpeg on the PATH, such as one installed with Homebrew or winget.
/// Apps opened from the Dock get a short PATH on macOS, so Homebrew's
/// folders are looked in too.
fn on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    dirs.into_iter()
        .map(|dir| dir.join(program_name()))
        .find(|p| p.is_file())
}

/// The FFmpeg to use: Stackyard's own download first, else one on the PATH.
pub fn ffmpeg(home: &Path) -> Option<PathBuf> {
    downloaded(home).or_else(on_path)
}

pub fn status(home: &Path) -> Status {
    let own = downloaded(home);
    let path = own.clone().or_else(on_path);
    let version = path.as_deref().and_then(version_of);
    Status {
        path,
        downloaded: own.is_some(),
        version,
    }
}

fn version_of(program: &Path) -> Option<String> {
    let mut command = Command::new(program);
    command.arg("-version").stdin(Stdio::null());
    crate::platform::hide_console(&mut command);
    let out = command.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?.trim();
    // "ffmpeg version n8.1-7-g… Copyright (c) …" → "ffmpeg version n8.1-7-g…"
    Some(
        first
            .split(" Copyright")
            .next()
            .unwrap_or(first)
            .to_string(),
    )
}

/// The build to download for this computer.
pub fn resolve(http: &Http, platform: Platform) -> Result<(Release, &'static str)> {
    match platform.os {
        Os::Macos => {
            let arch = match platform.arch {
                Arch::Arm64 => "arm64",
                Arch::X64 => "amd64",
            };
            // The stable link redirects to this week's build, which has its
            // checksum next to it.
            let latest = format!("{RIEDL}/{arch}/release/ffmpeg.zip");
            // The redirect service now and then answers 404 for a moment
            // (seen in CI), so it gets a few tries.
            let mut tries = 0;
            let url = loop {
                match http.final_url(&latest) {
                    Ok(url) => break url,
                    Err(_) if tries < 3 => {
                        tries += 1;
                        std::thread::sleep(std::time::Duration::from_secs(2 * tries));
                    }
                    Err(e) => return Err(e),
                }
            };
            let sums_url = format!("{url}.sha256");
            let sums = http.get_text(&sums_url)?;
            let checksum = sums
                .split_whitespace()
                .find(|w| download::is_sha256(w))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| Error::Download {
                    url: sums_url,
                    message: "no checksum in the answer".into(),
                })?;
            // …/download/macos/arm64/1760000000_8.0.1/ffmpeg.zip → 8.0.1
            let version = url
                .split('/')
                .filter_map(|part| part.split_once('_').map(|(_, v)| v))
                .find(|v| v.starts_with(|c: char| c.is_ascii_digit()))
                .unwrap_or("latest")
                .to_string();
            let release = Release {
                version,
                url,
                file_name: format!("ffmpeg-macos-{arch}.zip"),
                checksum,
            };
            Ok((release, "ffmpeg"))
        }
        Os::Windows | Os::Linux => {
            let name = match (platform.os, platform.arch) {
                (Os::Windows, Arch::X64) => "win64",
                (Os::Windows, Arch::Arm64) => "winarm64",
                (_, Arch::X64) => "linux64",
                (_, Arch::Arm64) => "linuxarm64",
            };
            let sums_url = format!("{BTBN}/checksums.sha256");
            let sums = http.get_text(&sums_url)?;
            let (file_name, version) = newest_btbn(&sums, name).ok_or_else(|| Error::Download {
                url: sums_url.clone(),
                message: format!("no FFmpeg build for {platform}"),
            })?;
            let checksum =
                download::find_checksum(&sums, &file_name).ok_or_else(|| Error::Download {
                    url: sums_url,
                    message: format!("no checksum for {file_name}"),
                })?;
            let release = Release {
                version,
                url: format!("{BTBN}/{file_name}"),
                file_name,
                checksum,
            };
            let program = if platform.os == Os::Windows {
                "bin/ffmpeg.exe"
            } else {
                "bin/ffmpeg"
            };
            Ok((release, program))
        }
    }
}

/// The newest numbered release in BtbN's checksum list for a platform such
/// as `win64`, e.g. `ffmpeg-n8.1-latest-win64-gpl-8.1.zip`, or the nightly
/// build when no numbered one is listed. Returns the file and its version.
fn newest_btbn(sums: &str, platform: &str) -> Option<(String, String)> {
    let ext = if platform.starts_with("win") {
        "zip"
    } else {
        "tar.xz"
    };
    let mut best: Option<(Vec<u32>, String, String)> = None;
    let mut nightly = None;
    for name in sums.lines().filter_map(|l| l.split_whitespace().nth(1)) {
        if name == format!("ffmpeg-master-latest-{platform}-gpl.{ext}") {
            nightly = Some((name.to_string(), "nightly".to_string()));
            continue;
        }
        let Some(rest) = name.strip_prefix("ffmpeg-n") else {
            continue;
        };
        let Some((version, tail)) = rest.split_once("-latest-") else {
            continue;
        };
        if tail != format!("{platform}-gpl-{version}.{ext}") {
            continue;
        }
        let Ok(parts) = version
            .split('.')
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _, _)| parts > *b) {
            best = Some((parts, name.to_string(), version.to_string()));
        }
    }
    best.map(|(_, name, version)| (name, version)).or(nightly)
}

/// Downloads FFmpeg into `tools/ffmpeg`, keeping only the `ffmpeg` program
/// (the builds also carry players, docs and libraries Stackyard has no use
/// for). Returns the version.
pub fn install(home: &Path, progress: &mut dyn FnMut(Step)) -> Result<String> {
    let http = Http::new();
    progress(Step::Resolving);
    let (release, program) = resolve(&http, Platform::current())?;
    let dest = dir(home);
    install::install(
        &http,
        &home.join("downloads"),
        &release,
        &dest,
        program,
        progress,
    )?;
    keep_only(&dest, program);
    Ok(release.version)
}

/// Windows keeps a program's file locked for a moment after it exits (and
/// virus scanners look at new programs), so removing it right after a run
/// gets a few tries.
pub fn uninstall(home: &Path) -> Result<()> {
    let mut tries = 0;
    loop {
        match install::remove(&dir(home)) {
            Err(Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::PermissionDenied && tries < 10 =>
            {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            other => return other,
        }
    }
}

/// Deletes everything in `dir` but `program` (a path inside it) and the
/// install receipt. Best effort: leftovers only cost disk space.
fn keep_only(dir: &Path, program: &str) {
    let keep: Vec<&str> = program.split('/').collect();
    fn prune(dir: &Path, keep: &[&str], top: bool) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if top && name == install::RECEIPT {
                continue;
            }
            let path = entry.path();
            match keep.split_first() {
                Some((first, rest)) if name == *first => {
                    if !rest.is_empty() {
                        prune(&path, rest, false);
                    }
                }
                _ => {
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                }
            }
        }
    }
    prune(dir, &keep, true);
}

/// Every video under the given files and folders.
pub fn find(paths: &[PathBuf]) -> Vec<Source> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, path, &mut found);
        } else if is_video(path) {
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
        } else if kind.is_file() && is_video(&path) {
            found.push(Source {
                path,
                base: base.to_owned(),
            });
        }
    }
}

/// A video or GIF, and not a copy this optimizer made (`clip.min.mp4`).
fn is_video(path: &Path) -> bool {
    let ext = extension(path);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase());
    matches!(
        ext.as_str(),
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "gif"
    ) && !stem.is_some_and(|s| s.ends_with(".min"))
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Where a new file for `source` goes: `clip.webm` next to `clip.mp4`, or
/// `clip.min.mp4` when it has the original's format.
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
    let ext = format.extension();
    let plain = dir.join(format!("{stem}.{ext}"));
    if plain == source.path {
        dir.join(format!("{stem}.min.{ext}"))
    } else {
        plain
    }
}

pub fn check(options: &Options) -> Result<()> {
    if options.formats.is_empty() {
        return Err(Error::Images(
            "pick at least one of MP4, WebM or GIF".into(),
        ));
    }
    if options.max_width.is_some_and(|w| !(16..=7680).contains(&w)) {
        return Err(Error::Images(
            "the width goes from 16 to 7680 pixels".into(),
        ));
    }
    Ok(())
}

/// Stops a running [`optimize_all`] after the FFmpeg it is running ends.
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// Makes every copy the options ask for, one video at a time (FFmpeg
/// already uses every core).
pub fn optimize_all(
    ffmpeg: &Path,
    sources: &[Source],
    options: &Options,
    progress: &mut dyn FnMut(Progress),
) -> Vec<FileResult> {
    STOP.store(false, Ordering::SeqCst);
    let total = sources.len();
    let mut results = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        let size = std::fs::metadata(&source.path).map_or(0, |m| m.len());
        let mut result = FileResult {
            source: source.path.clone(),
            size,
            made: Vec::new(),
            error: None,
        };
        for &format in &options.formats {
            if STOP.load(Ordering::SeqCst) {
                break;
            }
            let path = output_path(source, format, options);
            let mut report = |percent| {
                progress(Progress {
                    done: index,
                    total,
                    current: Some(source.path.clone()),
                    format: Some(format),
                    percent,
                    result: None,
                })
            };
            report(None);
            match convert(ffmpeg, &source.path, &path, format, options, &mut report) {
                Ok(new_size) => {
                    // A same-format copy that is not smaller is not worth a file.
                    let same = extension(&source.path) == format.extension();
                    let skipped = (same && new_size >= size).then(|| {
                        let _ = std::fs::remove_file(&path);
                        "not smaller than the original".to_string()
                    });
                    result.made.push(Made {
                        format,
                        path,
                        size: new_size,
                        skipped,
                    });
                }
                Err(e) => {
                    result.error = Some(e);
                    break;
                }
            }
        }
        results.push(result.clone());
        progress(Progress {
            done: index + 1,
            total,
            current: None,
            format: None,
            percent: None,
            result: Some(result),
        });
    }
    results
}

/// The FFmpeg arguments that turn `input` into `output` (written as
/// `format`, whatever `output`'s name).
pub fn arguments(input: &Path, output: &Path, format: Format, options: &Options) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-y",
        "-progress",
        "pipe:1",
        "-nostats",
        "-i",
    ]
    .map(String::from)
    .to_vec();
    args.push(input.display().to_string());
    // Even sizes, since H.264 and VP9 need them; never wider than the source.
    let width = match (format, options.max_width) {
        (_, Some(w)) => format!("trunc(min({w},iw)/2)*2"),
        (Format::Gif, None) => format!("trunc(min({GIF_WIDTH},iw)/2)*2"),
        _ => "trunc(iw/2)*2".into(),
    };
    let scale = format!("scale=w='{width}':h=-2:flags=lanczos");
    let quality = options.quality;
    let pick = |small: &str, balanced: &str, high: &str| {
        match quality {
            Quality::Small => small,
            Quality::Balanced => balanced,
            Quality::High => high,
        }
        .to_string()
    };
    let video_then_sound = ["-map", "0:v:0", "-map", "0:a:0?", "-map_metadata", "-1"];
    match format {
        Format::Mp4 => {
            args.extend(video_then_sound.map(String::from));
            args.extend(["-vf".into(), scale]);
            args.extend(["-c:v", "libx264", "-preset", "medium", "-crf"].map(String::from));
            args.push(pick("30", "25", "21"));
            args.extend(["-pix_fmt", "yuv420p", "-movflags", "+faststart"].map(String::from));
            sound(&mut args, options.mute, "aac", "128k");
            args.extend(["-f".into(), "mp4".into()]);
        }
        Format::Webm => {
            args.extend(video_then_sound.map(String::from));
            args.extend(["-vf".into(), scale]);
            args.extend(["-c:v", "libvpx-vp9", "-b:v", "0", "-crf"].map(String::from));
            args.push(pick("40", "34", "30"));
            args.extend(
                [
                    "-row-mt",
                    "1",
                    "-deadline",
                    "good",
                    "-cpu-used",
                    "2",
                    "-pix_fmt",
                    "yuv420p",
                ]
                .map(String::from),
            );
            sound(&mut args, options.mute, "libopus", "96k");
            args.extend(["-f".into(), "webm".into()]);
        }
        Format::Gif => {
            let fps = pick("8", "12", "15");
            args.extend([
                "-map".into(),
                "0:v:0".into(),
                "-vf".into(),
                format!(
                    "fps={fps},{scale},split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5"
                ),
                "-an".into(),
                "-loop".into(),
                "0".into(),
                "-f".into(),
                "gif".into(),
            ]);
        }
    }
    args.push(output.display().to_string());
    args
}

fn sound(args: &mut Vec<String>, mute: bool, codec: &str, rate: &str) {
    if mute {
        args.push("-an".into());
    } else {
        args.extend(["-c:a", codec, "-b:a", rate].map(String::from));
    }
}

/// Runs FFmpeg for one copy, written next to `output` first and renamed in
/// place when it is complete. Returns the new file's size.
fn convert(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    format: Format,
    options: &Options,
    report: &mut dyn FnMut(Option<f32>),
) -> std::result::Result<u64, String> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let partial = output.with_file_name(format!(
        "{}.partial",
        output.file_name().unwrap_or_default().to_string_lossy()
    ));
    let mut command = Command::new(ffmpeg);
    command
        .args(arguments(input, &partial, format, options))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::platform::hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start FFmpeg ({}): {e}", ffmpeg.display()))?;

    // FFmpeg prints the length on stderr ("Duration: 00:01:02.50") and its
    // progress on stdout ("out_time_us=1234567").
    let duration_us = Arc::new(AtomicU64::new(0));
    let last_lines = Arc::new(Mutex::new(Vec::<String>::new()));
    let stderr = child.stderr.take().expect("stderr is piped");
    let reader = {
        let duration_us = duration_us.clone();
        let last_lines = last_lines.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
                if let Some(us) = parse_duration(&line) {
                    duration_us
                        .compare_exchange(0, us, Ordering::SeqCst, Ordering::SeqCst)
                        .ok();
                }
                let mut kept = last_lines.lock().unwrap();
                kept.push(line);
                if kept.len() > KEEP_LINES * 4 {
                    kept.drain(..KEEP_LINES * 3);
                }
            }
        })
    };
    let stdout = child.stdout.take().expect("stdout is piped");
    for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
        if STOP.load(Ordering::SeqCst) {
            let _ = child.kill();
            break;
        }
        if let Some(us) = line
            .strip_prefix("out_time_us=")
            .and_then(|v| v.trim().parse::<u64>().ok())
        {
            let total = duration_us.load(Ordering::SeqCst);
            report((total > 0).then(|| (us as f32 * 100.0 / total as f32).min(100.0)));
        }
    }
    let status = child
        .wait()
        .map_err(|e| format!("FFmpeg did not finish: {e}"))?;
    let _ = reader.join();
    if STOP.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(&partial);
        return Err("stopped".into());
    }
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        let lines = last_lines.lock().unwrap();
        let why: Vec<&str> = lines
            .iter()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(KEEP_LINES)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(format!(
            "FFmpeg could not make the {}: {}",
            format.extension().to_uppercase(),
            why.join(" / ")
        ));
    }
    std::fs::rename(&partial, output)
        .map_err(|e| format!("could not save {}: {e}", output.display()))?;
    report(Some(100.0));
    std::fs::metadata(output)
        .map(|m| m.len())
        .map_err(|e| format!("could not read {}: {e}", output.display()))
}

/// `  Duration: 00:01:02.50, start: …` → 62 500 000 microseconds.
fn parse_duration(line: &str) -> Option<u64> {
    let rest = line.trim_start().strip_prefix("Duration: ")?;
    let time = rest.split(',').next()?.trim();
    let mut parts = time.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    let seconds =
        h.parse::<f64>().ok()? * 3600.0 + m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?;
    Some((seconds * 1_000_000.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_videos_and_skips_its_own_copies() {
        let dir = crate::testutil::tempdir();
        for name in [
            "a.mp4",
            "b.MOV",
            "c.min.mp4",
            "d.gif",
            "e.txt",
            "node_modules/f.mp4",
        ] {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        let found: Vec<String> = find(std::slice::from_ref(&dir))
            .iter()
            .map(|s| s.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, ["a.mp4", "b.MOV", "d.gif"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_new_files() {
        let source = Source {
            path: PathBuf::from("/site/media/clip.mp4"),
            base: PathBuf::from("/site"),
        };
        let mut options = Options::default();
        assert_eq!(
            output_path(&source, Format::Mp4, &options),
            PathBuf::from("/site/media/clip.min.mp4")
        );
        assert_eq!(
            output_path(&source, Format::Webm, &options),
            PathBuf::from("/site/media/clip.webm")
        );
        options.out_dir = Some(PathBuf::from("/out"));
        assert_eq!(
            output_path(&source, Format::Mp4, &options),
            PathBuf::from("/out/media/clip.mp4")
        );
    }

    #[test]
    fn picks_the_newest_numbered_btbn_build() {
        let sums = "\
aa  ffmpeg-master-latest-win64-gpl.zip
bb  ffmpeg-n8.1-latest-win64-gpl-8.1.zip
cc  ffmpeg-n9.0-latest-win64-gpl-9.0.zip
dd  ffmpeg-n9.0-latest-win64-lgpl-9.0.zip
ee  ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip
ff  ffmpeg-n10.0-latest-linux64-gpl-10.0.tar.xz
gg  ffmpeg-master-latest-linuxarm64-gpl.tar.xz";
        assert_eq!(
            newest_btbn(sums, "win64"),
            Some(("ffmpeg-n9.0-latest-win64-gpl-9.0.zip".into(), "9.0".into()))
        );
        assert_eq!(
            newest_btbn(sums, "linux64"),
            Some((
                "ffmpeg-n10.0-latest-linux64-gpl-10.0.tar.xz".into(),
                "10.0".into()
            ))
        );
        assert_eq!(
            newest_btbn(sums, "linuxarm64"),
            Some((
                "ffmpeg-master-latest-linuxarm64-gpl.tar.xz".into(),
                "nightly".into()
            ))
        );
        assert_eq!(newest_btbn(sums, "winarm64"), None);
    }

    #[test]
    fn reads_the_length() {
        assert_eq!(
            parse_duration("  Duration: 00:01:02.50, start: 0.000000, bitrate: 1205 kb/s"),
            Some(62_500_000)
        );
        assert_eq!(parse_duration("Duration: N/A, bitrate: N/A"), None);
    }

    #[test]
    fn arguments_follow_the_options() {
        let options = Options {
            max_width: Some(1280),
            mute: true,
            quality: Quality::Small,
            ..Options::default()
        };
        let args = arguments(
            Path::new("in.mov"),
            Path::new("out.mp4.partial"),
            Format::Mp4,
            &options,
        );
        let line = args.join(" ");
        assert!(
            line.contains("scale=w='trunc(min(1280,iw)/2)*2':h=-2"),
            "{line}"
        );
        assert!(line.contains("-crf 30"), "{line}");
        assert!(line.contains("-an"), "{line}");
        assert!(line.ends_with("-f mp4 out.mp4.partial"), "{line}");
        let gif = arguments(
            Path::new("in.mp4"),
            Path::new("o.gif"),
            Format::Gif,
            &Options::default(),
        )
        .join(" ");
        assert!(
            gif.contains("min(480,iw)") && gif.contains("palettegen"),
            "{gif}"
        );
    }

    #[test]
    fn keeps_only_the_program() {
        let dir = crate::testutil::tempdir();
        for name in [
            "bin/ffmpeg",
            "bin/ffplay",
            "bin/ffprobe",
            "doc/a.html",
            "LICENSE.txt",
        ] {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        keep_only(&dir, "bin/ffmpeg");
        assert!(dir.join("bin/ffmpeg").exists());
        assert!(!dir.join("bin/ffplay").exists());
        assert!(!dir.join("doc").exists());
        assert!(!dir.join("LICENSE.txt").exists());
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
            max_width: Some(4),
            ..Options::default()
        })
        .is_err());
    }

    #[test]
    #[ignore]
    fn live_resolves_ffmpeg_for_every_system() {
        let http = Http::new();
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            for arch in [Arch::X64, Arch::Arm64] {
                let (release, _) = resolve(&http, Platform { os, arch }).unwrap();
                assert!(download::is_sha256(&release.checksum), "{release:?}");
                println!("{os} {arch:?}: {} {}", release.version, release.url);
            }
        }
    }

    /// Downloads FFmpeg for this computer, then makes all three formats.
    #[test]
    #[ignore]
    fn live_downloads_ffmpeg_and_makes_mp4_webm_and_gif() {
        let home = crate::testutil::tempdir();
        let version = install(&home, &mut |_| {}).unwrap();
        println!("FFmpeg {version}");
        let ffmpeg = downloaded(&home).expect("ffmpeg was downloaded");
        assert!(status(&home).version.unwrap().starts_with("ffmpeg version"));
        let dir = home.join("site");
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mp4");
        let made = Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=641x361:rate=25",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
                "-c:v",
                "libx264",
                "-crf",
                "5",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(made.success());
        let options = Options {
            formats: vec![Format::Mp4, Format::Webm, Format::Gif],
            max_width: Some(320),
            ..Options::default()
        };
        let mut last = None;
        let results = optimize_all(
            &ffmpeg,
            &find(std::slice::from_ref(&dir)),
            &options,
            &mut |p| {
                if p.percent.is_some() {
                    last = p.percent;
                }
            },
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].error, None, "{:?}", results[0]);
        assert_eq!(results[0].made.len(), 3);
        assert!(dir.join("clip.min.mp4").is_file());
        assert!(dir.join("clip.webm").is_file());
        assert!(dir.join("clip.gif").is_file());
        assert_eq!(last, Some(100.0));
        uninstall(&home).unwrap();
        let _ = std::fs::remove_dir_all(&home);
    }
}
