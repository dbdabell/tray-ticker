//! Background polling thread + shared `AppState`.

use crate::data::{self, ChartData, TimeRange};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum AppStatus {
    Loading,
    Ok {
        fetched_at: DateTime<Utc>,
    },
    Stale {
        last_ok: DateTime<Utc>,
    },
    Error(String),
}

#[derive(Clone, Debug)]
pub struct AppState {
    pub symbol: String,
    pub status: AppStatus,
    pub last_intraday: Option<ChartData>,
    pub range_cache: HashMap<TimeRange, ChartData>,
    pub last_error: Option<String>,
}

impl AppState {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
            status: AppStatus::Loading,
            last_intraday: None,
            range_cache: HashMap::new(),
            last_error: None,
        }
    }
}

#[derive(Debug)]
pub enum PollerCmd {
    SetSymbol(String),
    FetchRange(TimeRange),
    Shutdown,
}

pub struct PollerHandle {
    pub cmd_tx: Sender<PollerCmd>,
    join: Option<thread::JoinHandle<()>>,
}

impl PollerHandle {
    pub fn shutdown(mut self) {
        let _ = self.cmd_tx.send(PollerCmd::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn spawn_poller(state: Arc<Mutex<AppState>>, symbol: String) -> PollerHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<PollerCmd>();
    {
        let mut st = state.lock().unwrap();
        st.symbol = symbol;
    }

    let state_clone = Arc::clone(&state);
    let join = thread::spawn(move || run_loop(state_clone, cmd_rx));

    PollerHandle {
        cmd_tx,
        join: Some(join),
    }
}

fn run_loop(state: Arc<Mutex<AppState>>, cmd_rx: Receiver<PollerCmd>) {
    let sym0 = state.lock().unwrap().symbol.clone();
    tick_fetch(&state, &sym0);
    let mut next_tick = Instant::now() + Duration::from_secs(30);
    loop {
        let wait = next_tick
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(200));
        match cmd_rx.recv_timeout(wait) {
            Ok(PollerCmd::Shutdown) => break,
            Ok(PollerCmd::SetSymbol(sym)) => {
                {
                    let mut st = state.lock().unwrap();
                    st.symbol = sym.clone();
                    st.range_cache.clear();
                    st.last_intraday = None;
                    st.status = AppStatus::Loading;
                    st.last_error = None;
                }
                tick_fetch(&state, &sym);
                next_tick = Instant::now() + Duration::from_secs(30);
            }
            Ok(PollerCmd::FetchRange(r)) => {
                let sym = state.lock().unwrap().symbol.clone();
                match data::fetch_chart_with_retry(&sym, r) {
                    Ok(d) => {
                        let mut st = state.lock().unwrap();
                        st.range_cache.insert(r, d);
                        st.last_error = None;
                    }
                    Err(e) => {
                        let mut st = state.lock().unwrap();
                        st.last_error = Some(e.to_string());
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if Instant::now() >= next_tick {
            let sym = state.lock().unwrap().symbol.clone();
            tick_fetch(&state, &sym);
            next_tick = Instant::now() + Duration::from_secs(30);
        }
    }
}

fn tick_fetch(state: &Arc<Mutex<AppState>>, symbol: &str) {
    match data::fetch_chart_with_retry(symbol, TimeRange::D1) {
        Ok(d) => {
            let mut st = state.lock().unwrap();
            st.last_intraday = Some(d.clone());
            st.range_cache.insert(TimeRange::D1, d);
            st.last_error = None;
            let now = Utc::now();
            st.status = AppStatus::Ok { fetched_at: now };
        }
        Err(e) => {
            let mut st = state.lock().unwrap();
            st.last_error = Some(e.to_string());
            st.status = AppStatus::Error(e.to_string());
        }
    }
}

/// Call from UI each frame to flip Ok → Stale if data is old.
pub fn refresh_stale(state: &Arc<Mutex<AppState>>) {
    let mut st = state.lock().unwrap();
    let now = Utc::now();
    if let AppStatus::Ok { fetched_at } = &st.status {
        if now.signed_duration_since(*fetched_at).num_seconds() > 120 {
            st.status = AppStatus::Stale {
                last_ok: *fetched_at,
            };
        }
    }
}
