//! Conversion des formats de capture usuels vers RGBA 8 bits.

use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::PixelFormat;

/// Décode une image brute dans `out` (redimensionné au besoin).
/// `stride` : octets par ligne du plan principal (0 = valeur compacte).
/// Renvoie les dimensions réelles de l'image décodée.
pub(crate) fn decode(
    format: PixelFormat,
    width: u32,
    height: u32,
    stride: usize,
    data: &[u8],
    out: &mut Vec<u8>,
) -> Result<(u32, u32), String> {
    if format == PixelFormat::Mjpeg {
        return decode_mjpeg(data, out);
    }
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return Err("dimensions nulles".into());
    }
    let bpp = match format {
        PixelFormat::Yuyv | PixelFormat::Uyvy => 2,
        PixelFormat::Rgb | PixelFormat::Bgr => 3,
        PixelFormat::Gray | PixelFormat::Nv12 => 1,
        PixelFormat::Mjpeg => unreachable!(),
    };
    let stride = if stride >= w * bpp { stride } else { w * bpp };
    // NV12 : plan Y puis plan UV entrelacé, sous-échantillonné verticalement.
    let needed = match format {
        PixelFormat::Nv12 => stride * h + stride * (h.div_ceil(2) - 1) + w.next_multiple_of(2),
        _ => stride * (h - 1) + w * bpp,
    };
    if data.len() < needed {
        return Err(format!("image incomplète ({} octets sur {needed})", data.len()));
    }
    out.clear();
    out.resize(w * h * 4, 255);
    for y in 0..h {
        let row = &data[y * stride..];
        let dst = &mut out[y * w * 4..(y + 1) * w * 4];
        match format {
            PixelFormat::Yuyv | PixelFormat::Uyvy => {
                let (iy0, iu, iy1, iv) = if format == PixelFormat::Yuyv { (0, 1, 2, 3) } else { (1, 0, 3, 2) };
                for x in (0..w).step_by(2) {
                    let s = &row[x * 2..];
                    let (u, v) = (s[iu] as i32, s[iv] as i32);
                    put(&mut dst[x * 4..], s[iy0] as i32, u, v);
                    if x + 1 < w {
                        put(&mut dst[(x + 1) * 4..], s[iy1] as i32, u, v);
                    }
                }
            }
            PixelFormat::Nv12 => {
                let uv = &data[stride * h + (y / 2) * stride..];
                for x in 0..w {
                    let c = (x / 2) * 2;
                    put(&mut dst[x * 4..], row[x] as i32, uv[c] as i32, uv[c + 1] as i32);
                }
            }
            PixelFormat::Gray => {
                for (d, &g) in dst.as_chunks_mut::<4>().0.iter_mut().zip(&row[..w]) {
                    d[..3].fill(g);
                }
            }
            PixelFormat::Rgb | PixelFormat::Bgr => {
                let swap = format == PixelFormat::Bgr;
                for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(row[..w * 3].as_chunks::<3>().0) {
                    if swap {
                        d[..3].copy_from_slice(&[s[2], s[1], s[0]]);
                    } else {
                        d[..3].copy_from_slice(s);
                    }
                }
            }
            PixelFormat::Mjpeg => unreachable!(),
        }
    }
    Ok((width, height))
}

/// YCbCr BT.601 « plage réduite » (16–235) vers RGB, en arithmétique entière.
#[inline]
fn put(d: &mut [u8], y: i32, u: i32, v: i32) {
    let c = 298 * (y - 16) + 128;
    let (du, dv) = (u - 128, v - 128);
    d[0] = ((c + 409 * dv) >> 8).clamp(0, 255) as u8;
    d[1] = ((c - 100 * du - 208 * dv) >> 8).clamp(0, 255) as u8;
    d[2] = ((c + 516 * du) >> 8).clamp(0, 255) as u8;
}

fn decode_mjpeg(data: &[u8], out: &mut Vec<u8>) -> Result<(u32, u32), String> {
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut dec = JpegDecoder::new_with_options(ZCursor::new(data), opts);
    dec.decode_headers().map_err(|e| format!("JPEG invalide : {e:?}"))?;
    let info = dec.info().ok_or("JPEG sans en-tête")?;
    let size = dec.output_buffer_size().ok_or("JPEG sans en-tête")?;
    let (w, h) = (info.width as usize, info.height as usize);
    if size != w * h * 4 {
        return Err("JPEG : sortie RGBA inattendue".into());
    }
    out.clear();
    out.resize(size, 0);
    dec.decode_into(out).map_err(|e| format!("JPEG invalide : {e:?}"))?;
    Ok((w as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yuyv_gris_et_couleurs() {
        // Deux pixels : Y=235 (blanc), Y=16 (noir), chroma neutre.
        let data = [235u8, 128, 16, 128];
        let mut out = Vec::new();
        decode(PixelFormat::Yuyv, 2, 1, 0, &data, &mut out).unwrap();
        assert_eq!(out, [255, 255, 255, 255, 0, 0, 0, 255]);
        // Rouge pur BT.601 : Y=81, U=90, V=240.
        let data = [81u8, 90, 81, 240];
        decode(PixelFormat::Yuyv, 2, 1, 0, &data, &mut out).unwrap();
        assert!(out[0] > 250 && out[1] < 5 && out[2] < 5, "{out:?}");
    }

    #[test]
    fn bgr_et_pas_de_lignes_decalees() {
        // 1×2 BGR avec un pas de ligne de 4 octets (un octet de bourrage).
        let data = [1u8, 2, 3, 0, 4, 5, 6];
        let mut out = Vec::new();
        decode(PixelFormat::Bgr, 1, 2, 4, &data, &mut out).unwrap();
        assert_eq!(out, [3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn image_tronquee_refusee() {
        let mut out = Vec::new();
        assert!(decode(PixelFormat::Yuyv, 4, 4, 0, &[0; 10], &mut out).is_err());
    }
}
