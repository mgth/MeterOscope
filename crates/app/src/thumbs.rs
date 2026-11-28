//! Vignettes de la série : réduites et calculées hors du fil de l'interface,
//! par quatre fils au plus (une photo de 24 Mpx se décode en entier pour
//! être réduite : on ne veut ni figer l'interface ni saturer la machine).

use egui::{TextureHandle, TextureOptions};
use image::RgbaImage;
use meteroscope_core::Document;
use meteroscope_io as io;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Plus grand côté d'une vignette, en pixels.
const THUMB_SIDE: u32 = 192;
/// Nombre maximal de fils de calcul.
const MAX_WORKERS: usize = 4;

struct Job {
    generation: u64,
    id: u64,
    path: PathBuf,
    /// Rotations et miroirs appliqués à l'image depuis sa lecture.
    transforms: Vec<io::Transform>,
    ctx: egui::Context,
}

/// (génération, identifiant, vignette ou `None` si l'image est illisible).
type Done = (u64, u64, Option<RgbaImage>);

/// Lit une image et lui réapplique ses rotations et miroirs.
pub fn load_transformed(path: &std::path::Path, transforms: &[io::Transform]) -> anyhow::Result<RgbaImage> {
    let mut img = io::load_image(path)?;
    for t in transforms {
        // Document vide : seule l'image nous intéresse ici.
        img = io::apply_transform(&img, &mut Document::new(), *t);
    }
    Ok(img)
}

pub struct Thumbs {
    textures: HashMap<u64, TextureHandle>,
    pending: HashSet<u64>,
    failed: HashSet<u64>,
    /// Change à chaque nouvelle série : les travaux d'avant sont ignorés.
    generation: Arc<AtomicU64>,
    jobs: Option<Sender<Job>>,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
}

impl Default for Thumbs {
    fn default() -> Self {
        let (done_tx, done_rx) = channel();
        Self {
            textures: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            generation: Arc::new(AtomicU64::new(0)),
            jobs: None,
            done_tx,
            done_rx,
        }
    }
}

impl Thumbs {
    /// Oublie tout (nouvelle série).
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.textures.clear();
        self.pending.clear();
        self.failed.clear();
    }

    pub fn get(&self, id: u64) -> Option<&TextureHandle> {
        self.textures.get(&id)
    }

    pub fn has(&self, id: u64) -> bool {
        self.textures.contains_key(&id)
    }

    /// Le calcul a échoué (image illisible).
    pub fn failed(&self, id: u64) -> bool {
        self.failed.contains(&id)
    }

    fn store(&mut self, ctx: &egui::Context, id: u64, thumb: &RgbaImage) {
        let ci = egui::ColorImage::from_rgba_unmultiplied([thumb.width() as usize, thumb.height() as usize], thumb.as_raw());
        self.textures.insert(id, ctx.load_texture(format!("vignette-{id}"), ci, TextureOptions::LINEAR));
        self.failed.remove(&id);
    }

    /// Vignette calculée tout de suite à partir d'une image déjà en mémoire.
    pub fn set(&mut self, ctx: &egui::Context, id: u64, image: &RgbaImage) {
        self.pending.remove(&id);
        self.store(ctx, id, &io::thumbnail(image, THUMB_SIDE));
    }

    /// Demande la vignette d'un fichier, si elle n'est ni prête ni en cours.
    pub fn request(&mut self, ctx: &egui::Context, id: u64, path: PathBuf, transforms: Vec<io::Transform>) {
        if self.has(id) || self.failed(id) || !self.pending.insert(id) {
            return;
        }
        let generation = self.generation.load(Ordering::Relaxed);
        let job = Job { generation, id, path, transforms, ctx: ctx.clone() };
        let _ = self.workers().send(job);
    }

    /// Lance les fils de calcul au premier besoin. Ils s'arrêtent quand
    /// l'application ferme (l'émetteur disparaît).
    fn workers(&mut self) -> &Sender<Job> {
        self.jobs.get_or_insert_with(|| {
            let (tx, rx) = channel::<Job>();
            let rx = Arc::new(Mutex::new(rx));
            let n = std::thread::available_parallelism().map_or(2, |n| n.get()).min(MAX_WORKERS);
            for k in 0..n {
                let (rx, done, generation) = (rx.clone(), self.done_tx.clone(), self.generation.clone());
                let _ = std::thread::Builder::new().name(format!("vignettes-{k}")).spawn(move || {
                    loop {
                        let job = rx.lock().map(|r| r.recv());
                        let Ok(Ok(job)) = job else { break };
                        if job.generation != generation.load(Ordering::Relaxed) {
                            continue;
                        }
                        let thumb = load_transformed(&job.path, &job.transforms).ok().map(|img| io::thumbnail(&img, THUMB_SIDE));
                        if done.send((job.generation, job.id, thumb)).is_err() {
                            break;
                        }
                        job.ctx.request_repaint();
                    }
                });
            }
            tx
        })
    }

    /// Récupère les vignettes terminées (à appeler à chaque image d'interface).
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((generation, id, thumb)) = self.done_rx.try_recv() {
            if generation != self.generation.load(Ordering::Relaxed) || !self.pending.remove(&id) {
                continue;
            }
            match thumb {
                Some(t) => self.store(ctx, id, &t),
                None => {
                    self.failed.insert(id);
                }
            }
        }
    }
}
