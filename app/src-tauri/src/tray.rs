use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::app_state::AppState;

/// Id fixo para localizar o ícone depois de criado.
pub const TRAY_ID: &str = "main";

/// Ícone na bandeja: alternar o modo bichinho (fallback se a hotkey falhar) e sair.
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        "Modo bichinho (Ctrl+Shift+P)",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &quit])?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Better Together")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => {
                if let Err(e) = app.state::<AppState>().toggle_pet() {
                    log::debug!("{e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Mostra ou esconde o ícone; sem ícone criado (ainda) não há o que fazer.
pub fn set_tray_visible(app: &AppHandle, visible: bool) {
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => {
            if let Err(e) = tray.set_visible(visible) {
                log::debug!("bandeja não ajustou a visibilidade: {e}");
            }
        }
        None => log::debug!("bandeja inexistente; visibilidade ignorada"),
    }
}
