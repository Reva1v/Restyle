//! Захват экрана как контекст для модели: окно с фокусом (по умолчанию) или
//! монитор под курсором → ресайз до 1280 px по длинной стороне → JPEG q70 →
//! base64. Захват (`capture`) делается ДО показа оверлея, чтобы он не попал в
//! кадр; ресайз и кодирование (`encode`) — уже после, в blocking-таске.
//! Любая ошибка = работаем без скриншота, не падаем.
//!
//! Снимок — BitBlt экранной области (то, что видит пользователь, включая
//! перекрывающие окна), а не PrintWindow: xcap 0.9 на PrintWindow давал
//! ~110 мс на окно 2560×1400, BitBlt + GetDIBits укладываются в единицы мс.
//! Сырой кадр хранится как BGRA без попиксельной конвертации — она делается
//! уже на уменьшенной картинке в `encode`.

use std::io::Cursor;
use std::time::{Duration, Instant};

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{imageops, DynamicImage, RgbaImage};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    GetDIBits, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsIconic};

/// Длинная сторона после ресайза.
pub const MAX_SIDE: u32 = 1280;
pub const JPEG_QUALITY: u8 = 70;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Window,
    Monitor,
}

impl Mode {
    pub fn from_setting(s: &str) -> Self {
        if s.eq_ignore_ascii_case("monitor") {
            Mode::Monitor
        } else {
            Mode::Window
        }
    }
}

/// Сырой кадр до кодирования: top-down BGRA, 4 байта на пиксель.
pub struct RawShot {
    bgra: Vec<u8>,
    width: u32,
    height: u32,
    pub capture: Duration,
}

#[derive(Clone, Debug)]
pub struct Encoded {
    /// Уходит в API (фаза 4); во фронтенд не передаётся.
    #[allow(dead_code)]
    pub jpeg_base64: String,
    pub bytes: usize,
    pub width: u32,
    pub height: u32,
    pub capture_ms: f32,
    pub encode_ms: f32,
}

fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

/// Видимые границы окна (без невидимой DWM-рамки), обрезанные по монитору.
fn window_rect(h: HWND) -> Result<RECT, String> {
    unsafe {
        if IsIconic(h).as_bool() {
            return Err("окно свёрнуто".into());
        }
        let mut r = RECT::default();
        let dwm = DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut core::ffi::c_void,
            size_of::<RECT>() as u32,
        );
        if dwm.is_err() {
            GetWindowRect(h, &mut r).map_err(|e| format!("GetWindowRect: {e}"))?;
        }
        let mon = monitor_rect(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST))?;
        Ok(intersect(r, mon))
    }
}

fn monitor_rect(hmon: windows::Win32::Graphics::Gdi::HMONITOR) -> Result<RECT, String> {
    unsafe {
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return Err("GetMonitorInfoW".into());
        }
        Ok(mi.rcMonitor)
    }
}

fn intersect(a: RECT, b: RECT) -> RECT {
    RECT {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    }
}

/// BitBlt области экрана (физические пиксели) → BGRA top-down.
fn grab(r: RECT) -> Result<(Vec<u8>, u32, u32), String> {
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    if w < 8 || h < 8 {
        return Err(format!("область {w}x{h} слишком мала"));
    }
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("GetDC".into());
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        // CAPTUREBLT — включая слоистые окна (WS_EX_LAYERED), иначе они выпадут из кадра.
        let blt = BitBlt(mem, 0, 0, w, h, screen, r.left, r.top, SRCCOPY | CAPTUREBLT);
        let mut out = Vec::new();
        let res = if let Err(e) = blt {
            Err(format!("BitBlt: {e}"))
        } else {
            let mut bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            out = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(
                mem,
                bmp,
                0,
                h as u32,
                Some(out.as_mut_ptr() as *mut core::ffi::c_void),
                &mut bi,
                DIB_RGB_COLORS,
            );
            if lines == 0 {
                Err("GetDIBits".into())
            } else {
                Ok(())
            }
        };
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        res.map(|_| (out, w as u32, h as u32))
    }
}

/// Снимок окна `target_hwnd` (Mode::Window) или монитора под `cursor`.
pub fn capture(mode: Mode, target_hwnd: isize, cursor: (i32, i32)) -> Result<RawShot, String> {
    let t0 = Instant::now();
    let rect = match mode {
        Mode::Window => window_rect(hwnd(target_hwnd))?,
        Mode::Monitor => unsafe {
            monitor_rect(MonitorFromPoint(POINT { x: cursor.0, y: cursor.1 }, MONITOR_DEFAULTTONEAREST))?
        },
    };
    let (bgra, width, height) = grab(rect)?;
    Ok(RawShot { bgra, width, height, capture: t0.elapsed() })
}

