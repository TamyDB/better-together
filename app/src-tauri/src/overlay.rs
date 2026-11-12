use std::thread;
use std::time::Duration;

use tauri::{
    AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindowBuilder,
};

use crate::capture;

pub const OVERLAY_LABEL: &str = "overlay";
pub const MAIN_LABEL: &str = "main";
const MAIN_URL: &str = "index.html?view=main";
const OVERLAY_URL: &str = "index.html?view=overlay";
/// Janela `main` como launcher.
pub const LAUNCHER_SIZE: (f64, f64) = (420.0, 560.0);
/// Janela `main` como viewer do convidado.
pub const VIEWER_SIZE: (f64, f64) = (1280.0, 720.0);
/// Janela `main` como painel do Host.
pub const PANEL_SIZE: (f64, f64) = (360.0, 340.0);
const OVERLAY_GONE_POLLS: u32 = 25;
const OVERLAY_GONE_POLL: Duration = Duration::from_millis(20);
const DEV_SIZE: (f64, f64) = (1280.0, 720.0);
const FALLBACK_ASPECT: f32 = 16.0 / 9.0;

/// Cria a janela `main` do launcher.
pub fn setup_main(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App(MAIN_URL.into()))
        .title("Better Together")
        .inner_size(LAUNCHER_SIZE.0, LAUNCHER_SIZE.1)
        .build()?;
    Ok(())
}

/// Redimensiona a janela `main` (launcher ou painel do Host).
pub fn resize_main(app: &AppHandle, (width, height): (f64, f64)) {
    let Some(main) = app.get_webview_window(MAIN_LABEL) else {
        log::warn!("janela main não encontrada");
        return;
    };
    if let Err(e) = main.set_size(LogicalSize::new(width, height)) {
        log::warn!("falha ao redimensionar a janela main: {e}");
    }
}

/// Fecha a overlay, se existir. O fechamento é assíncrono: a janela sai logo em seguida.
pub fn close_overlay(app: &AppHandle) {
    if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
        if let Err(e) = overlay.destroy() {
            log::warn!("falha ao fechar a overlay: {e}");
        }
    }
}

/// Espera a overlay da sessão anterior sumir, para a nova não colidir com o rótulo dela.
/// Bloqueia por no máximo ~500 ms; só chamar fora da thread principal.
pub fn wait_overlay_gone(app: &AppHandle) {
    for _ in 0..OVERLAY_GONE_POLLS {
        if app.get_webview_window(OVERLAY_LABEL).is_none() {
            return;
        }
        thread::sleep(OVERLAY_GONE_POLL);
    }
}

/// O que a sessão do Host precisa saber da overlay criada.
pub struct OverlayInfo {
    /// Proporção (largura/altura) da área coberta.
    pub aspect: f32,
    /// A overlay ficou fora da captura de tela (os bichinhos não vão no vídeo).
    pub excluded: bool,
}

/// Cria a janela da overlay e devolve a proporção (largura/altura) da área coberta,
/// que a simulação usa para manter o movimento isotrópico, e se ela ficou fora da captura.
///
/// Normal: transparente, sem borda, sempre no topo, click-through, sobre o monitor primário.
/// Dev (`DM_DEV_WINDOW=1`): janela comum 1280×720 focável, para testar no Linux.
/// Nos dois casos a janela nasce oculta e só aparece depois de sair da captura.
pub fn setup_overlay(app: &AppHandle, dev_window: bool) -> tauri::Result<OverlayInfo> {
    let builder =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App(OVERLAY_URL.into()))
            .title("Better Together")
            .visible(false);

    if dev_window {
        let window = builder
            .inner_size(DEV_SIZE.0, DEV_SIZE.1)
            .resizable(false)
            .build()?;
        let excluded = capture::exclude_from_capture(&window);
        window.show()?;
        return Ok(OverlayInfo {
            aspect: (DEV_SIZE.0 / DEV_SIZE.1) as f32,
            excluded,
        });
    }

    let window = builder
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .build()?;
    let excluded = capture::exclude_from_capture(&window);

    let Some(monitor) = app.primary_monitor()? else {
        log::warn!("monitor primário não encontrado; usando proporção padrão");
        window.show()?;
        return Ok(OverlayInfo {
            aspect: FALLBACK_ASPECT,
            excluded,
        });
    };
    let (pos, size) = (*monitor.position(), *monitor.size());
    window.set_position(PhysicalPosition::new(pos.x, pos.y))?;
    window.set_size(PhysicalSize::new(size.width, size.height))?;
    window.set_ignore_cursor_events(true)?;
    window.show()?;

    let aspect = size.width as f32 / size.height as f32;
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        FALLBACK_ASPECT
    };
    Ok(OverlayInfo { aspect, excluded })
}

pub fn dev_window_enabled() -> bool {
    std::env::var("DM_DEV_WINDOW").is_ok_and(|v| v == "1")
}
