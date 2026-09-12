//! UI Automation: чтение текста элемента с фокусом без клипборда.
//! Только с capture-потока (STA). Найденный элемент запоминается в thread-local
//! для последующей записи через `ValuePattern.SetValue` (фаза 5).

use std::cell::RefCell;

use windows::core::Interface;
use windows::Win32::Foundation::BOOL;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationValuePattern, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    UIA_TextPatternId, UIA_ValuePatternId,
};

thread_local! {
    /// Элемент последнего успешного чтения — цель для SetValue.
    static LAST_ELEMENT: RefCell<Option<IUIAutomationElement>> = const { RefCell::new(None) };
}

pub struct Client {
    auto: IUIAutomation,
}

#[derive(Debug)]
pub struct UiaText {
    pub text: String,
    /// `ValuePattern.SetValue` доступен (Edit/Document, не read-only).
    pub writable: bool,
    /// Взято текущее выделение, а не весь текст (режим «только выделение»).
    pub selection_only: bool,
}

#[derive(Debug)]
pub enum UiaError {
    /// Элемент с фокусом — поле пароля: не читаем ни UIA, ни клипбордом.
    Password,
    Com(String),
}

impl From<windows::core::Error> for UiaError {
    fn from(e: windows::core::Error) -> Self {
        UiaError::Com(e.to_string())
    }
}

impl Client {
    pub fn new() -> Result<Self, String> {
        let auto: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| e.to_string())?;
        Ok(Self { auto })
    }

    /// `Ok(None)` — элемент не текстовый или текста нет: идём в клипборд.
    pub fn read(&self, select_only: bool) -> Result<Option<UiaText>, UiaError> {
        unsafe {
            let el = self.auto.GetFocusedElement()?;
            if el.CurrentIsPassword().unwrap_or(BOOL(0)).as_bool() {
                return Err(UiaError::Password);
            }
            let ctype = el.CurrentControlType().unwrap_or_default();
            let is_text_control = ctype == UIA_EditControlTypeId || ctype == UIA_DocumentControlTypeId;

            let value_pat: Option<IUIAutomationValuePattern> = el
                .GetCurrentPattern(UIA_ValuePatternId)
                .ok()
                .and_then(|u| u.cast().ok());
            let text_pat: Option<IUIAutomationTextPattern> = el
                .GetCurrentPattern(UIA_TextPatternId)
                .ok()
                .and_then(|u| u.cast().ok());

            let writable = is_text_control
                && value_pat
                    .as_ref()
                    .map(|v| !v.CurrentIsReadOnly().unwrap_or(BOOL(1)).as_bool())
                    .unwrap_or(false);

            LAST_ELEMENT.with_borrow_mut(|l| *l = Some(el.clone()));

            if select_only {
                // Только выделение; без него — клипборд (Ctrl+C), чтобы не тащить документ.
                if let Some(tp) = &text_pat {
                    let sel = tp.GetSelection()?;
                    let n = sel.Length()?;
                    let mut s = String::new();
                    for i in 0..n {
                        s.push_str(&sel.GetElement(i)?.GetText(-1)?.to_string());
                    }
                    if !s.trim().is_empty() {
                        return Ok(Some(UiaText { text: s, writable: false, selection_only: true }));
                    }
                }
                return Ok(None);
            }

            if is_text_control {
                if let Some(vp) = &value_pat {
                    let v = vp.CurrentValue()?.to_string();
                    if !v.trim().is_empty() {
                        return Ok(Some(UiaText { text: v, writable, selection_only: false }));
                    }
                }
            }
            if let Some(tp) = &text_pat {
                let t = tp.DocumentRange()?.GetText(-1)?.to_string();
                if !t.trim().is_empty() {
                    return Ok(Some(UiaText { text: t, writable, selection_only: false }));
                }
            }
            eprintln!(
                "[restyle] UIA: у элемента с фокусом нет текста: type={} class={:?} name={:?} value={} text={}",
                ctype.0,
                el.CurrentClassName().map(|s| s.to_string()).unwrap_or_default(),
                el.CurrentName().map(|s| s.to_string()).unwrap_or_default(),
                value_pat.is_some(),
                text_pat.is_some()
            );
            Ok(None)
        }
    }
}

/// Записать текст в элемент последнего чтения (фаза 5). `Ok(false)` — элемент
/// не сохранён или не поддерживает ValuePattern.
#[allow(dead_code)]
pub fn write(text: &str) -> Result<bool, UiaError> {
    LAST_ELEMENT.with_borrow(|l| {
        let Some(el) = l else { return Ok(false) };
        unsafe {
            let Ok(u) = el.GetCurrentPattern(UIA_ValuePatternId) else { return Ok(false) };
            let vp: IUIAutomationValuePattern = match u.cast() {
                Ok(v) => v,
                Err(_) => return Ok(false),
            };
            if vp.CurrentIsReadOnly().unwrap_or(BOOL(1)).as_bool() {
                return Ok(false);
            }
            vp.SetValue(&windows::core::BSTR::from(text))?;
            Ok(true)
        }
    })
}
