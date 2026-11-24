//! Capture caméra (webcam, microscope USB…) sans dépendance à l'interface.
//!
//! CONTRAT PUBLIC (utilisé par `crates/app`) — ne pas changer les signatures
//! existantes ; on peut en ajouter.
//!
//! Sous Linux la capture passe directement par V4L2 (crate `v4l`) : nokhwa n'y
//! offre ni délai d'attente sur la lecture d'image (arrêt non garanti) ni tri
//! des nœuds /dev/video*. Les autres systèmes utilisent nokhwa.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod decode;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod backend;

#[cfg(not(target_os = "linux"))]
#[path = "other.rs"]
mod backend;

/// Une caméra détectée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub index: u32,
    pub name: String,
}

/// Une image capturée, RGBA 8 bits, lignes de haut en bas.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Numéro d'ordre croissant.
    pub seq: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("aucune caméra détectée")]
    NoDevice,
    #[error("impossible d'ouvrir la caméra : {0}")]
    Open(String),
}

/// Format de pixels transmis par la caméra (avant conversion en RGBA).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// Motion-JPEG (compressé, permet les hautes résolutions fluides).
    Mjpeg,
    /// YUV 4:2:2 entrelacé Y-U-Y-V (non compressé).
    Yuyv,
    /// YUV 4:2:2 entrelacé U-Y-V-Y (non compressé).
    Uyvy,
    /// YUV 4:2:0 semi-planaire.
    Nv12,
    /// Niveaux de gris 8 bits (caméras infrarouges, industrielles…).
    Gray,
    /// RGB 24 bits.
    Rgb,
    /// BGR 24 bits.
    Bgr,
}

impl PixelFormat {
    /// Nom court lisible (« MJPEG », « YUYV »…).
    pub fn name(self) -> &'static str {
        match self {
            Self::Mjpeg => "MJPEG",
            Self::Yuyv => "YUYV",
            Self::Uyvy => "UYVY",
            Self::Nv12 => "NV12",
            Self::Gray => "Gris",
            Self::Rgb => "RGB",
            Self::Bgr => "BGR",
        }
    }
}

/// Un mode vidéo proposé par une caméra.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    /// Cadence maximale annoncée par la caméra (images/s).
    pub fps: f32,
    pub format: PixelFormat,
}

/// Cadence en dessous de laquelle un mode n'est plus considéré comme fluide.
const FLUID_FPS: f32 = 14.5;

/// Liste les caméras utilisables (sans doublons : sous Linux une webcam
/// expose plusieurs nœuds /dev/video*, ne garder que ceux qui capturent).
pub fn list_devices() -> Vec<DeviceInfo> {
    let mut devices = catch_unwind(backend::list_devices).unwrap_or_default();
    // Noms uniques, pour que l'utilisateur puisse les distinguer.
    for i in 1..devices.len() {
        let base = devices[i].name.clone();
        let mut n = 1;
        while devices[..i].iter().any(|d| d.name == devices[i].name) {
            n += 1;
            devices[i].name = format!("{base} ({n})");
        }
    }
    devices
}

/// Résolutions proposées par la caméra, de la plus grande à la plus petite,
/// avec pour chacune le meilleur format disponible. Vide si la caméra ne
/// répond pas.
pub fn list_modes(device: &DeviceInfo) -> Vec<VideoMode> {
    let all = catch_unwind(|| backend::list_modes(device)).unwrap_or_default();
    let mut best: Vec<VideoMode> = Vec::new();
    for (m, ()) in rank_modes(all.into_iter().map(|m| (m, ())).collect(), None) {
        if !best.iter().any(|b| (b.width, b.height) == (m.width, m.height)) {
            best.push(m);
        }
    }
    best.sort_by(|a, b| {
        (b.width as u64 * b.height as u64)
            .cmp(&(a.width as u64 * a.height as u64))
            .then(b.width.cmp(&a.width))
    });
    best
}

