//! Tests sur fichiers (dossier temporaire sous `crates/io/target-tests/`).

use image::{ImageEncoder, Rgba, RgbaImage};
use meteroscope_core::{Document, Pt, Shape};
use meteroscope_io::*;
use std::path::{Path, PathBuf};

/// Dossier de test propre à chaque test, supprimé à la fin.
struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> TestDir {
        // `absolute` : sous Wine, CARGO_MANIFEST_DIR reste un chemin Unix
        // alors que l'application voit `Z:\…`.
        let d = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("target-tests").join(name)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TestDir(d)
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        // Supprime le dossier parent s'il est vide.
        if let Some(p) = self.0.parent() {
            let _ = std::fs::remove_dir(p);
        }
    }
}

fn sample_doc() -> Document {
    let mut d = Document::new();
    d.add(Shape::Length { a: Pt::new(1.0, 2.0), b: Pt::new(30.5, 40.25) });
    d.add(Shape::Area { pts: vec![Pt::new(0., 0.), Pt::new(10., 0.), Pt::new(5., 8.)] });
    d.add(Shape::Marker { p: Pt::new(3.0, 4.0) });
    d.calibration.set_from_reference(100.0, 2.5, "mm");
    d
}

#[test]
fn project_round_trip_relative() {
    meteroscope_i18n::set_language("fr");
    let dir = TestDir::new("projet");
    std::fs::create_dir_all(dir.path("a/images")).unwrap();
    let img = dir.path("a/images/photo.png");
    std::fs::write(&img, b"x").unwrap();
    let proj = dir.path("a/essai.meteroscope");
    let doc = sample_doc();
    save_project(&Project { image_path: Some(img.clone()), document: doc.clone() }, &proj).unwrap();

    let text = std::fs::read_to_string(&proj).unwrap();
    assert!(text.contains("\"version\": 1"));
    assert!(text.contains("\"image\": \"images/photo.png\""), "{text}");

    let p = load_project(&proj).unwrap();
    assert_eq!(p.image_path.as_deref(), Some(img.as_path()));
    assert!(p.image_path.unwrap().is_absolute());
    assert_eq!(p.document.measurements, doc.measurements);
    assert_eq!(p.document.calibration, doc.calibration);
    // Les compteurs de noms sont conservés.
    let mut d2 = p.document;
    let id = d2.add(Shape::Marker { p: Pt::default() });
    assert_eq!(d2.get(id).unwrap().name, "Marqueur 2");

    // On déplace projet + image ensemble : le chemin suit.
    std::fs::rename(dir.path("a"), dir.path("b")).unwrap();
    let p = load_project(&dir.path("b/essai.meteroscope")).unwrap();
    assert_eq!(p.image_path.as_deref(), Some(dir.path("b/images/photo.png").as_path()));

    // Image à côté, dossier parent.
    std::fs::create_dir_all(dir.path("b/sous")).unwrap();
    let proj2 = dir.path("b/sous/p.meteroscope");
    save_project(&Project { image_path: Some(dir.path("b/images/photo.png")), document: Document::new() }, &proj2).unwrap();
    assert!(std::fs::read_to_string(&proj2).unwrap().contains("\"image\": \"../images/photo.png\""));
    assert_eq!(load_project(&proj2).unwrap().image_path, Some(dir.path("b/images/photo.png")));

    // Sans image.
    save_project(&Project { image_path: None, document: Document::new() }, &proj2).unwrap();
    assert_eq!(load_project(&proj2).unwrap().image_path, None);
    // Pas de fichier temporaire résiduel.
    assert!(!dir.path("b/sous/p.meteroscope.tmp").exists());
}

#[test]
fn project_moved_alone_falls_back_to_absolute() {
    meteroscope_i18n::set_language("fr");
    let dir = TestDir::new("projet-seul");
    let img = dir.path("photo.png");
    std::fs::write(&img, b"x").unwrap();
    save_project(&Project { image_path: Some(img.clone()), document: Document::new() }, &dir.path("p.meteroscope")).unwrap();
    std::fs::create_dir_all(dir.path("ailleurs")).unwrap();
    std::fs::rename(dir.path("p.meteroscope"), dir.path("ailleurs/p.meteroscope")).unwrap();
    assert_eq!(load_project(&dir.path("ailleurs/p.meteroscope")).unwrap().image_path, Some(img));
}

