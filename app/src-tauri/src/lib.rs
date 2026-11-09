mod app_state;
mod input;
mod net_config;
mod overlay;
mod pet_mode;
mod runtime;
mod session;
mod tray;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use dm_protocol::Animal;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use app_state::{parse_recv, AppState, Mode};
use input::keys::Key;
use net_config::NetConfig;
use pet_mode::{PetMode, KEEP_ALIVE_MS};
use runtime::guest_loop::spawn_guest_loop;
use runtime::host_loop::spawn_host_loop;
use session::guest::GuestClient;
use session::host::HostSession;
use session::net::NetIn;

fn pet_mode_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyP)
}

/// Bandeja, hook de teclado e keep-alive: instalados na primeira sessão de Host e mantidos.
fn install_host_services(app: &AppHandle, pet_mode: Arc<PetMode>) {
    if let Err(e) = tray::setup_tray(app) {
        log::warn!("bandeja indisponível: {e}");
    }
    #[cfg(windows)]
    input::win_hook::install_keyboard_hook(pet_mode.clone());

    let keep_alive = thread::Builder::new()
        .name("pet-mode-keepalive".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_millis(KEEP_ALIVE_MS));
            pet_mode.keep_alive();
        });
    if let Err(e) = keep_alive {
        log::warn!("keep-alive do modo bichinho indisponível: {e}");
    }
}

#[tauri::command]
fn net_config() -> Result<NetConfig, String> {
    Ok(NetConfig::from_env())
}

// `async`: criar janela em comando síncrono trava no Windows (ver docs do WebviewWindowBuilder).
#[tauri::command(async)]
fn host_start(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    animal: Animal,
) -> Result<(), String> {
    let _starting = state.begin_start("host")?;

    overlay::wait_overlay_gone(&app);
    let aspect = overlay::setup_overlay(&app, overlay::dev_window_enabled()).map_err(|e| {
        overlay::close_overlay(&app);
        format!("falha ao criar a overlay: {e}")
    })?;

    let session = HostSession::new(aspect, rand::random(), &name, animal);
    let handle = Arc::new(spawn_host_loop(app.clone(), session));
    let pet_mode = state.bind_pet_sink(handle.clone());

    state.install_host_services_once(|pet_mode| install_host_services(&app, pet_mode));
    if let Err(e) = app.global_shortcut().register(pet_mode_shortcut()) {
        log::warn!("não foi possível registrar Ctrl+Shift+P ({e}); use o menu da bandeja");
    }
    overlay::resize_main(&app, overlay::PANEL_SIZE);

    state.set_mode(Mode::Host { handle, pet_mode });
    Ok(())
}

#[tauri::command]
fn guest_start(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let token = state.rejoin_token();
    state.enter_guest(|| spawn_guest_loop(app, GuestClient::new(token)))
}

#[tauri::command]
fn guest_choose(state: State<'_, AppState>, name: String, animal: Animal) -> Result<(), String> {
    state.guest_choose(name, animal)
}

#[tauri::command]
fn net_peer_opened(state: State<'_, AppState>, peer: u64) -> Result<(), String> {
    state.net(NetIn::PeerOpened(peer));
    Ok(())
}

#[tauri::command]
fn net_peer_closed(state: State<'_, AppState>, peer: u64) -> Result<(), String> {
    state.net(NetIn::PeerClosed(peer));
    Ok(())
}

#[tauri::command]
fn net_recv(
    state: State<'_, AppState>,
    peer: u64,
    channel: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    state.net(parse_recv(peer, &channel, bytes)?);
    Ok(())
}

/// Teclas vindas da webview: pet do Host ou input do convidado.
#[tauri::command]
fn key_event(state: State<'_, AppState>, code: String, down: bool) -> Result<(), String> {
    if let Some(key) = Key::from_code(&code) {
        state.key_event(key, down);
    }
    Ok(())
}

#[tauri::command]
fn toggle_pet_mode(state: State<'_, AppState>) -> Result<(), String> {
    state.toggle_pet()
}

// `async` pelo mesmo motivo do `host_start`: mexe em janelas.
#[tauri::command(async)]
fn session_reset(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let was_host = state.end_session()?;
    if was_host {
        if let Err(e) = app.global_shortcut().unregister(pet_mode_shortcut()) {
            log::debug!("atalho do modo bichinho não removido: {e}");
        }
        overlay::resize_main(&app, overlay::LAUNCHER_SIZE);
    }
    overlay::close_overlay(&app);
    Ok(())
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if *shortcut == pet_mode_shortcut() && event.state() == ShortcutState::Pressed {
                        if let Err(e) = app.state::<AppState>().toggle_pet() {
                            log::debug!("{e}");
                        }
                    }
                })
                .build(),
        )
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            net_config,
            host_start,
            guest_start,
            guest_choose,
            net_peer_opened,
            net_peer_closed,
            net_recv,
            key_event,
            toggle_pet_mode,
            session_reset,
        ])
        .setup(|app| {
            overlay::setup_main(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o app");
}