/// Целевой размер: длинная сторона ≤ MAX_SIDE, пропорции сохраняются.
pub fn fit(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= max_side {
        return (width, height);
    }
    let k = max_side as f64 / long as f64;
    (
        ((width as f64 * k).round() as u32).max(1),
        ((height as f64 * k).round() as u32).max(1),
    )
}

/// Ресайз + BGRA→RGB + JPEG + base64. Отдельный шаг — выполнять в blocking-таске.
pub fn encode(raw: RawShot) -> Result<Encoded, String> {
    let t0 = Instant::now();
    let (w, h) = fit(raw.width, raw.height, MAX_SIDE);
    // Байты BGRA притворяются RGBA: ресайз каналам безразличен, а меняем
    // местами R/B уже на маленькой картинке.
    let full = RgbaImage::from_raw(raw.width, raw.height, raw.bgra)
        .ok_or_else(|| "размер буфера не совпадает".to_string())?;
    let mut small = if (w, h) == (raw.width, raw.height) {
        full
    } else {
        imageops::thumbnail(&full, w, h) // box-фильтр: быстрый и достаточный для контекста
    };
    let t_resize = t0.elapsed();
    for px in small.pixels_mut() {
        px.0.swap(0, 2);
    }
    let rgb = DynamicImage::ImageRgba8(small).into_rgb8();
    let t_swap = t0.elapsed();
    let mut buf = Vec::with_capacity(256 * 1024);
    JpegEncoder::new_with_quality(Cursor::new(&mut buf), JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| format!("jpeg: {e}"))?;
    let t_jpeg = t0.elapsed();
    eprintln!(
        "[restyle] encode: resize {:.1} ms, bgra->rgb {:.1} ms, jpeg {:.1} ms",
        t_resize.as_secs_f64() * 1000.0,
        (t_swap - t_resize).as_secs_f64() * 1000.0,
        (t_jpeg - t_swap).as_secs_f64() * 1000.0
    );
    if let Some(path) = std::env::var_os("RESTYLE_DUMP_SCREENSHOT") {
        if let Err(e) = std::fs::write(&path, &buf) {
            eprintln!("[restyle] dump screenshot: {e}");
        }
    }
    let bytes = buf.len();
    let jpeg_base64 = base64::engine::general_purpose::STANDARD.encode(&buf);
    Ok(Encoded {
        jpeg_base64,
        bytes,
        width: w,
        height: h,
        capture_ms: raw.capture.as_secs_f32() * 1000.0,
        encode_ms: t0.elapsed().as_secs_f32() * 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_small_images_and_scales_long_side() {
        assert_eq!(fit(800, 600, 1280), (800, 600));
        assert_eq!(fit(2560, 1440, 1280), (1280, 720));
        assert_eq!(fit(1080, 2400, 1280), (576, 1280));
        assert_eq!(fit(5000, 1, 1280), (1280, 1));
    }

    #[test]
    fn intersect_clips_to_monitor() {
        let win = RECT { left: -100, top: 50, right: 3000, bottom: 900 };
        let mon = RECT { left: 0, top: 0, right: 2560, bottom: 1440 };
        let r = intersect(win, mon);
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 50, 2560, 900));
    }

    #[test]
    fn encode_swaps_channels_and_produces_jpeg_within_bounds() {
        // BGRA: синий канал = 200, красный = 10
        let (w, h) = (2000u32, 1000u32);
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            bgra.extend_from_slice(&[200, 50, 10, 255]);
        }
        let raw = RawShot { bgra, width: w, height: h, capture: Duration::from_millis(1) };
        let e = encode(raw).unwrap();
        assert_eq!((e.width, e.height), (1280, 640));
        assert!(e.bytes > 500 && e.bytes < 400_000, "{}", e.bytes);
        let decoded = base64::engine::general_purpose::STANDARD.decode(&e.jpeg_base64).unwrap();
        assert_eq!(&decoded[..2], &[0xFF, 0xD8], "JPEG SOI");
        let img = image::load_from_memory(&decoded).unwrap().into_rgb8();
        let p = img.get_pixel(100, 100).0;
        assert!(p[0] < 40 && p[2] > 160, "ожидался RGB≈(10,50,200), получили {p:?}");
    }
}
