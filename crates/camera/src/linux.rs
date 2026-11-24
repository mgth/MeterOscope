//! Capture sous Linux, directement par V4L2.

use std::io;
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use v4l::buffer::Type;
use v4l::frameinterval::FrameIntervalEnum;
use v4l::framesize::FrameSizeEnum;
use v4l::io::mmap::Stream as MmapStream;
use v4l::io::traits::CaptureStream;
use v4l::prelude::Device;
use v4l::video::Capture;
use v4l::video::capture::Parameters;
use v4l::{Format, FourCC, Fraction};

use crate::{CameraError, DeviceInfo, PixelFormat, Publisher, VideoMode, rank_modes, spawn_capture};

/// Délai d'attente d'une image : borne le temps de réaction à l'arrêt.
const POLL: Duration = Duration::from_millis(100);
/// Sans image pendant ce temps, la caméra est considérée comme perdue.
const STALL: Duration = Duration::from_secs(5);
/// Nombre de tampons noyau (latence faible, mémoire bornée).
const BUFFERS: u32 = 3;

fn fourcc_to_format(f: FourCC) -> Option<PixelFormat> {
    Some(match &f.repr {
        b"MJPG" | b"JPEG" => PixelFormat::Mjpeg,
        b"YUYV" => PixelFormat::Yuyv,
        b"UYVY" => PixelFormat::Uyvy,
        b"NV12" => PixelFormat::Nv12,
        b"GREY" => PixelFormat::Gray,
        b"RGB3" => PixelFormat::Rgb,
        b"BGR3" => PixelFormat::Bgr,
        _ => return None,
    })
}

fn path(index: u32) -> PathBuf {
    PathBuf::from(format!("/dev/video{index}"))
}

/// Nom lisible depuis sysfs (« HD Pro Webcam C920 »). Les pilotes UVC
/// répètent souvent le nom : « Modèle: Modèle Xyz » devient « Modèle Xyz ».
fn device_name(index: u32) -> String {
    let raw = std::fs::read_to_string(format!("/sys/class/video4linux/video{index}/name")).unwrap_or_default();
    let raw = raw.trim();
    let name = match raw.split_once(": ") {
        Some((a, b)) if b.starts_with(a) => b,
        _ => raw,
    };
    if name.is_empty() { format!("Caméra {index}") } else { name.to_string() }
}

pub(crate) fn list_devices() -> Vec<DeviceInfo> {
    let Ok(dir) = std::fs::read_dir("/dev") else { return Vec::new() };
    let mut indices: Vec<u32> = dir
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_prefix("video")?.parse().ok())
        .collect();
    indices.sort_unstable();
    // Les nœuds de métadonnées n'ont aucun format de capture vidéo.
    let mut found: Vec<(u32, u8)> = indices
        .into_iter()
        .filter_map(|i| {
            let modes = std::panic::catch_unwind(|| probe(i)).unwrap_or_default();
            // Un format couleur non compressé signe un vrai capteur couleur ;
            // les capteurs de profondeur annoncent parfois un « MJPEG » maison.
            let rank = modes.iter().map(|(m, _)| match m.format {
                PixelFormat::Gray => 0,
                PixelFormat::Mjpeg => 1,
                _ => 2,
            });
            (!modes.is_empty()).then(|| (i, rank.max().unwrap_or(0)))
        })
        .collect();
    // Caméras couleur d'abord (l'application ouvre la première par défaut),
    // puis les autres flux (infrarouge, profondeur…).
    found.sort_by_key(|&(i, rank)| (std::cmp::Reverse(rank), i));
    found.into_iter().map(|(index, _)| DeviceInfo { index, name: device_name(index) }).collect()
}

pub(crate) fn list_modes(device: &DeviceInfo) -> Vec<VideoMode> {
    probe(device.index).into_iter().map(|(m, _)| m).collect()
}