/// Trie les modes du meilleur au moins bon : couleur avant niveaux de gris,
/// fluide (≥ ~15 i/s) avant saccadé, puis plus haute résolution (ou, faute de
/// mode fluide, plus haute cadence), puis format non compressé avant MJPEG à
/// résolution égale (pas d'artefacts de compression, utile pour mesurer).
/// `wanted` restreint aux modes de cette résolution.
pub(crate) fn rank_modes<T>(mut modes: Vec<(VideoMode, T)>, wanted: Option<(u32, u32)>) -> Vec<(VideoMode, T)> {
    if let Some(wh) = wanted {
        modes.retain(|(m, _)| (m.width, m.height) == wh);
    }
    let px = |m: &VideoMode| m.width as u64 * m.height as u64;
    let color = |m: &VideoMode| m.format != PixelFormat::Gray;
    let fluid = |m: &VideoMode| m.fps >= FLUID_FPS;
    let raw = |m: &VideoMode| m.format != PixelFormat::Mjpeg;
    modes.sort_by(|(a, _), (b, _)| {
        color(b)
            .cmp(&color(a))
            .then(fluid(b).cmp(&fluid(a)))
            .then_with(|| if fluid(a) { px(b).cmp(&px(a)) } else { b.fps.total_cmp(&a.fps) })
            .then(px(b).cmp(&px(a)))
            .then(raw(b).cmp(&raw(a)))
            .then(b.fps.total_cmp(&a.fps))
    });
    modes
}

/// État partagé entre le fil de capture et le `CameraStream`.
#[derive(Default)]
pub(crate) struct Shared {
    latest: Mutex<Option<Arc<Frame>>>,
    /// Numéro de la dernière image rendue par `take_frame`.
    taken: AtomicU64,
    error: Mutex<Option<String>>,
    stop: AtomicBool,
    mode: Mutex<Option<VideoMode>>,
    /// Cadence mesurée (bits d'un f32), 0 tant qu'inconnue.
    fps_bits: AtomicU32,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Côté fil de capture : publie les images et les erreurs.
pub(crate) struct Publisher {
    shared: Arc<Shared>,
    on_frame: Box<dyn Fn() + Send>,
    seq: u64,
    /// Tampon recyclé de l'avant-dernière image, si plus personne ne la tient.
    spare: Option<Vec<u8>>,
    last: Option<Instant>,
    fps: f32,
    /// Fermé quand le fil se termine (permet un arrêt borné dans le temps).
    _alive: Sender<()>,
}

impl Publisher {
    pub(crate) fn should_stop(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }

    pub(crate) fn set_mode(&self, mode: VideoMode) {
        *lock(&self.shared.mode) = Some(mode);
    }

    pub(crate) fn fail(&self, msg: impl Into<String>) {
        *lock(&self.shared.error) = Some(msg.into());
    }

    /// Décode une image brute et la publie. Renvoie l'erreur de décodage
    /// (l'image est alors simplement ignorée).
    pub(crate) fn decode_and_publish(
        &mut self,
        format: PixelFormat,
        width: u32,
        height: u32,
        stride: usize,
        data: &[u8],
    ) -> Result<(), String> {
        let mut buf = self.spare.take().unwrap_or_default();
        match decode::decode(format, width, height, stride, data, &mut buf) {
            Ok((w, h)) => {
                self.publish(w, h, buf);
                Ok(())
            }
            Err(e) => {
                self.spare = Some(buf);
                Err(e)
            }
        }
    }

    fn publish(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        let now = Instant::now();
        if let Some(last) = self.last {
            let inst = 1.0 / now.duration_since(last).as_secs_f32().max(1e-4);
            self.fps = if self.fps == 0.0 { inst } else { self.fps * 0.9 + inst * 0.1 };
            self.shared.fps_bits.store(self.fps.to_bits(), Ordering::Relaxed);
        }
        self.last = Some(now);
        self.seq += 1;
        let frame = Arc::new(Frame { width, height, rgba, seq: self.seq });
        let old = lock(&self.shared.latest).replace(frame);
        // Une seule image conservée : l'ancienne est libérée ou recyclée.
        if let Some(f) = old.and_then(|o| Arc::try_unwrap(o).ok()) {
            self.spare = Some(f.rgba);
        }
        (self.on_frame)();
    }
}

/// Lance le fil de capture ; une panique y devient une erreur lisible.
pub(crate) fn spawn_capture(
    mut publisher: Publisher,
    body: impl FnOnce(&mut Publisher) + Send + 'static,
) -> Result<JoinHandle<()>, CameraError> {
    std::thread::Builder::new()
        .name("meteroscope-camera".into())
        .spawn(move || {
            if catch_unwind(AssertUnwindSafe(|| body(&mut publisher))).is_err() {
                publisher.fail("La capture vidéo s'est arrêtée sur une erreur interne.");
            }
        })
        .map_err(|e| CameraError::Open(format!("impossible de lancer le fil de capture ({e})")))
}

/// Flux vidéo en direct, capturé dans un fil dédié. S'arrête au `Drop`.
pub struct CameraStream {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    alive: Receiver<()>,
}

impl CameraStream {
    /// Ouvre la caméra à sa meilleure résolution (en gardant un débit fluide)
    /// et démarre la capture. `on_frame` est appelé depuis le fil de capture
    /// à chaque nouvelle image (sert à demander un rafraîchissement de l'UI).
    pub fn open(device: &DeviceInfo, on_frame: impl Fn() + Send + 'static) -> Result<Self, CameraError> {
        Self::open_impl(device, None, Box::new(on_frame))
    }

