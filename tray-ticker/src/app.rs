//! Main eframe UI: tiny host viewport + chart popup + tray integration.

use crate::autostart;
use crate::chart;
use crate::config::{self, Config};
use crate::data::TimeRange;
use crate::icon::{self, ICON_SIZE};
use crate::paths;
use crate::poller::{self, AppState, AppStatus, PollerCmd, PollerHandle};
use anyhow::Result;
use eframe::egui::{self, ViewportCommand};
use egui_plot::{CoordinatesFormatter, Corner, Plot};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

const MENU_CHANGE: &str = "change";
const MENU_AUTOSTART: &str = "start_win";

/// Popup chart dimensions in egui logical points.
const POPUP_W: f32 = 400.0;
const POPUP_H: f32 = 280.0;

pub struct TrayTickerApp {
    _singleton: single_instance::SingleInstance,
    state: Arc<Mutex<AppState>>,
    poller: Option<PollerHandle>,
    tray_icon: TrayIcon,
    check_autostart: CheckMenuItem,
    selected_range: TimeRange,
    last_fetch_sent: Option<(TimeRange, Instant)>,
    show_popup: bool,
    show_ticker_dialog: bool,
    ticker_buf: String,
    ticker_err: Option<String>,
    config: Config,
    last_icon_fingerprint: u64,
    popup_grace_frames: u32,
    menu_queue: Arc<Mutex<Vec<String>>>,
    menu_change_id: String,
    menu_autostart_id: String,
}

