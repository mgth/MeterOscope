//! Tests de la détection sur images synthétiques.

use super::*;
use meteroscope_core::geometry::polygon_area;
use std::f64::consts::PI;

const BG: u8 = 40;
const FG: u8 = 200;

/// Image unie `bg` avec des disques `(cx, cy, r)` de valeur `fg`
/// (pixel dans le disque si son centre y est).
fn disks(w: u32, h: u32, bg: u8, fg: u8, list: &[(f64, f64, f64)]) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let inside = list.iter().any(|&(cx, cy, r)| (px - cx).hypot(py - cy) <= r);
        let v = if inside { fg } else { bg };
        image::Rgba([v, v, v, 255])
    })
}

/// Bruit uniforme ±`amp`, pseudo-aléatoire et reproductible.
fn add_noise(img: &mut RgbaImage, amp: i32) {
    let mut s: u64 = 0x2545_F491_4F6C_DD1D;
    for p in img.pixels_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let n = (s % (2 * amp as u64 + 1)) as i32 - amp;
        let v = (p[0] as i32 + n).clamp(0, 255) as u8;
        *p = image::Rgba([v, v, v, 255]);
    }
}

/// Nombre exact de pixels d'un disque (même règle que `disks`).
fn disk_pixels(cx: f64, cy: f64, r: f64) -> f64 {
    let mut n = 0;
    for y in (cy - r - 1.0) as i64..=(cy + r + 1.0) as i64 {
        for x in (cx - r - 1.0) as i64..=(cx + r + 1.0) as i64 {
            if (x as f64 + 0.5 - cx).hypot(y as f64 + 0.5 - cy) <= r {
                n += 1;
            }
        }
    }
    n as f64
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b
}

/// Disques isolés de rayons connus, un disque coupé par le bord, deux
/// disques qui se touchent.
fn scene() -> (RgbaImage, Vec<(f64, f64, f64)>) {
    let isolated = vec![(60.0, 60.0, 8.0), (160.0, 70.0, 15.0), (300.0, 80.0, 25.0), (90.0, 200.0, 40.0), (330.0, 220.0, 12.5)];
    let mut all = isolated.clone();
    all.push((4.0, 150.0, 18.0)); // touche le bord gauche
    all.push((200.0, 230.0, 20.0)); // deux disques qui se touchent
    all.push((238.0, 230.0, 18.0));
    (disks(400, 300, BG, FG, &all), isolated)
}

fn params(threshold: u8, bright: bool) -> DetectParams {
    DetectParams { threshold, bright, min_area: 20.0, ..Default::default() }
}

/// Vérifie les objets isolés (dans l'ordre de lecture) : aire en pixels
/// exacte à `tol_px` près, aire du polygone simplifié et centre.
fn check_isolated(objs: &[Object], isolated: &[(f64, f64, f64)], tol_px: f64, tol_poly: f64) {
    for &(cx, cy, r) in isolated {
        let o = objs.iter().find(|o| o.centroid.dist(Pt::new(cx, cy)) < 1.0).unwrap_or_else(|| panic!("disque ({cx}, {cy}) absent : {objs:?}"));
        let exact = disk_pixels(cx, cy, r);
        assert!(rel(o.area, exact) <= tol_px, "r = {r} : {} px pour {exact}", o.area);
        // Le disque discret reste proche de πr² (au pixel près sur le bord).
        assert!(rel(exact, PI * r * r) < 0.04, "r = {r}");
        let poly = polygon_area(&o.polygon);
        assert!(rel(poly, o.area) <= tol_poly, "r = {r} : polygone {poly} pour {} px", o.area);
        assert!(o.polygon.len() >= 8 && o.polygon.len() < (8.0 * r) as usize, "r = {r} : {} sommets", o.polygon.len());
    }
}