/// Modes de capture pris en charge, avec l'intervalle d'image minimal de
/// chacun. Ouvre le nœud sans bloquer et sans démarrer de flux.
fn probe(index: u32) -> Vec<(VideoMode, Fraction)> {
    let Ok(dev) = Device::with_path(path(index)) else { return Vec::new() };
    let Ok(formats) = dev.enum_formats() else { return Vec::new() };
    let mut modes = Vec::new();
    for desc in formats {
        let Some(format) = fourcc_to_format(desc.fourcc) else { continue };
        let Ok(sizes) = dev.enum_framesizes(desc.fourcc) else { continue };
        let mut dims = Vec::new();
        for s in sizes {
            match s.size {
                FrameSizeEnum::Discrete(d) => dims.push((d.width, d.height)),
                // Tailles continues : on ne retient que les extrêmes.
                FrameSizeEnum::Stepwise(s) => {
                    dims.push((s.max_width, s.max_height));
                    dims.push((s.min_width, s.min_height));
                }
            }
        }
        dims.dedup();
        for (w, h) in dims {
            if w == 0 || h == 0 {
                continue;
            }
            let best = dev
                .enum_frameintervals(desc.fourcc, w, h)
                .unwrap_or_default()
                .into_iter()
                .map(|i| match i.interval {
                    FrameIntervalEnum::Discrete(f) => f,
                    FrameIntervalEnum::Stepwise(s) => s.min,
                })
                .filter(|f| f.numerator > 0 && f.denominator > 0)
                .max_by(|a, b| fps(*a).total_cmp(&fps(*b)));
            // Sans intervalle annoncé, on suppose 30 i/s (cas de pilotes simples).
            let interval = best.unwrap_or(Fraction::new(1, 30));
            modes.push((VideoMode { width: w, height: h, fps: fps(interval), format }, interval));
        }
    }
    modes
}

fn fps(f: Fraction) -> f32 {
    f.denominator as f32 / f.numerator.max(1) as f32
}

/// Message français pour une erreur système V4L2.
fn explain(e: &io::Error) -> String {
    match e.raw_os_error() {
        Some(libc_errno::EBUSY) => "la caméra est déjà utilisée par une autre application".into(),
        Some(libc_errno::EACCES | libc_errno::EPERM) => {
            "permission refusée (l'utilisateur doit appartenir au groupe « video »)".into()
        }
        Some(libc_errno::ENOENT | libc_errno::ENODEV | libc_errno::ENXIO) => "caméra introuvable ou débranchée".into(),
        Some(libc_errno::ENOSPC) => {
            "bande passante USB insuffisante (essayez une résolution plus basse ou un autre port USB)".into()
        }
        _ => format!("erreur du pilote ({e})"),
    }
}

/// Codes d'erreur Linux utiles (évite une dépendance à `libc`).
mod libc_errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const EINTR: i32 = 4;
    pub const ENXIO: i32 = 6;
    pub const EAGAIN: i32 = 11;
    pub const EACCES: i32 = 13;
    pub const EBUSY: i32 = 16;
    pub const ENODEV: i32 = 19;
    pub const ENOSPC: i32 = 28;
}

/// Flux configuré, prêt à démarrer.
struct Configured {
    stream: MmapStream<'static>,
    format: PixelFormat,
    actual: Format,
    mode: VideoMode,
}

/// Applique un mode : format, cadence, puis allocation des tampons.
fn configure(dev: &Device, mode: VideoMode, interval: Fraction) -> io::Result<Configured> {
    let fourcc = FourCC::new(match mode.format {
        PixelFormat::Mjpeg => b"MJPG",
        PixelFormat::Yuyv => b"YUYV",
        PixelFormat::Uyvy => b"UYVY",
        PixelFormat::Nv12 => b"NV12",
        PixelFormat::Gray => b"GREY",
        PixelFormat::Rgb => b"RGB3",
        PixelFormat::Bgr => b"BGR3",
    });
    let actual = dev.set_format(&Format::new(mode.width, mode.height, fourcc))?;
    let Some(format) = fourcc_to_format(actual.fourcc) else {
        return Err(io::Error::other("format refusé par la caméra"));
    };
    // La cadence n'est qu'une préférence : on ignore un refus.
    let rate = dev.set_params(&Parameters::new(interval)).map(|p| fps(p.interval)).unwrap_or(mode.fps);
    let mut stream = MmapStream::with_buffers(dev, Type::VideoCapture, BUFFERS)?;
    stream.set_timeout(POLL);
    let mode = VideoMode { width: actual.width, height: actual.height, fps: rate, format };
    Ok(Configured { stream, format, actual, mode })
}

pub(crate) fn start(
    device: &DeviceInfo,
    wanted: Option<(u32, u32)>,
    publisher: Publisher,
) -> Result<JoinHandle<()>, CameraError> {
    let dev = Device::with_path(path(device.index)).map_err(|e| CameraError::Open(explain(&e)))?;
    let mut candidates = rank_modes(probe(device.index), wanted);
    if candidates.is_empty() {
        return Err(CameraError::Open(match wanted {
            Some((w, h)) => format!("résolution {w}×{h} non prise en charge"),
            None => "aucun format d'image pris en charge".into(),
        }));
    }
    // Configuration synchrone du meilleur mode : les erreurs d'ouverture
    // (appareil occupé, permission…) remontent directement à l'appelant.
    let mut last_err = None;
    let mut configured = None;
    while !candidates.is_empty() {
        let (mode, interval) = candidates.remove(0);
        match configure(&dev, mode, interval) {
            Ok(c) => {
                configured = Some(c);
                break;
            }
            // Occupée ou interdite : inutile d'essayer d'autres modes.
            Err(e) if matches!(e.raw_os_error(), Some(libc_errno::EBUSY | libc_errno::EACCES | libc_errno::ENODEV)) => {
                return Err(CameraError::Open(explain(&e)));
            }
            Err(e) => last_err = Some(e),
        }
    }
    let Some(configured) = configured else {
        return Err(CameraError::Open(last_err.map_or_else(|| "aucun mode utilisable".into(), |e| explain(&e))));
    };
    publisher.set_mode(configured.mode);
    spawn_capture(publisher, move |p| capture(p, dev, configured, candidates))
}

