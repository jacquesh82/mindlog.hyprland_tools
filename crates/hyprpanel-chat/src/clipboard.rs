//! Presse-papiers Wayland lu uniquement après une action de collage explicite.
use anyhow::{Context, ensure};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tempfile::TempPath;
use wl_clipboard_rs::paste::{self, ClipboardType, MimeType, Seat};

const MAX_BYTES: usize = 5 * 1024 * 1024;
const MAX_PIXELS: u64 = 32 * 1024 * 1024;

pub struct Store {
    directory: PathBuf,
    pub reading: AtomicBool,
    generation: AtomicU64,
    images: Mutex<HashMap<PathBuf, TempPath>>,
}

impl Store {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            reading: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            images: Mutex::new(HashMap::new()),
        }
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn begin(&self) -> Option<u64> {
        self.reading
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(self.generation.load(Ordering::Acquire))
    }
    pub fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }
    pub fn insert(&self, file: TempPath) -> PathBuf {
        let path = file.to_path_buf();
        self.images
            .lock()
            .expect("images empoisonnées")
            .insert(path.clone(), file);
        path
    }
    pub fn remove(&self, path: &Path) {
        self.images
            .lock()
            .expect("images empoisonnées")
            .remove(path);
    }
    pub fn take(&self, paths: &[PathBuf]) -> Vec<TempPath> {
        let mut images = self.images.lock().expect("images empoisonnées");
        paths
            .iter()
            .filter_map(|path| images.remove(path))
            .collect()
    }
    pub fn reset(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.images.lock().expect("images empoisonnées").clear();
    }
}

fn choose_mime(types: &std::collections::HashSet<String>) -> Option<(&'static str, &'static str)> {
    [
        ("image/png", ".png"),
        ("image/jpeg", ".jpg"),
        ("image/webp", ".webp"),
        ("image/gif", ".gif"),
    ]
    .into_iter()
    .find(|(mime, _)| types.contains(*mime))
}

fn stage_image(bytes: &[u8], suffix: &str, directory: &Path) -> anyhow::Result<TempPath> {
    ensure!(bytes.len() <= MAX_BYTES, "image limitée à 5 Mio");
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let expected = match suffix {
        ".png" => image::ImageFormat::Png,
        ".jpg" => image::ImageFormat::Jpeg,
        ".webp" => image::ImageFormat::WebP,
        ".gif" => image::ImageFormat::Gif,
        _ => anyhow::bail!("format non pris en charge"),
    };
    ensure!(
        reader.format() == Some(expected),
        "format de l’image incohérent"
    );
    let (width, height) = reader.into_dimensions().context("image illisible")?;
    ensure!(
        width > 0 && height > 0 && u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "dimensions d’image trop importantes"
    );
    let mut file = tempfile::Builder::new()
        .prefix("lyra-clipboard-")
        .suffix(suffix)
        .tempfile_in(directory)?;
    file.write_all(bytes)?;
    Ok(file.into_temp_path())
}

pub fn read_image(directory: &Path) -> anyhow::Result<Option<TempPath>> {
    let types = match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
        Ok(types) => types,
        Err(paste::Error::ClipboardEmpty | paste::Error::NoSeats | paste::Error::NoMimeType) => {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let Some((mime, suffix)) = choose_mime(&types) else {
        return Ok(None);
    };
    let (mut pipe, _) = paste::get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific(mime),
    )?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "lecture du presse-papiers trop longue"
        );
        let mut fd = libc::pollfd {
            fd: pipe.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // Le descripteur est possédé par pipe ; poll n'en prend pas possession.
        let ready = unsafe {
            libc::poll(
                &mut fd,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        ensure!(ready > 0, "lecture du presse-papiers trop longue");
        ensure!(
            fd.revents & (libc::POLLERR | libc::POLLNVAL) == 0,
            "presse-papiers interrompu"
        );
        let count = pipe.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        ensure!(bytes.len() + count <= MAX_BYTES, "image limitée à 5 Mio");
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(Some(stage_image(&bytes, suffix, directory)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn image_paste_is_bounded_private_and_has_a_draft_lifetime() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::new(directory.path().into());
        let generation = store.begin().unwrap();
        assert!(store.begin().is_none());
        assert!(store.current(generation));
        let bytes = png();
        let path = store.insert(stage_image(&bytes, ".png", directory.path()).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let in_flight = store.take(std::slice::from_ref(&path));
        store.reset();
        assert!(!store.current(generation));
        assert!(
            path.exists(),
            "l’agent garde l’image jusqu’à la fin de l’envoi"
        );
        drop(in_flight);
        assert!(!path.exists());
        let path = store.insert(stage_image(&bytes, ".png", directory.path()).unwrap());
        store.remove(&path);
        assert!(!path.exists());
    }
    #[test]
    fn non_images_and_large_payloads_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        assert!(stage_image(b"not an image", ".png", directory.path()).is_err());
        assert!(stage_image(&png(), ".jpg", directory.path()).is_err());
        assert!(stage_image(&vec![0; MAX_BYTES + 1], ".png", directory.path()).is_err());
        assert!(choose_mime(&["text/plain".to_string()].into_iter().collect()).is_none());
        assert_eq!(
            choose_mime(
                &["image/jpeg".to_string(), "image/png".to_string()]
                    .into_iter()
                    .collect()
            )
            .unwrap()
            .0,
            "image/png"
        );
    }
}
