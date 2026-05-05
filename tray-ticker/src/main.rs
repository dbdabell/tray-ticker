#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod chart;
mod config;
mod data;
mod icon;
mod instance_guard;
mod logging;
mod paths;
mod poller;

use app::{build_tray_menu, create_tray_icon, TrayTickerApp};
use instance_guard::acquire_or_signal_show;
use poller::{spawn_poller, AppState};
use std::sync::{Arc, Mutex};
use tray_icon::menu::MenuEvent;

fn main() -> eframe::Result<()> {
    let _ = logging::init();

    let singleton = match acquire_or_signal_show() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("single-instance: {e}");
            return Ok(());
        }
    };
    let Some(singleton) = singleton else {
        return Ok(());
    };

    let cfg = config::load();
    let state = Arc::new(Mutex::new(AppState::new(cfg.symbol.clone())));
    let poller = spawn_poller(Arc::clone(&state), cfg.symbol.clone());

    let (menu, check_autostart, change_id, autostart_id, quit_id) =
        build_tray_menu(&cfg).expect("tray menu");
    let tray = create_tray_icon(menu).expect("tray icon");

    // Shared queue for menu events. We use set_event_handler so events always
    // arrive even when the egui viewport is tiny/idle. Quit is handled
    // immediately in the callback; other actions are queued for update().
    let menu_queue: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mq = Arc::clone(&menu_queue);
    MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
        let id = ev.id().as_ref().to_string();
        log::info!("menu event: {id}");
        if id == quit_id {
            log::info!("Quit triggered – exiting");
            std::process::exit(0);
        }
        mq.lock().unwrap().push(id);
    }));

    // Small (1×1) always-visible host window that keeps the egui event loop
    // alive for tray interaction. The chart popup expands/collapses this.
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_visible(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size([1.0, 1.0])
            .with_title("Tray Ticker"),
        ..Default::default()
    };

    eframe::run_native(
        "Tray Ticker",
        native_options,
        Box::new(move |cc| {
            Ok(Box::new(TrayTickerApp::new(
                cc,
                singleton,
                state,
                poller,
                cfg.clone(),
                tray,
                check_autostart,
                menu_queue,
                change_id,
                autostart_id,
            )))
        }),
    )?;
    Ok(())
}