impl TrayTickerApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        singleton: single_instance::SingleInstance,
        state: Arc<Mutex<AppState>>,
        poller: PollerHandle,
        config: Config,
        tray_icon: TrayIcon,
        check_autostart: CheckMenuItem,
        menu_queue: Arc<Mutex<Vec<String>>>,
        menu_change_id: String,
        menu_autostart_id: String,
    ) -> Self {
        let selected_range = TimeRange::from_label(&config.last_range).unwrap_or(TimeRange::D1);
        Self {
            _singleton: singleton,
            state,
            poller: Some(poller),
            tray_icon,
            check_autostart,
            selected_range,
            last_fetch_sent: None,
            show_popup: false,
            show_ticker_dialog: false,
            ticker_buf: config.symbol.clone(),
            ticker_err: None,
            config,
            last_icon_fingerprint: 0,
            popup_grace_frames: 0,
            menu_queue,
            menu_change_id,
            menu_autostart_id,
        }
    }

    // ---- tray icon ----

    fn icon_fingerprint(st: &AppState) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        st.symbol.hash(&mut h);
        format!("{:?}", st.status).hash(&mut h);
        if let Some(d) = &st.last_intraday {
            (d.price as i64).hash(&mut h);
        }
        h.finish()
    }

    fn update_tray_icon(&mut self) {
        let st = self.state.lock().unwrap();
        let fp = Self::icon_fingerprint(&st);
        if fp == self.last_icon_fingerprint {
            return;
        }
        self.last_icon_fingerprint = fp;
        let price = st.last_intraday.as_ref().map(|d| d.price);
        let prev  = st.last_intraday.as_ref().map(|d| d.previous_close);
        let rgba  = icon::render_tray_rgba(&st.status, price, prev);
        let tip   = build_tooltip(&st);
        drop(st);
        if let Ok(ic) = Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE) {
            let _ = self.tray_icon.set_icon(Some(ic));
        }
        let _ = self.tray_icon.set_tooltip(Some(tip));
    }

    // ---- popup lifecycle ----

    fn open_popup(&mut self, ctx: &egui::Context) {
        log::info!(
            "open_popup ENTER: was show_popup={} show_ticker_dialog={}",
            self.show_popup, self.show_ticker_dialog
        );
        self.show_popup = true;
        self.popup_grace_frames = 4;
        // Order matters on Windows: make the host window visible before
        // resizing/positioning. If the host was effectively 1×1 hidden, sending
        // InnerSize first can be a no-op until visibility is applied.
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(POPUP_W, POPUP_H)));
        self.position_popup(ctx);
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        self.ensure_range_cached();
        ctx.request_repaint();
        log::info!("open_popup EXIT: show_popup=true grace={}", self.popup_grace_frames);
    }

    fn hide_popup(&mut self, ctx: &egui::Context) {
        // Defensive: never hide the host while the change-ticker dialog is
        // active — if we shrink to 1×1 here, the dialog has nothing to render
        // into and the user sees "nothing happened".
        if self.show_ticker_dialog {
            log::warn!("hide_popup BLOCKED: show_ticker_dialog is true");
            return;
        }
        log::info!("hide_popup: was show_popup={}", self.show_popup);
        self.show_popup = false;
        // Shrink back to 1×1 but keep the window VISIBLE so eframe keeps calling
        // update() — if we set Visible(false) the repaint timer stops and the
        // next tray click never gets processed.
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(1.0, 1.0)));
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
    }

    fn toggle_popup(&mut self, ctx: &egui::Context) {
        if self.show_popup {
            self.hide_popup(ctx);
        } else {
            self.open_popup(ctx);
        }
    }

    fn position_popup(&self, ctx: &egui::Context) {
        let pp = ctx.native_pixels_per_point().unwrap_or(1.0) as f64;
        let w = POPUP_W as f64;
        let h = POPUP_H as f64;

        if let Some(rect) = self.tray_icon.rect() {
            let cx  = rect.position.x + rect.size.width  as f64 * 0.5;
            let top = rect.position.y;
            let x   = cx / pp - w * 0.5;
            let y   = top / pp - h - 8.0;
            log::debug!("popup pos ({x:.0},{y:.0}) tray=({},{} {}x{})",
                rect.position.x, rect.position.y, rect.size.width, rect.size.height);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(
                x.max(4.0) as f32,
                y.max(4.0) as f32,
            )));
        }
    }

    // ---- range fetch ----

    fn ensure_range_cached(&mut self) {
        let (sym, has) = {
            let st = self.state.lock().unwrap();
            (st.symbol.clone(), st.range_cache.contains_key(&self.selected_range))
        };
        if !has {
            self.maybe_send_fetch(self.selected_range, &sym);
        }
    }

    fn maybe_send_fetch(&mut self, range: TimeRange, _sym: &str) {
        let now = Instant::now();
        if let Some((r, t)) = self.last_fetch_sent {
            if r == range && now.duration_since(t) < Duration::from_millis(500) {
                return;
            }
        }
        self.last_fetch_sent = Some((range, now));
        if let Some(p) = &self.poller {
            let _ = p.cmd_tx.send(PollerCmd::FetchRange(range));
        }
    }

    // ---- event draining ----

    fn drain_tray_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            match ev {
                // Click fires on both Down AND Up – only toggle on Up (release)
                // so we don't open and immediately close the popup.
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    // Keep the popup/dialog stable while the change-ticker dialog is
                    // active. Some platforms emit extra tray click events around
                    // context-menu interactions.
                    if self.show_ticker_dialog {
                        log::debug!("tray left click ignored while ticker dialog is open");
                        continue;
                    }
                    log::info!("tray left click (up) – toggling popup");
                    self.toggle_popup(ctx);
                }
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    log::info!("tray double click – ensure open");
                    if !self.show_popup {
                        self.open_popup(ctx);
                    }
                }
                _ => {}
            }
        }
    }

    fn drain_menu_queue(&mut self, ctx: &egui::Context) {
        let ids: Vec<String> = {
            let mut q = self.menu_queue.lock().unwrap();
            if !q.is_empty() {
                log::info!("drain_menu_queue ENTER: queue has {} id(s)", q.len());
            }
            q.drain(..).collect()
        };
        for id in ids {
            let is_change = id == self.menu_change_id || id == MENU_CHANGE;
            let is_autostart = id == self.menu_autostart_id || id == MENU_AUTOSTART;
            log::info!(
                "drain_menu_queue: processing id='{id}' (change_id='{}' autostart_id='{}' is_change={} is_autostart={})",
                self.menu_change_id, self.menu_autostart_id, is_change, is_autostart
            );
            if is_change {
                self.ticker_buf = self.state.lock().unwrap().symbol.clone();
                self.ticker_err = None;
                self.show_ticker_dialog = true;
                log::info!("drain_menu_queue: show_ticker_dialog flipped to TRUE");
                // Always reopen/refocus from menu action so the dialog is visible.
                self.open_popup(ctx);
                // Belt-and-suspenders: ensure another paint occurs so the
                // dialog renders even if the prior wake-up was coalesced.
                ctx.request_repaint();
                log::info!(
                    "drain_menu_queue: post-open show_popup={} show_ticker_dialog={}",
                    self.show_popup, self.show_ticker_dialog
                );
            } else if is_autostart {
                log::info!("drain_menu_queue: toggling autostart");
                if let Ok(exe) = std::env::current_exe() {
                    let on = !autostart::is_enabled();
                    if autostart::set_enabled(on, &exe).is_ok() {
                        self.check_autostart.set_checked(on);
                        self.config.autostart = on;
                        let _ = config::save(&self.config);
                    }
                }
            } else {
                log::warn!("drain_menu_queue: unrecognized menu id='{id}'");
            }
        }
        let _ = ctx;
    }

    fn poll_show_request(&mut self, ctx: &egui::Context) {
        if let Ok(p) = paths::show_request_path() {
            if p.exists() {
                let _ = std::fs::remove_file(&p);
                self.open_popup(ctx);
            }
        }
    }

    // ---- UI ----

    fn ui_main(&mut self, ctx: &egui::Context) {
        if self.show_popup {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (err_banner, symbol, status, intraday, cache) = {
                    let st = self.state.lock().unwrap();
                    (
                        st.last_error.clone(),
                        st.symbol.clone(),
                        st.status.clone(),
                        st.last_intraday.clone(),
                        st.range_cache.clone(),
                    )
                };

                if let Some(e) = err_banner {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(200, 140, 0), format!("⚠ {e}"));
                        if ui.button("Retry").clicked() {
                            let sym = self.state.lock().unwrap().symbol.clone();
                            if let Some(p) = &self.poller {
                                let _ = p.cmd_tx.send(PollerCmd::SetSymbol(sym));
                            }
                        }
                    });
                    ui.separator();
                }

                ui.horizontal(|ui| {
                    ui.heading(&symbol);
                    ui.separator();
                    if let Some(d) = &intraday {
                        ui.label(format!("${:.2}", d.price));
                        let pct = pct_change(d.price, d.previous_close);
                        let col = if pct >= 0.0 {
                            egui::Color32::from_rgb(50, 200, 80)
                        } else {
                            egui::Color32::from_rgb(240, 70, 70)
                        };
                        ui.colored_label(col, format!("{:+.2}%", pct));
                        if let Some(ts) = latest_price_update_ts(d) {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!("Updated {}", fmt_ts(ts)))
                                    .small()
                                    .color(egui::Color32::from_gray(150)),
                            );
                        }
                    } else {
                        ui.label(match &status {
                            AppStatus::Loading => "loading…",
                            AppStatus::Error(_) => "error",
                            _ => "—",
                        });
                    }
                    if ui.button("↻").clicked() {
                        let sym = self.state.lock().unwrap().symbol.clone();
                        if let Some(p) = &self.poller {
                            let _ = p.cmd_tx.send(PollerCmd::SetSymbol(sym));
                        }
                    }
                });

                if let Some(name) = intraday.as_ref().and_then(|d| d.long_name.clone()) {
                    ui.label(
                        egui::RichText::new(name)
                            .small()
                            .color(egui::Color32::from_gray(170)),
                    );
                }

                ui.separator();
                ui.horizontal(|ui| {
                    for r in TimeRange::ALL {
                        let selected = r == self.selected_range;
                        if ui.selectable_label(selected, r.label()).clicked() {
                            self.selected_range = r;
                            self.config.last_range = r.label().into();
                            let _ = config::save(&self.config);
                            let sym = self.state.lock().unwrap().symbol.clone();
                            self.maybe_send_fetch(r, &sym);
                        }
                    }
                });

                let data = cache.get(&self.selected_range).or(intraday.as_ref()).cloned();

                if let Some(d) = data {
                    // Snap hover readout to the nearest bar on the series so price is always
                    // an actual close, not the free Y under the crosshair.
                    let times = d.times.clone();
                    let closes = d.closes.clone();
                    Plot::new("price")
                        .height(ui.available_height().max(100.0))
                        .x_axis_formatter(|mark, _| fmt_ts(mark.value as i64))
                        // Default hover text follows the nearest point on the line and clips at
                        // the plot edge. Show the same info in a fixed corner instead.
                        .label_formatter(|_name, _value| String::new())
                        .coordinates_formatter(
                            Corner::LeftTop,
                            CoordinatesFormatter::new(move |value, _bounds| {
                                match nearest_series_point(&times, &closes, value.x) {
                                    Some((ts, price)) => {
                                        format!("{}\n${:.2}", fmt_ts(ts), price)
                                    }
                                    None => format!("{}\n${:.2}", fmt_ts(value.x as i64), value.y),
                                }
                            }),
                        )
                        .show(ui, |plot_ui| {
                            for marker in chart::boundary_vlines(&d, self.selected_range) {
                                plot_ui.vline(marker);
                            }
                            plot_ui.line(chart::price_line(&d));
                        });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.spinner();
                    });
                }
            });
        }

        if self.show_ticker_dialog {
            let mut open = true;
            egui::Window::new("Change ticker")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Enter a symbol:");
                    let resp = ui.text_edit_singleline(&mut self.ticker_buf);
                    if let Some(e) = &self.ticker_err {
                        ui.colored_label(egui::Color32::RED, e);
                    }
                    let enter = resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.show_ticker_dialog = false;
                        }
                        if ui.button("OK").clicked() || enter {
                            let q = self.ticker_buf.trim().to_uppercase();
                            match crate::data::search_symbol(&q) {
                                Ok(sym) => {
                                    self.config.symbol = sym.clone();
                                    self.ticker_err = None;
                                    if let Some(p) = &self.poller {
                                        let _ = p.cmd_tx.send(PollerCmd::SetSymbol(sym));
                                    }
                                    let _ = config::save(&self.config);
                                    self.show_ticker_dialog = false;
                                }
                                Err(e) => {
                                    self.ticker_err = Some(format!("{e} — try the exact ticker"));
                                }
                            }
                        }
                    });
                });
            if !open {
                self.show_ticker_dialog = false;
            }
        }
    }
}

