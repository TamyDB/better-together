//! Hook de teclado low-level do Windows: lê WASD/Shift/Esc e os emotes `1`–`6` no modo
//! bichinho sem que essas teclas cheguem aos apps do Host.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    SetWindowsHookExW, TranslateMessage, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use super::keys::{filter_key, Key};
use crate::pet_mode::PetMode;

// O callback do hook é `extern "system"` e não recebe contexto; o estado vive aqui.
static MODE: OnceLock<Arc<PetMode>> = OnceLock::new();
// Teclas cujo key-down foi engolido (ver `filter_key`). Só a thread do hook escreve.
static SWALLOWED: AtomicU16 = AtomicU16::new(0);

/// Instala o hook numa thread dedicada com o loop de mensagens que ele exige.
pub fn install_keyboard_hook(mode: Arc<PetMode>) {
    if MODE.set(mode).is_err() {
        log::warn!("hook de teclado já instalado");
        return;
    }
    thread::Builder::new()
        .name("keyboard-hook".into())
        .spawn(|| unsafe {
            let module: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
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
            let pressed = |vk: VIRTUAL_KEY| GetAsyncKeyState(vk.0 as i32) < 0;
            let modifier =
                pressed(VK_CONTROL) || pressed(VK_MENU) || pressed(VK_LWIN) || pressed(VK_RWIN);

            // Numa janela do próprio app (ex.: o chat do painel) as teclas são do app: o
            // key-down não é engolido nem vira movimento/emote. Key-ups seguem as regras de
            // `filter_key` com o modo inativo: liberam o que foi engolido antes.
            let active = mode.is_on() && !own_window_foreground();
            let (state, decision) = filter_key(
                SWALLOWED.load(Ordering::Relaxed),
                active,
                key,
                down,
                modifier,
            );
            SWALLOWED.store(state, Ordering::Relaxed);
            if let (Some(k), true) = (key, decision.forward) {
                mode.on_key(k, down);
            }
            if decision.swallow {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// A janela em primeiro plano é do próprio app (painel, overlay, viewer)?
fn own_window_foreground() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}