#[test]
fn disques_nets() {
    let (img, isolated) = scene();
    let prep = prepare(&img, None, u32::MAX, false);
    let t = prep.otsu();
    assert!((BG..FG).contains(&t), "Otsu {t}");
    let det = detect(&prep, &params(t, true), true);
    // 5 isolés + la paire fusionnée ; le disque du bord est écarté.
    assert_eq!(det.objects.len(), 6, "{:?}", det.objects.iter().map(|o| o.centroid).collect::<Vec<_>>());
    assert_eq!(det.rejected, 1);
    check_isolated(&det.objects, &isolated, 0.0, 0.02);
    let pair = det.objects.iter().find(|o| (o.centroid.y - 230.0).abs() < 2.0 && o.centroid.x > 190.0).unwrap();
    let pair_px = disk_pixels(200.0, 230.0, 20.0) + disk_pixels(238.0, 230.0, 18.0);
    assert!(pair.area <= pair_px && pair.area > 0.95 * pair_px);
    // En gardant les objets du bord : 7.
    let det = detect(&prep, &DetectParams { exclude_border: false, ..params(t, true) }, false);
    assert_eq!((det.objects.len(), det.rejected), (7, 0));
    // Aire maximale : le grand disque (r = 40, ≈ 5027 px) est écarté, pas la paire (≈ 2275 px).
    let det = detect(&prep, &DetectParams { max_area: 3000.0, ..params(t, true) }, false);
    assert_eq!(det.objects.len(), 5);
    // Aire minimale : le petit disque (r = 8, ≈ 200 px) disparaît.
    let det = detect(&prep, &DetectParams { min_area: 300.0, ..params(t, true) }, false);
    assert_eq!(det.objects.len(), 5);
}

#[test]
fn objets_sombres_et_bruit() {
    let (img, isolated) = scene();
    // Objets sombres sur fond clair, bruit ±40.
    let mut dark = img.clone();
    for p in dark.pixels_mut() {
        let v = 255 - p[0];
        *p = image::Rgba([v, v, v, 255]);
    }
    add_noise(&mut dark, 40);
    let prep = prepare(&dark, None, u32::MAX, true);
    let t = prep.otsu();
    assert!((255 - FG..255 - BG).contains(&t), "Otsu {t}");
    let det = detect(&prep, &params(t, false), true);
    assert_eq!(det.objects.len(), 6);
    check_isolated(&det.objects, &isolated, 0.06, 0.03);
    // Sans lissage, le bruit isolé est filtré par l'aire minimale.
    let prep = prepare(&dark, None, u32::MAX, false);
    let det = detect(&prep, &params(prep.otsu(), false), false);
    assert_eq!(det.objects.len(), 6);
}

#[test]
fn trous_bouches_ou_non() {
    // Anneau R = 30, r = 12.
    let ring = RgbaImage::from_fn(100, 100, |x, y| {
        let d = (x as f64 + 0.5 - 50.0).hypot(y as f64 + 0.5 - 50.0);
        let v = if d <= 30.0 && d > 12.0 { FG } else { BG };
        image::Rgba([v, v, v, 255])
    });
    let prep = prepare(&ring, None, u32::MAX, false);
    let full = disk_pixels(50.0, 50.0, 30.0);
    let hole = disk_pixels(50.0, 50.0, 12.0);
    let filled = detect(&prep, &params(120, true), true);
    let open = detect(&prep, &DetectParams { fill_holes: false, ..params(120, true) }, true);
    assert_eq!((filled.objects.len(), open.objects.len()), (1, 1));
    assert_eq!(filled.objects[0].area, full);
    assert_eq!(open.objects[0].area, full - hole);
    // Trou bouché : le polygone englobe le trou ; trou gardé : il le contourne
    // en trou de serrure et son aire suit celle des pixels.
    let rel = |o: &Object| (polygon_area(&o.polygon) - o.area).abs() / o.area;
    assert!(rel(&filled.objects[0]) < 0.01 && rel(&open.objects[0]) < 0.01);
    assert!(open.objects[0].polygon.len() > filled.objects[0].polygon.len());
}

