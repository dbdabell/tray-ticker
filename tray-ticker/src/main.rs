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

    // Shared queue for menu events. We register the global MenuEvent handler
    // INSIDE the eframe creator below so the closure can capture a clone of
    // egui's Context — that way we can `request_repaint()` from the callback
    // and not rely on the 150ms idle repaint timer to drain the queue.
    let menu_queue: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    log::info!(
        "menu ids: change={} autostart={} quit={}",
        change_id, autostart_id, quit_id
    );

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

    let menu_queue_for_handler = Arc::clone(&menu_queue);
    let menu_queue_for_app = Arc::clone(&menu_queue);
    let quit_id_for_handler = quit_id;

    eframe::run_native(
        "Tray Ticker",
        native_options,
        Box::new(move |cc| {
            // Register the menu event handler *now* so we can capture a clone
            // of the live egui Context. Without this, queued menu events
            // (e.g. "Change ticker…") only get drained on the next idle
            // repaint, which can be delayed indefinitely on Windows after the
            // tray context menu closes.
            let ctx_for_menu = cc.egui_ctx.clone();
            let mq = menu_queue_for_handler;
            let qid = quit_id_for_handler;
            log::info!("registering MenuEvent::set_event_handler with egui ctx");
            MenuEvent::set_event_handler(Some(move |ev: MenuEvent| {
                let id = ev.id().as_ref().to_string();
                log::info!("menu callback fired: id='{id}'");
                if id == qid {
                    log::info!("Quit triggered – exiting");
                    std::process::exit(0);
                }
                let qlen = {
                    let mut q = mq.lock().unwrap();
                    q.push(id.clone());
                    q.len()
                };
                log::info!("menu callback queued id='{id}' (queue len={qlen}); waking egui");
                ctx_for_menu.request_repaint();
            }));

            Ok(Box::new(TrayTickerApp::new(
                cc,
                singleton,
                state,
                poller,
                cfg.clone(),
                tray,
                check_autostart,
                menu_queue_for_app,
                change_id,
                autostart_id,
            )))
        }),
    )?;
    Ok(())
}
