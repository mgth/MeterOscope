//! Capture hors Linux (Windows, macOS) par nokhwa. Le décodage reste
//! celui de ce crate (pas de mozjpeg).

use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nokhwa::Camera;
use nokhwa::utils::{ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};

use crate::{CameraError, DeviceInfo, PixelFormat, Publisher, VideoMode, rank_modes, spawn_capture};

const ALL_FORMATS: &[FrameFormat] = &[
    FrameFormat::MJPEG,
    FrameFormat::YUYV,
    FrameFormat::NV12,
    FrameFormat::GRAY,
    FrameFormat::RAWRGB,
    FrameFormat::RAWBGR,
];

/// Sans image pendant ce temps, la caméra est considérée comme perdue.
const STALL: Duration = Duration::from_secs(5);

fn to_pixel(f: FrameFormat) -> PixelFormat {
    match f {
        FrameFormat::MJPEG => PixelFormat::Mjpeg,
        FrameFormat::YUYV => PixelFormat::Yuyv,
        FrameFormat::NV12 => PixelFormat::Nv12,
        FrameFormat::GRAY => PixelFormat::Gray,
        FrameFormat::RAWRGB => PixelFormat::Rgb,
        FrameFormat::RAWBGR => PixelFormat::Bgr,
    }
}

fn to_frame(f: PixelFormat) -> Option<FrameFormat> {
    Some(match f {
        PixelFormat::Mjpeg => FrameFormat::MJPEG,
        PixelFormat::Yuyv => FrameFormat::YUYV,
        PixelFormat::Nv12 => FrameFormat::NV12,
        PixelFormat::Gray => FrameFormat::GRAY,
        PixelFormat::Rgb => FrameFormat::RAWRGB,
        PixelFormat::Bgr => FrameFormat::RAWBGR,
        PixelFormat::Uyvy => return None,
    })
}

/// macOS : demande l'autorisation d'accès à la caméra (une seule fois).
fn init() {
    #[cfg(target_os = "macos")]
    if !nokhwa::nokhwa_check() {
        nokhwa::nokhwa_initialize(|_| {});
    }
}

pub(crate) fn list_devices() -> Vec<DeviceInfo> {
    init();
    let Ok(infos) = nokhwa::query(ApiBackend::Auto) else { return Vec::new() };
    infos
        .iter()
        .enumerate()
        .map(|(pos, info)| {
            let index = match info.index() {
                CameraIndex::Index(i) => *i,
                CameraIndex::String(_) => pos as u32,
            };
            let name = info.human_name().trim().to_string();
            let name = if name.is_empty() { format!("Caméra {}", index) } else { name };
            DeviceInfo { index, name }
        })
        .collect()
}

pub(crate) fn list_modes(device: &DeviceInfo) -> Vec<VideoMode> {
    let request = RequestedFormat::with_formats(RequestedFormatType::AbsoluteHighestFrameRate, ALL_FORMATS);
    let Ok(mut cam) = Camera::new(CameraIndex::Index(device.index), request) else { return Vec::new() };
    let formats = cam.compatible_camera_formats().unwrap_or_default();
    formats.into_iter().map(to_mode).collect()
}

fn to_mode(f: CameraFormat) -> VideoMode {
    VideoMode { width: f.width(), height: f.height(), fps: f.frame_rate() as f32, format: to_pixel(f.format()) }
}

fn explain(e: &nokhwa::NokhwaError) -> String {
    let s = e.to_string();
    let low = s.to_lowercase();
    if low.contains("denied") || low.contains("permission") || low.contains("authoriz") {
        "permission refusée (autorisez l'accès à la caméra dans les réglages du système)".into()
    } else if low.contains("busy") || low.contains("in use") {
        "la caméra est déjà utilisée par une autre application".into()
    } else {
        format!("erreur du pilote ({s})")
    }
}

pub(crate) fn start(
    device: &DeviceInfo,
    wanted: Option<(u32, u32)>,
    publisher: Publisher,
) -> Result<JoinHandle<()>, CameraError> {
    init();
    let index = device.index;
    let shared = publisher.shared.clone();
    // La caméra est créée dans le fil de capture (COM sous Windows) ;
    // le résultat de l'ouverture nous revient par ce canal.
    let (tx, rx) = sync_channel::<Result<(), String>>(1);
    let handle = spawn_capture(publisher, move |p| {
        let request = RequestedFormat::with_formats(RequestedFormatType::AbsoluteHighestFrameRate, ALL_FORMATS);
        let opened = Camera::new(CameraIndex::Index(index), request).and_then(|mut cam| {
            let modes = cam.compatible_camera_formats()?.into_iter().map(|f| (to_mode(f), f)).collect();
            let Some((mode, fmt)) = rank_modes(modes, wanted).into_iter().find(|(m, _)| to_frame(m.format).is_some())
            else {
                return Ok(None);
            };
            cam.set_camera_requset(RequestedFormat::with_formats(RequestedFormatType::Exact(fmt), ALL_FORMATS))?;
            cam.open_stream()?;
            Ok(Some((cam, mode)))
        });
        let (mut cam, _) = match opened {
            Ok(Some((cam, mode))) => {
                p.set_mode(VideoMode { format: to_pixel(cam.frame_format()), ..mode });
                let _ = tx.send(Ok(()));
                (cam, mode)
            }
            Ok(None) => {
                let _ = tx.send(Err(match wanted {
                    Some((w, h)) => format!("résolution {w}×{h} non prise en charge"),
                    None => "aucun format d'image pris en charge".into(),
                }));
                return;
            }
            Err(e) => {
                let _ = tx.send(Err(explain(&e)));
                return;
            }
        };
        let mut last_frame = Instant::now();
        let mut errors = 0u32;
        while !p.should_stop() {
            match cam.frame() {
                Ok(buf) => {
                    last_frame = Instant::now();
                    let res = buf.resolution();
                    let fmt = to_pixel(buf.source_frame_format());
                    if p.decode_and_publish(fmt, res.width(), res.height(), 0, buf.buffer()).is_err() {
                        errors += 1;
                    } else {
                        errors = 0;
                    }
                }
                Err(_) => {
                    errors += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            if errors >= 60 || last_frame.elapsed() > STALL {
                p.fail("La caméra ne fournit plus d'image.");
                break;
            }
        }
        let _ = cam.stop_stream();
    })?;
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(handle),
        Ok(Err(msg)) => {
            let _ = handle.join();
            Err(CameraError::Open(msg))
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = handle.join();
            Err(CameraError::Open("erreur interne du pilote".into()))
        }
        // Le fil reste détaché ; il s'arrêtera seul quand le pilote rendra la main.
        Err(RecvTimeoutError::Timeout) => {
            shared.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            Err(CameraError::Open("la caméra ne répond pas".into()))
        }
    }
}