#[test]
fn trou_de_serrure_exact() {
    // Carré 20 × 20 évidé de 10 × 10 (cas de la relecture), et un îlot de
    // 2 × 2 dans un second trou : contours carrés, aucune simplification.
    let img = RgbaImage::from_fn(60, 40, |x, y| {
        let square = (5..25).contains(&x) && (5..25).contains(&y) && !((10..20).contains(&x) && (10..20).contains(&y));
        let frame = (30..55).contains(&x) && (5..30).contains(&y) && !((33..52).contains(&x) && (8..27).contains(&y));
        let island = (40..42).contains(&x) && (15..17).contains(&y);
        let v = if square || frame || island { FG } else { BG };
        image::Rgba([v, v, v, 255])
    });
    let prep = prepare(&img, None, u32::MAX, false);
    let p = DetectParams { fill_holes: false, min_area: 1.0, ..params(120, true) };
    let det = detect(&prep, &p, true);
    assert_eq!(det.objects.len(), 3);
    let areas: Vec<(f64, f64)> = det.objects.iter().map(|o| (o.area, polygon_area(&o.polygon))).collect();
    assert_eq!(areas, [(300.0, 300.0), (25.0 * 25.0 - 19.0 * 19.0, 25.0 * 25.0 - 19.0 * 19.0), (4.0, 4.0)]);
    // Un filtre d'aire max. à 350 garde le carré évidé, mesuré 300 px².
    let det = detect(&prep, &DetectParams { max_area: 350.0, ..p }, true);
    assert_eq!(polygon_area(&det.objects[0].polygon), 300.0);
    // Périmètre : bords extérieur (80) et du trou (40), sans la fente.
    assert_eq!(meteroscope_core::geometry::polygon_perimeter(&det.objects[0].polygon), 120.0);
    // Trous bouchés : le contour extérieur seul.
    let det = detect(&prep, &DetectParams { fill_holes: true, ..p }, true);
    assert_eq!((det.objects[0].area, polygon_area(&det.objects[0].polygon)), (400.0, 400.0));
}

#[test]
fn contour_exact_et_diagonales() {
    // Pixels reliés seulement par un coin (8-connexité) : un seul objet, et
    // l'aire du contour non simplifié vaut exactement le nombre de pixels.
    let pts = [(2, 2), (3, 3), (4, 4), (5, 3), (6, 2), (6, 3), (2, 8), (3, 8), (4, 8), (4, 7)];
    let (w, h) = (10, 10);
    let mut mask = vec![false; w * h];
    for (x, y) in pts {
        mask[y * w + x] = true;
    }
    let (labels, comps) = label(&mask, w, h, None);
    assert_eq!(comps.len(), 2);
    let a = &comps[0];
    assert_eq!((a.count, a.first), (6, (2, 2)));
    for (i, c) in comps.iter().enumerate() {
        let raw: Vec<Pt> = trace_contour(&labels, w, h, i as u32 + 1, c.first).into_iter().map(|(x, y)| Pt::new(x as f64, y as f64)).collect();
        assert_eq!(polygon_area(&raw), c.count as f64);
    }
    // Pixel isolé : un carré.
    let mut one = vec![false; 9];
    one[4] = true;
    let (labels, comps) = label(&one, 3, 3, None);
    assert_eq!(trace_contour(&labels, 3, 3, 1, comps[0].first), vec![(1, 1), (2, 1), (2, 2), (1, 2)]);
}

#[test]
fn simplification() {
    // Ligne en escalier : réduite à ses extrémités ; carré conservé.
    let sq = vec![Pt::new(0., 0.), Pt::new(10., 0.), Pt::new(10., 10.), Pt::new(0., 10.)];
    assert_eq!(simplify_closed(&sq, 1.0), sq);
    let mut stairs = vec![];
    for i in 0..10 {
        stairs.push(Pt::new(i as f64, i as f64));
        stairs.push(Pt::new(i as f64 + 1.0, i as f64));
    }
    stairs.push(Pt::new(10., 10.));
    stairs.push(Pt::new(0., 10.));
    let s = simplify_closed(&stairs, 1.0);
    assert!(s.len() <= 4, "{s:?}");
    // Rectangle d'un pixel de large : pas de polygone dégénéré.
    let thin = vec![Pt::new(0., 0.), Pt::new(20., 0.), Pt::new(20., 1.), Pt::new(0., 1.)];
    assert_eq!(simplify_closed(&thin, 1.0).len(), 4);
}

