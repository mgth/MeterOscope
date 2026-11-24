//! Essai sur matériel : liste les caméras, ouvre la première (ou celle dont
//! l'index est donné), mesure la cadence, enregistre une image PNG, puis
//! vérifie fermeture → réouverture immédiate.
//!
//! cargo run -p meteroscope-camera --example snap -- <sortie.png> [index] [LxH]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use meteroscope_camera::{CameraStream, list_devices, list_modes};

fn wait_frames(stream: &CameraStream, counter: &AtomicU64, n: u64, timeout: Duration) -> bool {
    let t0 = Instant::now();
    while counter.load(Ordering::Relaxed) < n {
        if let Some(e) = stream.error() {
            eprintln!("Erreur de capture : {e}");
            return false;
        }
        if t0.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(out) = args.get(1) else {
        eprintln!("usage : snap <sortie.png> [index] [LxH]");
        std::process::exit(2);
    };
    let wanted_index: Option<u32> = args.get(2).and_then(|s| s.parse().ok());
    let wanted_res = args.get(3).and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });

    let t = Instant::now();
    let devices = list_devices();
    println!("{} caméra(s) en {:?} :", devices.len(), t.elapsed());
    for d in &devices {
        let modes = list_modes(d);
        let summary: Vec<String> =
            modes.iter().take(6).map(|m| format!("{}×{} {} {:.0} i/s", m.width, m.height, m.format.name(), m.fps)).collect();
        println!("  [{}] {} — {}", d.index, d.name, summary.join(", "));
    }
    let device = match wanted_index {
        Some(i) => devices.iter().find(|d| d.index == i),
        None => devices.first(),
    };
    let Some(device) = device.cloned() else {
        eprintln!("Aucune caméra correspondante.");
        std::process::exit(1);
    };

    let counter = Arc::new(AtomicU64::new(0));
    let c = counter.clone();
    let t = Instant::now();
    let opened = match wanted_res {
        Some((w, h)) => CameraStream::open_with_resolution(&device, w, h, move || {
            c.fetch_add(1, Ordering::Relaxed);
        }),
        None => CameraStream::open(&device, move || {
            c.fetch_add(1, Ordering::Relaxed);
        }),
    };
    let stream = match opened {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Échec : {e}");
            std::process::exit(1);
        }
    };
    println!("Ouverte en {:?}, mode {:?}", t.elapsed(), stream.mode());
    let t = Instant::now();
    if !wait_frames(&stream, &counter, 1, Duration::from_secs(8)) {
        eprintln!("Aucune image reçue.");
        std::process::exit(1);
    }
    println!("Première image après {:?}", t.elapsed());
    // Seconde ouverture concurrente : doit échouer proprement (appareil occupé).
    match CameraStream::open(&device, || {}) {
        Ok(_) => println!("Seconde ouverture simultanée : acceptée"),
        Err(e) => println!("Seconde ouverture simultanée : {e}"),
    }
    // Environ deux secondes de capture pour mesurer la cadence.
    let n0 = counter.load(Ordering::Relaxed);
    let t = Instant::now();
    std::thread::sleep(Duration::from_secs(2));
    let n = counter.load(Ordering::Relaxed) - n0;
    println!(
        "{n} images en {:.2} s → {:.1} i/s (estimation interne {:?})",
        t.elapsed().as_secs_f32(),
        n as f32 / t.elapsed().as_secs_f32(),
        stream.measured_fps()
    );
    let f = stream.take_frame().expect("image");
    println!("Image n° {} : {}×{}", f.seq, f.width, f.height);
    assert!(stream.take_frame().is_none() || counter.load(Ordering::Relaxed) > f.seq);
    let img = image::RgbaImage::from_raw(f.width, f.height, f.rgba.clone()).expect("tampon");
    img.save(out).expect("écriture PNG");
    println!("Enregistrée dans {out}");

    // Fermeture puis réouverture immédiate.
    let t = Instant::now();
    drop(stream);
    println!("Fermeture en {:?}", t.elapsed());
    counter.store(0, Ordering::Relaxed);
    let c = counter.clone();
    let t = Instant::now();
    let again = CameraStream::open(&device, move || {
        c.fetch_add(1, Ordering::Relaxed);
    });
    match again {
        Ok(s) => {
            let ok = wait_frames(&s, &counter, 1, Duration::from_secs(8));
            println!("Réouverture : {} après {:?}", if ok { "image reçue" } else { "AUCUNE image" }, t.elapsed());
            let t = Instant::now();
            drop(s);
            println!("Fermeture en {:?}", t.elapsed());
        }
        Err(e) => println!("Réouverture impossible : {e}"),
    }
}
