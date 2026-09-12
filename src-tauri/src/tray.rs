//! Иконка в трее. Меню — своё окно (`menu.html`), а не системное.
//!
//! Системное меню Tauri v2 статично: подменю «Последние переписывания»
//! приходилось пересобирать целиком, и при пересборке пункты дублировались
//! (`remove_at` чистит его не всегда). Плюс выглядело оно чужеродно рядом с
//! остальным интерфейсом. Своё окно решает и то, и другое; фокус оно не
//! забирает — иначе «вернуть предыдущий текст» вставляло бы в само меню
//! (вставка сверяет HWND целевого окна).

use tauri::image::Image;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};

use crate::control::ControlMsg;
use crate::CtrlTx;

pub fn build(app: &tauri::App) -> tauri::Result<()> {
    TrayIconBuilder::with_id("restyle-tray")
        .tooltip("Restyle")
        .icon(tray_icon())
        .on_tray_icon_event(|tray, event| {
            // По отпусканию кнопки (как системное меню) и по любой из двух:
            // пользователю всё равно, левой он ткнул или правой.
            if std::env::var_os("RESTYLE_LOG_TRAY").is_some() {
                // Какие события шлёт Windows на левый и правый клик — видно
                // только так: системного меню нет, и «ничего не произошло»
                // от «событие не пришло» иначе не отличить.
                println!("[restyle] трей: {event:?}");
            }
            if let TrayIconEvent::Click { button, button_state, .. } = event {
                if button_state == MouseButtonState::Up
                    && matches!(button, MouseButton::Left | MouseButton::Right)
                {
                    send(tray.app_handle(), ControlMsg::TrayMenu);
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn send(app: &AppHandle, msg: ControlMsg) {
    if let Some(tx) = app.try_state::<CtrlTx>() {
        let _ = tx.0.send(msg);
    }
}

/// Иконка трея ровно того размера, который запрашивает Windows: `SM_CXSMICON`
/// даёт 16 px при 100 %, 20 при 125 %, 24 при 150 %. Даунскейл силами системы
/// из одной большой картинки мылит букву, поэтому каждый размер нарисован
/// отдельно (`scripts/make_icons.py`).
fn tray_icon() -> Image<'static> {
    const PNG_16: &[u8] = include_bytes!("../icons/tray-16.png");
    const PNG_20: &[u8] = include_bytes!("../icons/tray-20.png");
    const PNG_24: &[u8] = include_bytes!("../icons/tray-24.png");
    const PNG_32: &[u8] = include_bytes!("../icons/tray-32.png");

    let want = unsafe { GetSystemMetrics(SM_CXSMICON) };
    let bytes = match want {
        ..=17 => PNG_16,
        18..=21 => PNG_20,
        22..=27 => PNG_24,
        _ => PNG_32,
    };
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("иконка трея — валидный PNG")
        .to_rgba8();
    let (w, h) = (img.width(), img.height());
    Image::new_owned(img.into_raw(), w, h)
}
