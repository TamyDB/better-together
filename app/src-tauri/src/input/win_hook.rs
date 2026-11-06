//! Hook de teclado low-level do Windows: lê WASD/Shift/Esc no modo bichinho
//! sem que essas teclas cheguem aos apps do Host.

use std::sync::{Arc, OnceLock};
use std::thread;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, HC_ACTION,
    HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};

use super::keys::{should_swallow, Key};
use crate::pet_mode::PetMode;

// O callback do hook é `extern "system"` e não recebe contexto; o estado vive aqui.
static MODE: OnceLock<Arc<PetMode>> = OnceLock::new();

/// Instala o hook numa thread dedicada com o loop de mensagens que ele exige.
pub fn install_keyboard_hook(mode: Arc<PetMode>) {
    if MODE.set(mode).is_err() {
        log::warn!("hook de teclado já instalado");
        return;
    }
    thread::Builder::new()
        .name("keyboard-hook".into())
        .spawn(|| unsafe {
            let module = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
            if let Err(e) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) {
                log::error!("falha ao instalar o hook de teclado: {e}");
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("falha ao criar a thread do hook");
}

/// Precisa ser rápido: o Windows remove hooks que demoram a responder.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(mode) = MODE.get() {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let key = Key::from_vk(kb.vkCode as u16);
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let modifier =
                GetAsyncKeyState(VK_CONTROL.0 as i32) < 0 || GetAsyncKeyState(VK_MENU.0 as i32) < 0;

            if let Some(k) = key {
                if should_swallow(mode.is_on(), key, modifier) {
                    mode.on_key(k, down);
                    return LRESULT(1);
                }
                // Soltar a tecla sempre atualiza o estado, para não "prender" o movimento
                // quando ela foi solta com Ctrl/Alt pressionado.
                if !down {
                    mode.on_key(k, false);
                }
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}