    /// Ouvre la caméra à une résolution donnée (voir [`list_modes`]), avec le
    /// meilleur format disponible pour cette résolution.
    pub fn open_with_resolution(
        device: &DeviceInfo,
        width: u32,
        height: u32,
        on_frame: impl Fn() + Send + 'static,
    ) -> Result<Self, CameraError> {
        Self::open_impl(device, Some((width, height)), Box::new(on_frame))
    }

    fn open_impl(
        device: &DeviceInfo,
        wanted: Option<(u32, u32)>,
        on_frame: Box<dyn Fn() + Send>,
    ) -> Result<Self, CameraError> {
        let shared = Arc::new(Shared::default());
        let (tx, alive) = channel();
        let publisher = Publisher {
            shared: shared.clone(),
            on_frame,
            seq: 0,
            spare: None,
            last: None,
            fps: 0.0,
            _alive: tx,
        };
        let thread = catch_unwind(AssertUnwindSafe(|| backend::start(device, wanted, publisher)))
            .unwrap_or_else(|_| Err(CameraError::Open("erreur interne du pilote".into())))?;
        Ok(Self { shared, thread: Some(thread), alive })
    }

    /// Dernière image reçue, si elle est nouvelle depuis le dernier appel.
    pub fn take_frame(&self) -> Option<Arc<Frame>> {
        let f = lock(&self.shared.latest).clone()?;
        let prev = self.shared.taken.fetch_max(f.seq, Ordering::Relaxed);
        (f.seq > prev).then_some(f)
    }

    /// Dernière image reçue (même déjà vue).
    pub fn latest(&self) -> Option<Arc<Frame>> {
        lock(&self.shared.latest).clone()
    }

    /// Erreur survenue dans le fil de capture (caméra débranchée…).
    pub fn error(&self) -> Option<String> {
        lock(&self.shared.error).clone()
    }

    /// Résolution négociée avec la caméra (connue dès l'ouverture).
    pub fn resolution(&self) -> Option<(u32, u32)> {
        self.mode().map(|m| (m.width, m.height))
    }

    /// Mode vidéo négocié avec la caméra.
    pub fn mode(&self) -> Option<VideoMode> {
        *lock(&self.shared.mode)
    }

    /// Cadence réellement mesurée (images/s), une fois quelques images reçues.
    pub fn measured_fps(&self) -> Option<f32> {
        let f = f32::from_bits(self.shared.fps_bits.load(Ordering::Relaxed));
        (f > 0.0).then_some(f)
    }
}

impl Drop for CameraStream {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Le fil vérifie l'arrêt au moins tous les ~100 ms ; on ne l'attend
        // pas indéfiniment si un pilote reste bloqué (il sera alors détaché).
        match self.alive.recv_timeout(Duration::from_millis(1500)) {
            Err(RecvTimeoutError::Disconnected) | Ok(()) => {
                if let Some(t) = self.thread.take() {
                    let _ = t.join();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(w: u32, h: u32, fps: f32, format: PixelFormat) -> (VideoMode, ()) {
        (VideoMode { width: w, height: h, fps, format }, ())
    }

    #[test]
    fn choix_du_mode() {
        use PixelFormat::*;
        // Profil d'une C920 : YUYV 1080p à 5 i/s, MJPEG 1080p à 30 i/s.
        let modes = vec![
            m(640, 480, 30.0, Yuyv),
            m(1920, 1080, 5.0, Yuyv),
            m(2304, 1536, 2.0, Yuyv),
            m(1920, 1080, 30.0, Mjpeg),
            m(640, 480, 30.0, Mjpeg),
        ];
        let r = rank_modes(modes.clone(), None);
        assert_eq!((r[0].0.width, r[0].0.format), (1920, Mjpeg));
        let r = rank_modes(modes.clone(), Some((640, 480)));
        assert_eq!(r[0].0.format, Yuyv);
        // Sans mode fluide : la plus haute cadence.
        let r = rank_modes(vec![m(2304, 1536, 2.0, Yuyv), m(1920, 1080, 5.0, Yuyv)], None);
        assert_eq!(r[0].0.width, 1920);
        // La couleur passe avant les niveaux de gris.
        let r = rank_modes(vec![m(1280, 800, 30.0, Gray), m(640, 480, 30.0, Yuyv)], None);
        assert_eq!(r[0].0.format, Yuyv);
    }
}