/// Issue d'une session de capture.
enum Outcome {
    Finished,
    /// Bande passante USB insuffisante au démarrage : essayer un autre mode.
    NoBandwidth(io::Error),
    /// Images de ce format jamais décodables (p. ex. « MJPEG » propriétaire
    /// d'un capteur de profondeur) : essayer un autre format.
    Undecodable(PixelFormat, String),
}

/// Fil de capture : enchaîne les modes de repli si le premier ne démarre pas.
fn capture(p: &mut Publisher, dev: Device, first: Configured, mut fallback: Vec<(VideoMode, Fraction)>) {
    let mut cur = first;
    loop {
        // `run` consomme le flux : ses tampons sont libérés avant tout repli.
        let mut err = match run(p, cur) {
            Outcome::Finished => return,
            Outcome::NoBandwidth(e) => explain(&e),
            Outcome::Undecodable(format, e) => {
                fallback.retain(|(m, _)| m.format != format);
                format!("images illisibles ({e})")
            }
        };
        loop {
            if p.should_stop() {
                return;
            }
            if fallback.is_empty() {
                p.fail(format!("Capture impossible : {err}."));
                return;
            }
            let (mode, interval) = fallback.remove(0);
            match configure(&dev, mode, interval) {
                Ok(c) => {
                    p.set_mode(c.mode);
                    cur = c;
                    break;
                }
                Err(e) => err = explain(&e),
            }
        }
    }
}

fn run(p: &mut Publisher, mut cur: Configured) -> Outcome {
    let mut got_frame = false;
    let mut last_frame = Instant::now();
    let mut decode_errors = 0u32;
    // Après un délai dépassé, le tampon courant est déjà rendu au pilote :
    // il faut seulement en récupérer un (sans le ré-enfiler).
    let mut dequeue_only = false;
    let (w, h, stride) = (cur.actual.width, cur.actual.height, cur.actual.stride as usize);
    loop {
        if p.should_stop() {
            return Outcome::Finished;
        }
        let res = if dequeue_only {
            CaptureStream::dequeue(&mut cur.stream).map(|_| None)
        } else {
            cur.stream.next().map(|(buf, meta)| Some((buf, meta.bytesused as usize)))
        };
        match res {
            Ok(None) => dequeue_only = false,
            Ok(Some((buf, used))) => {
                last_frame = Instant::now();
                // bytesused vaut parfois 0 avec certains pilotes : tampon entier.
                let data = if used == 0 || used > buf.len() { buf } else { &buf[..used] };
                match p.decode_and_publish(cur.format, w, h, stride, data) {
                    Ok(()) => {
                        got_frame = true;
                        decode_errors = 0;
                    }
                    // Images corrompues isolées : fréquentes en MJPEG, ignorées.
                    Err(e) => {
                        decode_errors += 1;
                        if !got_frame && decode_errors >= 10 {
                            return Outcome::Undecodable(cur.format, e);
                        }
                        if decode_errors >= 60 {
                            p.fail(format!("Images illisibles reçues de la caméra : {e}"));
                            return Outcome::Finished;
                        }
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                dequeue_only = true;
                if last_frame.elapsed() > STALL {
                    p.fail(if got_frame {
                        "La caméra ne fournit plus d'image."
                    } else {
                        "La caméra n'envoie aucune image."
                    });
                    return Outcome::Finished;
                }
            }
            Err(e) if matches!(e.raw_os_error(), Some(libc_errno::EINTR | libc_errno::EAGAIN)) => {}
            Err(e) if !got_frame && e.raw_os_error() == Some(libc_errno::ENOSPC) => return Outcome::NoBandwidth(e),
            Err(e) => {
                let msg = match e.raw_os_error() {
                    Some(libc_errno::ENODEV | libc_errno::ENXIO) => "La caméra a été débranchée.".to_string(),
                    _ => format!("Capture interrompue : {}.", explain(&e)),
                };
                p.fail(msg);
                return Outcome::Finished;
            }
        }
    }
}