#[test]
fn project_errors() {
    meteroscope_i18n::set_language("fr");
    let dir = TestDir::new("projet-erreurs");
    let err = |content: &str| {
        let p = dir.path("x.meteroscope");
        std::fs::write(&p, content).unwrap();
        format!("{:#}", load_project(&p).unwrap_err())
    };
    let e = err("{\"format\":\"meteroscope\",\"version\":99,\"document\":{}}");
    assert!(e.contains("version 99 inconnu") && e.contains("x.meteroscope"), "{e}");
    assert!(err("pas du json").contains("JSON invalide"));
    assert!(err("{\"a\":1}").contains("pas un projet MeterOscope"));
    let e = err("{\"format\":\"meteroscope\",\"version\":1,\"document\":{\"measurements\":12}}");
    assert!(e.contains("contenu du projet invalide"), "{e}");
    let e = format!("{:#}", load_project(&dir.path("absent.meteroscope")).unwrap_err());
    assert!(e.contains("introuvable"), "{e}");
}

fn quadrants(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| match (x < w / 2, y < h / 2) {
        (true, true) => Rgba([255, 0, 0, 255]),
        (false, true) => Rgba([0, 255, 0, 255]),
        (true, false) => Rgba([0, 0, 255, 255]),
        (false, false) => Rgba([255, 255, 255, 255]),
    })
}

#[test]
fn save_and_load_images() {
    meteroscope_i18n::set_language("fr");
    let dir = TestDir::new("images");
    let mut img = quadrants(32, 16);
    img.put_pixel(0, 0, Rgba([1, 2, 3, 128]));
    for ext in ["png", "tif", "webp", "bmp", "gif"] {
        let p = dir.path(&format!("i.{ext}"));
        save_image(&img, &p).unwrap();
        let back = load_image(&p).unwrap();
        assert_eq!(back.dimensions(), (32, 16), "{ext}");
        if ext == "png" || ext == "tif" || ext == "webp" {
            assert_eq!(back, img, "{ext}");
        }
    }
    let p = dir.path("i.JPG");
    save_image(&img, &p).unwrap();
    let back = load_image(&p).unwrap();
    assert_eq!(back.dimensions(), (32, 16));
    let px = back.get_pixel(24, 12);
    assert!(px.0.iter().all(|&c| c > 240));
    // Extension trompeuse : le contenu prime.
    std::fs::copy(dir.path("i.png"), dir.path("vraiment-png.jpg")).unwrap();
    assert_eq!(load_image(&dir.path("vraiment-png.jpg")).unwrap(), img);

    let e = format!("{:#}", save_image(&img, &dir.path("sans-extension")).unwrap_err());
    assert!(e.contains("extension manquante"), "{e}");
    let e = format!("{:#}", save_image(&img, &dir.path("x.xyz")).unwrap_err());
    assert!(e.contains("non pris en charge"), "{e}");
    let e = format!("{:#}", load_image(&dir.path("absent.png")).unwrap_err());
    assert!(e.contains("absent.png") && e.contains("introuvable"), "{e}");
    std::fs::write(dir.path("faux.png"), b"ceci n'est pas une image").unwrap();
    let e = format!("{:#}", load_image(&dir.path("faux.png")).unwrap_err());
    assert!(e.contains("faux.png"), "{e}");
    std::fs::write(dir.path("tronque.png"), &std::fs::read(dir.path("i.png")).unwrap()[..60]).unwrap();
    assert!(load_image(&dir.path("tronque.png")).is_err());
}

/// Bloc EXIF minimal (TIFF petit-boutiste) avec la seule balise Orientation.
fn exif_orientation(value: u16) -> Vec<u8> {
    let mut v = b"II*\0".to_vec();
    v.extend(8u32.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(0x0112u16.to_le_bytes());
    v.extend(3u16.to_le_bytes());
    v.extend(1u32.to_le_bytes());
    v.extend(value.to_le_bytes());
    v.extend([0, 0]);
    v.extend(0u32.to_le_bytes());
    v
}

#[test]
fn exif_orientation_is_applied() {
    meteroscope_i18n::set_language("fr");
    let dir = TestDir::new("exif");
    let (w, h) = (6u32, 4u32);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([50, 50, 50, 255]));
    img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
    // PNG (sans perte) : orientation 6 = rotation de 90° horaire.
    let p = dir.path("o6.png");
    let mut f = std::fs::File::create(&p).unwrap();
    let mut enc = image::codecs::png::PngEncoder::new(&mut f);
    enc.set_exif_metadata(exif_orientation(6)).unwrap();
    enc.write_image(&img, w, h, image::ExtendedColorType::Rgba8).unwrap();
    drop(f);
    let back = load_image(&p).unwrap();
    assert_eq!(back.dimensions(), (h, w));
    assert_eq!(*back.get_pixel(h - 1, 0), Rgba([255, 0, 0, 255]));

    // JPEG (cas réel des appareils photo) : orientation 8 = 90° anti-horaire.
    let q = quadrants(64, 32);
    let p = dir.path("o8.jpg");
    let mut f = std::fs::File::create(&p).unwrap();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut f, 95);
    enc.set_exif_metadata(exif_orientation(8)).unwrap();
    let rgb = image::DynamicImage::ImageRgba8(q).to_rgb8();
    enc.write_image(&rgb, 64, 32, image::ExtendedColorType::Rgb8).unwrap();
    drop(f);
    let back = load_image(&p).unwrap();
    assert_eq!(back.dimensions(), (32, 64));
    // Le quadrant rouge (haut gauche) se retrouve en bas à gauche.
    let px = back.get_pixel(8, 56);
    assert!(px[0] > 200 && px[1] < 60 && px[2] < 60, "{px:?}");
}

