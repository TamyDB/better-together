//! Tira a overlay do Host da captura de tela: o convidado vê os bichinhos desenhados
//! localmente, então eles não podem aparecer de novo no vídeo.

/// Marca a janela como invisível para captura (`WDA_EXCLUDEFROMCAPTURE`, Windows 10 2004+).
#[cfg(windows)]
pub fn exclude_hwnd(hwnd: windows::Win32::Foundation::HWND) -> windows::core::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };
    unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }
}

/// Aplica `exclude_hwnd` na janela. Devolve `false` se falhou: a sessão segue, mas os
/// bichinhos do Host vão aparecer duplicados no vídeo.
///
/// `true` não garante a exclusão: antes do Windows 10 2004 a chamada "funciona" como
/// `WDA_MONITOR` e a overlay vira um retângulo preto no vídeo, sem erro. O mínimo da spec é
/// o 2004+; checar a versão fica para o M6 (instalador).
#[cfg(windows)]
pub fn exclude_from_capture(window: &tauri::WebviewWindow) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;

    // O HWND vem do `raw-window-handle` para não depender da versão do `windows` do Tauri.
    let handle = match window.window_handle() {
        Ok(handle) => handle,
        Err(e) => {
            log::warn!("overlay sem handle de janela; ela vai aparecer no vídeo: {e}");
            return false;
        }
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        log::warn!("overlay sem HWND; ela vai aparecer no vídeo");
        return false;
    };
    match exclude_hwnd(HWND(h.hwnd.get() as _)) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("falha ao tirar a overlay da captura; ela vai aparecer no vídeo: {e}");
            false
        }
    }
}

/// Fora do Windows não há captura a proteger.
#[cfg(not(windows))]
pub fn exclude_from_capture(_window: &tauri::WebviewWindow) -> bool {
    true
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
        WINDOW_EX_STYLE, WINDOW_STYLE,
    };

    #[test]
    fn exclude_hwnd_sets_affinity() {
        unsafe {
            let instance = GetModuleHandleW(None).expect("módulo do processo");
            // Sem WS_VISIBLE: a janela fica oculta.
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("better-together-test"),
                WINDOW_STYLE::default(),
                0,
                0,
                100,
                100,
                None,
                None,
                instance,
                None,
            )
            .expect("cria a janela de teste");

            let applied = exclude_hwnd(hwnd);
            let mut affinity = 0u32;
            let read = GetWindowDisplayAffinity(hwnd, &mut affinity);
            let _ = DestroyWindow(hwnd);

            applied.expect("aplica a afinidade");
            read.expect("lê a afinidade");
            assert_eq!(affinity, WDA_EXCLUDEFROMCAPTURE.0);
        }
    }
}