fn fmt_ts(ts: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| ts.to_string())
}

/// Nearest `(timestamp, close)` to plot-x `x` (Unix seconds as f64).
fn nearest_series_point(times: &[i64], closes: &[f64], x: f64) -> Option<(i64, f64)> {
    if times.is_empty() || times.len() != closes.len() {
        return None;
    }
    let n = times.len();
    let idx = times.partition_point(|&t| (t as f64) < x);
    let i0 = idx.saturating_sub(1);
    let i1 = idx.min(n - 1);
    let pick = |i: usize| {
        let p = closes[i];
        if p.is_finite() {
            Some((times[i], p))
        } else {
            None
        }
    };
    let a = pick(i0);
    let b = pick(i1);
    match (a, b) {
        (Some((ta, pa)), Some((tb, pb))) => {
            let da = (ta as f64 - x).abs();
            let db = (tb as f64 - x).abs();
            if da <= db {
                Some((ta, pa))
            } else {
                Some((tb, pb))
            }
        }
        (Some(tpa), None) => Some(tpa),
        (None, Some(tpb)) => Some(tpb),
        (None, None) => None,
    }
}

fn pct_change(price: f64, prev: f64) -> f64 {
    if !prev.is_finite() || prev.abs() < 1e-9 { return 0.0; }
    (price - prev) / prev * 100.0
}

