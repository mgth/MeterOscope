//! Lecture et écriture des fichiers image.

use meteroscope_i18n::tr;
use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, ImageError, ImageFormat, ImageReader, RgbImage, RgbaImage};
use std::fs::File;
use std::io::{BufWriter, ErrorKind, Write};
use std::path::Path;

/// Qualité JPEG à l'enregistrement (bon compromis netteté / taille).
const JPEG_QUALITY: u8 = 92;

/// Nom court du fichier pour les messages.
pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned()
}

/// Message français pour une erreur d'entrée/sortie.
pub(crate) fn describe_io(e: &std::io::Error) -> String {
    match e.kind() {
        ErrorKind::NotFound => tr!("io.not_found").to_owned(),
        ErrorKind::PermissionDenied => tr!("io.denied").to_owned(),
        ErrorKind::IsADirectory => tr!("io.is_dir").to_owned(),
        ErrorKind::StorageFull => tr!("io.disk_full").to_owned(),
        ErrorKind::ReadOnlyFilesystem => tr!("io.read_only").to_owned(),
        _ => tr!("io.other", error = e),
    }
}

/// Message français pour une erreur du crate `image`.
fn describe_image(e: &ImageError) -> String {
    match e {
        ImageError::Unsupported(u) => tr!("image.unsupported", detail = u),
        ImageError::Decoding(d) => tr!("image.corrupt", detail = d),
        ImageError::Encoding(d) => tr!("image.encode_failed", detail = d),
        ImageError::Limits(_) => tr!("image.too_large").to_owned(),
        ImageError::Parameter(p) => tr!("image.bad_parameter", detail = p),
        ImageError::IoError(io) => describe_io(io),
    }
}

/// Charge une image en RGBA en appliquant l'orientation EXIF.
pub fn load_image(path: &Path) -> Result<RgbaImage> {
    let ctx = || tr!("image.open_failed", file = file_name(path));
    let reader = ImageReader::open(path).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    // Le contenu prime sur l'extension (fichiers mal nommés).
    let reader = reader.with_guessed_format().map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    if reader.format().is_none() {
        return Err(anyhow!(tr!("image.unknown_format"))).with_context(ctx);
    }
    let mut decoder = reader.into_decoder().map_err(|e| anyhow!(describe_image(&e))).with_context(ctx)?;
    use image::ImageDecoder;
    // Une orientation illisible n'empêche pas d'afficher l'image.
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| anyhow!(describe_image(&e))).with_context(ctx)?;
    img.apply_orientation(orientation);
    Ok(img.into_rgba8())
}

/// Aplatit l'alpha sur fond blanc (pour les formats sans transparence).
fn flatten_on_white(image: &RgbaImage) -> RgbImage {
    let mut out = RgbImage::new(image.width(), image.height());
    for (d, s) in out.as_mut().as_chunks_mut::<3>().0.iter_mut().zip(image.as_raw().as_chunks::<4>().0) {
        let a = s[3] as u32;
        for c in 0..3 {
            d[c] = ((s[c] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
    }
    out
}

/// Enregistre une image ; le format est déduit de l'extension.
pub fn save_image(image: &RgbaImage, path: &Path) -> Result<()> {
    let ctx = || tr!("image.save_failed", file = file_name(path));
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let Some(ext) = ext else {
        return Err(anyhow!(tr!("image.missing_extension"))).with_context(ctx);
    };
    let Some(format) = ImageFormat::from_extension(&ext).filter(|f| f.writing_enabled()) else {
        return Err(anyhow!(tr!("image.unsupported_save", ext = ext))).with_context(ctx);
    };
    let file = File::create(path).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    let mut w = BufWriter::new(file);
    let r = match format {
        ImageFormat::Jpeg => {
            let rgb = flatten_on_white(image);
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, JPEG_QUALITY);
            rgb.write_with_encoder(enc)
        }
        // BMP : RGB, plus compatible que BMP 32 bits.
        ImageFormat::Bmp => flatten_on_white(image).write_to(&mut w, format),
        _ => image.write_to(&mut w, format),
    };
    if let Err(e) = r {
        drop(w);
        let _ = std::fs::remove_file(path);
        return Err(anyhow!(describe_image(&e))).with_context(ctx);
    }
    w.flush().map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    Ok(())
}

/// Vérifie qu'une extension est enregistrable (utile avant un long rendu).
pub fn can_save_extension(ext: &str) -> bool {
    ImageFormat::from_extension(ext).is_some_and(|f| f.writing_enabled())
}
