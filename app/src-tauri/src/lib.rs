mod input;
mod overlay;
mod pet_mode;
mod runtime;
mod tray;

use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use input::keys::Key;
use pet_mode::{PetMode, KEEP_ALIVE_MS};
use runtime::sim_loop::spawn_sim_loop;

fn pet_mode_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyP)
}

/// Teclas vindas da webview (modo dev). Fora do modo dev a overlay nunca tem foco.
#[tauri::command]
fn dev_key(code: String, down: bool, mode: State<'_, Arc<PetMode>>) {
    if let Some(key) = Key::from_code(&code) {
        mode.on_key(key, down);
    }
}

#[tauri::command]
fn toggle_pet_mode(mode: State<'_, Arc<PetMode>>) {
    mode.toggle();
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if *shortcut == pet_mode_shortcut() && event.state() == ShortcutState::Pressed {
                        app.state::<Arc<PetMode>>().toggle();
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![dev_key, toggle_pet_mode])
        .setup(|app| {
            let aspect = overlay::setup_overlay(app.handle(), overlay::dev_window_enabled())?;
            // Fora da sim, o relógio é uma fonte aceitável de seed.
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            let sim = Arc::new(spawn_sim_loop(app.handle().clone(), aspect, seed));
            let mode = Arc::new(PetMode::new(sim));
            app.manage(mode.clone());

            if let Err(e) = app.global_shortcut().register(pet_mode_shortcut()) {
                log::warn!("não foi possível registrar Ctrl+Shift+P ({e}); use o menu da bandeja");
            }
            if let Err(e) = tray::setup_tray(app.handle()) {
                log::warn!("bandeja indisponível: {e}");
            }
            #[cfg(windows)]
            input::win_hook::install_keyboard_hook(mode.clone());

            thread::Builder::new()
                .name("pet-mode-keepalive".into())
                .spawn(move || loop {
                    thread::sleep(Duration::from_millis(KEEP_ALIVE_MS));
                    mode.keep_alive();
                })?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o app");
}