fn latest_price_update_ts(d: &crate::data::ChartData) -> Option<i64> {
    d.times.last().copied()
}

fn build_tooltip(st: &AppState) -> String {
    let name_suffix = |d: &crate::data::ChartData| {
        d.long_name
            .as_deref()
            .map(|n| format!("\n{n}"))
            .unwrap_or_default()
    };
    let updated_suffix = |d: &crate::data::ChartData| {
        latest_price_update_ts(d)
            .map(|ts| format!("\nUpdated: {}", fmt_ts(ts)))
            .unwrap_or_default()
    };
    match &st.status {
        AppStatus::Loading => "Tray Ticker — loading…".into(),
        AppStatus::Error(e) => format!("Tray Ticker — error: {e}"),
        AppStatus::Stale { last_ok } => st.last_intraday.as_ref().map(|d| {
            format!("{} ${:.2} ({:+.2}%) stale since {}{}{}",
                d.symbol, d.price, pct_change(d.price, d.previous_close),
                last_ok.format("%H:%M"), name_suffix(d), updated_suffix(d))
        }).unwrap_or_else(|| "Tray Ticker — stale".into()),
        AppStatus::Ok { .. } => st.last_intraday.as_ref().map(|d| {
            format!("{} ${:.2} ({:+.2}%){}{}",
                d.symbol, d.price, pct_change(d.price, d.previous_close),
                name_suffix(d), updated_suffix(d))
        }).unwrap_or_else(|| "Tray Ticker".into()),
    }
}

