mod runtime;

use std::time::{SystemTime, UNIX_EPOCH};

use tauri::Manager;

use runtime::sim_loop::spawn_sim_loop;

const FALLBACK_ASPECT: f32 = 16.0 / 9.0;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let aspect = app
                .primary_monitor()?
                .map(|m| m.size().width as f32 / m.size().height as f32)
                .filter(|a| a.is_finite() && *a > 0.0)
                .unwrap_or(FALLBACK_ASPECT);
            // Fora da sim, o relógio é uma fonte aceitável de seed.
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            let sim = spawn_sim_loop(app.handle().clone(), aspect, seed);
            app.manage(sim);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o app");
}