#[test]
fn otsu_bimodal() {
    let mut h = [0u64; 256];
    h[30] = 1000;
    h[31] = 500;
    h[180] = 800;
    h[181] = 300;
    let t = otsu(&h);
    assert!((31..180).contains(&t), "{t}");
    assert_eq!(otsu(&[0; 256]), 128);
}

#[test]
fn zone_polygonale() {
    let (img, _) = scene();
    // Zone rectangulaire autour des deux disques du haut à droite ; le
    // disque r = 25 est coupé par le bord de la zone.
    let zone = vec![Pt::new(120., 30.), Pt::new(290., 30.), Pt::new(290., 120.), Pt::new(120., 120.)];
    let prep = prepare(&img, Some(&zone), u32::MAX, false);
    assert_eq!((prep.origin, prep.width, prep.height), ((120, 30), 170, 90));
    let det = detect(&prep, &params(120, true), true);
    assert_eq!(det.objects.len(), 1);
    assert!(det.objects[0].centroid.dist(Pt::new(160., 70.)) < 0.5);
    assert_eq!(det.rejected, 1);
    // Zone triangulaire inclinée : le disque r = 15 est entièrement dedans.
    let tri = vec![Pt::new(100., 20.), Pt::new(260., 60.), Pt::new(110., 140.)];
    let det = detect(&prepare(&img, Some(&tri), u32::MAX, false), &params(120, true), true);
    assert_eq!(det.objects.len(), 1);
    assert_eq!(det.objects[0].area, disk_pixels(160., 70., 15.));
    // Les sommets restent dans le repère de l'image.
    assert!(det.objects[0].polygon.iter().all(|p| p.x >= 144.0 && p.x <= 176.0));
}

#[test]
fn apercu_reduit() {
    let (img, isolated) = scene();
    // Aperçu à 1/4 : mêmes objets, aires approchées.
    let prep = prepare(&img, None, 100, false);
    assert_eq!((prep.scale, prep.width, prep.height), (4, 100, 75));
    let p = params(120, true);
    let (overlay, n) = preview(&prep, &p);
    assert_eq!((overlay.width(), overlay.height(), n), (100, 75, 6));
    let det = detect(&prep, &p, false);
    assert_eq!(det.objects.len(), 6);
    for &(cx, cy, r) in &isolated[1..] {
        let o = det.objects.iter().find(|o| o.centroid.dist(Pt::new(cx, cy)) < 3.0).unwrap();
        assert!(rel(o.area, PI * r * r) < 0.25, "r = {r} : {}", o.area);
    }
    // Couleurs : objet retenu au centre d'un disque, écarté pour celui du bord.
    assert_eq!(overlay.get_pixel(75, 20).0, PREVIEW_KEPT);
    assert_eq!(overlay.get_pixel(0, 37).0, PREVIEW_REJECTED);
    assert_eq!(overlay.get_pixel(50, 5).0[3], 0);
}

#[test]
fn calcul_en_fil_identique() {
    let (img, _) = scene();
    let zone = vec![Pt::new(20.5, 20.5), Pt::new(390., 25.), Pt::new(380., 290.), Pt::new(30., 280.)];
    let p = params(120, true);
    let rx = spawn_detection(&img, Some(zone.clone()), true, p);
    let det = rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    let direct = detect(&prepare(&img, Some(&zone), u32::MAX, true), &p, true);
    assert_eq!(det.objects, direct.objects);
    assert_eq!(det.objects.len(), 6);
}