impl eframe::App for TrayTickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        poller::refresh_stale(&self.state);
        self.poll_show_request(ctx);
        self.drain_tray_events(ctx);
        self.drain_menu_queue(ctx);
        self.check_autostart.set_checked(autostart::is_enabled());
        self.update_tray_icon();

        // Keyboard dismiss
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.show_ticker_dialog {
                self.show_ticker_dialog = false;
            } else if self.show_popup {
                self.hide_popup(ctx);
            }
        }

        // Focus-loss dismiss (skip grace period right after opening)
        if self.show_popup {
            if self.popup_grace_frames > 0 {
                self.popup_grace_frames -= 1;
            } else if let Some(false) = ctx.input(|i| i.viewport().focused) {
                if !self.show_ticker_dialog {
                    self.hide_popup(ctx);
                }
            }
        }

        self.ui_main(ctx);

        // Keep polling at a reasonable rate so tray events aren't missed.
        ctx.request_repaint_after(Duration::from_millis(150));
    }
}

impl Drop for TrayTickerApp {
    fn drop(&mut self) {
        if let Some(p) = self.poller.take() {
            p.shutdown();
        }
    }
}

// ---- tray setup helpers ----

/// Returns (menu, autostart check item, change ID, autostart ID, quit ID).
pub fn build_tray_menu(
    _config: &Config,
) -> Result<(Menu, CheckMenuItem, String, String, String)> {
    let change    = MenuItem::with_id(MENU_CHANGE, "Change ticker…", true, None);
    let autostart = CheckMenuItem::with_id(MENU_AUTOSTART, "Start with Windows", true, autostart::is_enabled(), None);
    let sep       = PredefinedMenuItem::separator();
    let quit      = MenuItem::new("Quit", true, None); // let tray-icon assign its own ID
    let change_id = change.id().as_ref().to_string();
    let autostart_id = autostart.id().as_ref().to_string();
    let quit_id   = quit.id().as_ref().to_string();
    let menu      = Menu::with_items(&[&change, &autostart, &sep, &quit])?;
    Ok((menu, autostart, change_id, autostart_id, quit_id))
}

pub fn create_tray_icon(menu: Menu) -> Result<TrayIcon> {
    let rgba  = icon::render_tray_rgba(&AppStatus::Loading, None, None);
    let icon  = Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE)?;
    let tray  = TrayIconBuilder::new()
        .with_menu_on_left_click(false)
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("Tray Ticker")
        .build()?;
    Ok(tray)
}
