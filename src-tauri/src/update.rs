//! Проверка новой версии на GitHub Releases.
//!
//! Раз в сутки (первый раз через 20 с после старта) `GET releases/latest`;
//! наружу уходит только `User-Agent: Restyle/<версия>` — ни ключей, ни текста.
//! Найденное обновление лежит в `UpdateState` (для окон, открытых позже) и
//! рассылается событием `update:available`; пункт в меню трея и строка в окне
//! настроек ведут на страницу релиза через `open_url` (github.com в белом
//! списке). Автоскачивания нет: MSI не подписан, пусть пользователь сам решает.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use ts_rs::TS;

use crate::settings::Settings;

pub const RELEASES_API: &str = "https://api.github.com/repos/Reva1v/Restyle/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/Reva1v/Restyle/releases/latest";
const FIRST_DELAY: Duration = Duration::from_secs(20);
/// Как часто просыпаться: настройку могли включить между проверками.
const TICK: Duration = Duration::from_secs(60 * 60);
const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(10);

/// Payload события `update:available` и ответ `update_status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/bindings/")]
pub struct UpdateInfo {
    /// Номер без `v`: «0.1.2».
    pub version: String,
    /// Страница релиза на github.com.
    pub url: String,
}

/// Последнее найденное обновление (окна, открытые после проверки, читают его
/// командой `update_status`).
#[derive(Default)]
pub struct UpdateState(pub Mutex<Option<UpdateInfo>>);

/// `v0.1.2`, `0.1.2-beta.1`, `1.2` → (0,1,2) / (0,1,2) / (1,2,0). Мусор — None.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core: String = s.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut parts = core.split('.').filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().ok());
    let a = parts.next()??;
    let b = parts.next().unwrap_or(Some(0))?;
    let c = parts.next().unwrap_or(Some(0))?;
    Some((a, b, c))
}

/// Тег новее текущей версии → его номер без `v`.
pub fn newer(current: &str, tag: &str) -> Option<String> {
    let cur = parse_version(current)?;
    let new = parse_version(tag)?;
    (new > cur).then(|| format!("{}.{}.{}", new.0, new.1, new.2))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    html_url: String,
}

/// Один запрос к GitHub: `Ok(None)` — обновлений нет.
pub async fn check_once(current: &str) -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(RELEASES_API)
        .header("User-Agent", format!("Restyle/{current}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub ответил {}", resp.status().as_u16()));
    }
    let rel: Release = resp.json().await.map_err(|e| e.without_url().to_string())?;
    Ok(newer(current, &rel.tag_name).map(|version| UpdateInfo {
        version,
        // Только github.com — иначе `open_url` откажет по белому списку.
        url: if rel.html_url.starts_with("https://github.com/") { rel.html_url } else { RELEASES_PAGE.into() },
    }))
}

/// Версия приложения; в debug-сборке `RESTYLE_FAKE_VERSION` подменяет её,
/// чтобы проверить путь «обновление найдено» без нового релиза.
fn current_version() -> String {
    let real = env!("CARGO_PKG_VERSION").to_string();
    if cfg!(debug_assertions) {
        std::env::var("RESTYLE_FAKE_VERSION").unwrap_or(real)
    } else {
        real
    }
}

/// Фоновая задача: раз в час смотрит настройку и раз в сутки спрашивает GitHub.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        let mut last: Option<Instant> = None;
        loop {
            let due = last.map_or(true, |t| t.elapsed() >= INTERVAL);
            if due && Settings::load(&app).check_updates {
                last = Some(Instant::now());
                match check_once(&current_version()).await {
                    Ok(Some(info)) => {
                        println!("[restyle] доступна версия {}", info.version);
                        if let Some(st) = app.try_state::<UpdateState>() {
                            *st.0.lock().unwrap() = Some(info.clone());
                        }
                        let _ = app.emit("update:available", &info);
                    }
                    Ok(None) => println!("[restyle] обновлений нет"),
                    Err(e) => eprintln!("[restyle] проверка обновлений: {e}"),
                }
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_and_garbage() {
        assert_eq!(parse_version("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("0.1.2-beta.1"), Some((0, 1, 2)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version(" V2.0.0 "), Some((2, 0, 0)));
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn newer_only_when_strictly_greater() {
        assert_eq!(newer("0.1.1", "v0.1.2").as_deref(), Some("0.1.2"));
        assert_eq!(newer("0.1.1", "v0.2.0").as_deref(), Some("0.2.0"));
        assert_eq!(newer("0.1.1", "v1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(newer("0.1.1", "v0.1.1"), None);
        assert_eq!(newer("0.1.1", "v0.1.0"), None);
        assert_eq!(newer("0.1.1", "v0.1.10").as_deref(), Some("0.1.10"), "числовое, не строковое сравнение");
        assert_eq!(newer("0.1.1", "nightly"), None);
    }
}