#[test]
fn grande_image_multi_fils() {
    // Assez grande pour passer par les fils de calcul : résultat identique.
    let mut list = vec![];
    for i in 0..20 {
        for j in 0..15 {
            list.push((30.0 + i as f64 * 60.0, 30.0 + j as f64 * 60.0, 10.0 + (i + j) as f64 % 15.0));
        }
    }
    let img = disks(1200, 900, BG, FG, &list);
    let prep = prepare(&img, None, u32::MAX, true);
    // Référence : lissage naïf sur un seul fil.
    let raw = prepare(&img, None, u32::MAX, false).luma;
    let (w, h) = (1200i64, 900i64);
    let at = |x: i64, y: i64| raw[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize] as u32;
    let k = [1, 2, 1];
    let seq: Vec<u8> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let mut s = 0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    s += k[(dx + 1) as usize] * k[(dy + 1) as usize] * at(x + dx, y + dy);
                }
            }
            ((s + 8) / 16) as u8
        })
        .collect();
    assert_eq!(prep.luma, seq);
    let det = detect(&prep, &params(prep.otsu(), true), true);
    assert_eq!(det.objects.len(), 300);
}

/// Écart entre l'aire du polygone simplifié et le nombre de pixels, par
/// rayon (affiché avec `cargo test -- --nocapture ecart`).
#[test]
fn ecart_polygone_pixels() {
    let radii = [2.0, 3.0, 5.0, 8.0, 12.0, 20.0, 40.0, 80.0];
    let list: Vec<_> = radii.iter().enumerate().map(|(i, &r)| (100.0 + 200.0 * i as f64, 120.0, r)).collect();
    let img = disks(1700, 240, BG, FG, &list);
    let mut det = detect(&prepare(&img, None, u32::MAX, false), &DetectParams { min_area: 1.0, ..params(120, true) }, true);
    assert_eq!(det.objects.len(), radii.len());
    det.objects.sort_by(|a, b| a.centroid.x.total_cmp(&b.centroid.x));
    for (o, r) in det.objects.iter().zip(radii) {
        let poly = polygon_area(&o.polygon);
        let e = (poly - o.area) / o.area;
        println!("r = {r:>4} : {:>6} px, polygone {poly:>9.1} ({:+.2} %), {} sommets", o.area, 100.0 * e, o.polygon.len());
        assert!(e.abs() < if r < 4.0 { 0.08 } else { 0.03 }, "r = {r} : {e}");
    }
}

/// Temps de calcul sur une photo de 24 Mpx (`cargo test --release -- --ignored perf`).
#[test]
#[ignore]
fn perf_24_mpx() {
    let mut list = vec![];
    for i in 0..60 {
        for j in 0..40 {
            list.push((50.0 + i as f64 * 100.0, 50.0 + j as f64 * 100.0, 10.0 + ((i * 7 + j * 3) % 30) as f64));
        }
    }
    let mut img = disks(6000, 4000, BG, FG, &list);
    add_noise(&mut img, 30);
    let t = std::time::Instant::now();
    let prep = prepare(&img, None, 1600, true);
    let t_prep = t.elapsed();
    let p = params(prep.otsu(), true);
    let t = std::time::Instant::now();
    let (_, n) = preview(&prep, &p);
    println!("aperçu 1600 px : préparation {t_prep:?}, seuillage {:?}, {n} objets", t.elapsed());
    let t = std::time::Instant::now();
    let det = spawn_detection(&img, None, true, p).recv().unwrap();
    println!("complet : {:?}, {} objets", t.elapsed(), det.objects.len());
    assert_eq!(det.objects.len(), 2400);
}

#[test]
fn zone_vide() {
    // Zone aplatie mais longue : sans pixel, réduite ou non, trous gardés
    // ou bouchés, la détection est vide au lieu d'échouer.
    let img = RgbaImage::from_pixel(64, 2000, image::Rgba([200, 200, 200, 255]));
    let zone = [Pt::new(10., 10.), Pt::new(10., 1810.), Pt::new(10., 1810.), Pt::new(10., 10.)];
    for fill_holes in [true, false] {
        for side in [1600, u32::MAX] {
            let p = prepare(&img, Some(&zone), side, true);
            assert!(p.is_empty());
            let d = detect(&p, &DetectParams { fill_holes, ..Default::default() }, true);
            assert!(d.objects.is_empty() && d.rejected == 0);
        }
    }
}
