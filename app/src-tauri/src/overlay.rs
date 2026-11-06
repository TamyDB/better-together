use tauri::{AppHandle, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

pub const OVERLAY_LABEL: &str = "overlay";
const DEV_SIZE: (f64, f64) = (1280.0, 720.0);
const FALLBACK_ASPECT: f32 = 16.0 / 9.0;

/// Cria a janela da overlay e devolve a proporção (largura/altura) da área coberta,
/// que a simulação usa para manter o movimento isotrópico.
///
/// Normal: transparente, sem borda, sempre no topo, click-through, sobre o monitor primário.
/// Dev (`DM_DEV_WINDOW=1`): janela comum 1280×720 focável, para testar no Linux.
pub fn setup_overlay(app: &AppHandle, dev_window: bool) -> tauri::Result<f32> {
    let builder = WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::default())
        .title("Better Together");

    if dev_window {
        builder
            .inner_size(DEV_SIZE.0, DEV_SIZE.1)
            .resizable(false)
            .build()?;
        return Ok((DEV_SIZE.0 / DEV_SIZE.1) as f32);
    }

    let window = builder
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;

    let Some(monitor) = app.primary_monitor()? else {
        log::warn!("monitor primário não encontrado; usando proporção padrão");
        window.show()?;
        return Ok(FALLBACK_ASPECT);
    };
    let (pos, size) = (*monitor.position(), *monitor.size());
    window.set_position(PhysicalPosition::new(pos.x, pos.y))?;
    window.set_size(PhysicalSize::new(size.width, size.height))?;
    window.set_ignore_cursor_events(true)?;
    window.show()?;

    let aspect = size.width as f32 / size.height as f32;
    Ok(if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        FALLBACK_ASPECT
    })
}

pub fn dev_window_enabled() -> bool {
    std::env::var("DM_DEV_WINDOW").is_ok_and(|v| v == "1")
}