#[test]
fn export_csv_file_has_bom() {
    meteroscope_i18n::set_language("fr");
    let dir = TestDir::new("csv");
    let p = dir.path("m.csv");
    export_csv(&sample_doc(), &p).unwrap();
    let bytes = std::fs::read(&p).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(export_csv(&sample_doc(), &dir.path("absent/m.csv")).is_err());
}

/// Rendu de démonstration à regarder à l'œil : `METEROSCOPE_RENDER_DEMO=<dossier> cargo test`.
#[test]
fn render_demo() {
    meteroscope_i18n::set_language("fr");
    let Ok(out_dir) = std::env::var("METEROSCOPE_RENDER_DEMO") else { return };
    for (w, h) in [(640u32, 480u32), (4000, 3000)] {
        let k = w as f64 / 640.0;
        let img = RgbaImage::from_fn(w, h, |x, y| {
            let v = (60.0 + 80.0 * (x as f64 / w as f64) + 40.0 * (y as f64 / h as f64)) as u8;
            Rgba([v, v.saturating_add(10), v.saturating_add(25), 255])
        });
        let mut d = Document::new();
        let s = |x: f64, y: f64| Pt::new(x * k, y * k);
        d.add(Shape::Length { a: s(40., 60.), b: s(260., 110.) });
        d.add(Shape::Length { a: s(600., 40.), b: s(600., 300.) });
        d.add(Shape::Angle { a: s(80., 300.), vertex: s(40., 420.), b: s(220., 420.) });
        d.add(Shape::Area { pts: vec![s(300., 300.), s(420., 280.), s(460., 400.), s(330., 430.)] });
        d.add(Shape::Rect { a: s(300., 150.), b: s(450., 240.), angle: 0.0 });
        d.add(Shape::Circle { center: s(150., 200.), edge: s(200., 200.) });
        d.add(Shape::Circle3 { a: s(480., 100.), b: s(540., 120.), c: s(500., 180.) });
        d.add(Shape::Path { pts: vec![s(480., 300.), s(520., 340.), s(500., 380.), s(560., 400.)] });
        for i in 0..4 {
            d.add(Shape::Marker { p: s(250. + 30. * i as f64, 20. + 8. * i as f64) });
        }
        d.calibration.set_from_reference(100.0 * k, 1.0, "µm");
        let out = render_annotated(&img, &d);
        save_image(&out, &Path::new(&out_dir).join(format!("demo-{w}.png"))).unwrap();
    }
}

#[test]
fn exported_image_follows_the_straightened_view() {
    meteroscope_i18n::set_language("fr");
    let img = RgbaImage::from_pixel(200, 100, Rgba([90, 90, 90, 255]));
    let mut doc = Document::new();
    // Segment penché de 30° dans la photo, que le redressement met à l'horizontale.
    let (s, c) = 30f64.to_radians().sin_cos();
    let (a, b) = (Pt::new(100.0 - 60.0 * c, 50.0 + 60.0 * s), Pt::new(100.0 + 60.0 * c, 50.0 - 60.0 * s));
    doc.add(Shape::Length { a, b });
    doc.set_rotation(30.0);
    let filter = RotationFilter::Lanczos3;
    let plain = rotate_image(&img, 30.0, filter);
    let out = render_view(&img, &doc, filter);
    assert_eq!(out.dimensions(), rotated_dimensions(200, 100, 30.0));
    // Le trait suit la rangée centrale de l'image exportée, d'un bout à l'autre.
    let changed = |x: u32, y: u32| out.get_pixel(x, y) != plain.get_pixel(x, y);
    let (cx, cy) = (out.width() / 2, out.height() / 2);
    for dx in [-55i32, -30, 30, 55] {
        let x = (cx as i32 + dx) as u32;
        assert!(changed(x, cy) || changed(x, cy - 1), "trait absent en x = {x}");
        // Rien à l'emplacement que le trait aurait dans la photo non redressée
        // (vers les extrémités, loin de l'étiquette posée au milieu).
        let (ox, oy) = (cx as f64 + dx as f64 * c, cy as f64 - dx as f64 * s);
        assert!(dx.abs() < 50 || !changed(ox as u32, oy as u32), "trait non redressé en ({ox:.0}, {oy:.0})");
    }
    // Sans redressement : l'export habituel.
    let mut flat = Document::new();
    flat.add(Shape::Length { a, b });
    assert_eq!(render_view(&img, &flat, filter), render_annotated(&img, &flat));
}
