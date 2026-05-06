//! Yahoo Finance v8 chart + v1 search.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum TimeRange {
    D1,
    W1,
    M1,
    Y1,
}

impl TimeRange {
    pub const ALL: [TimeRange; 4] = [TimeRange::D1, TimeRange::W1, TimeRange::M1, TimeRange::Y1];

    pub fn label(self) -> &'static str {
        match self {
            TimeRange::D1 => "1D",
            TimeRange::W1 => "1W",
            TimeRange::M1 => "1M",
            TimeRange::Y1 => "1Y",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        match s {
            "1D" => Some(TimeRange::D1),
            "1W" => Some(TimeRange::W1),
            "1M" => Some(TimeRange::M1),
            "1Y" => Some(TimeRange::Y1),
            _ => None,
        }
    }

    /// Yahoo chart interval for this logical range.
    pub fn yahoo_interval(self) -> &'static str {
        match self {
            TimeRange::D1 => "5m",
            TimeRange::W1 => "15m",
            TimeRange::M1 => "1h",
            TimeRange::Y1 => "1d",
        }
    }

    /// Strict trailing-window duration in seconds.
    pub fn trailing_window_secs(self) -> i64 {
        match self {
            TimeRange::D1 => 24 * 60 * 60,
            TimeRange::W1 => 7 * 24 * 60 * 60,
            TimeRange::M1 => 30 * 24 * 60 * 60,
            TimeRange::Y1 => 365 * 24 * 60 * 60,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChartData {
    pub symbol: String,
    pub long_name: Option<String>,
    pub price: f64,
    pub previous_close: f64,
    pub times: Vec<i64>,
    pub closes: Vec<f64>,
}

#[derive(Deserialize)]
struct YahooChartRoot {
    chart: YahooChart,
}

#[derive(Deserialize)]
struct YahooChart {
    result: Option<Vec<YahooResult>>,
    error: Option<YahooError>,
}

#[derive(Deserialize)]
struct YahooError {
    description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YahooResult {
    meta: YahooMeta,
    timestamp: Option<Vec<i64>>,
    indicators: YahooIndicators,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct YahooMeta {
    symbol: Option<String>,
    regular_market_price: Option<f64>,
    previous_close: Option<f64>,
    chart_previous_close: Option<f64>,
    long_name: Option<String>,
    short_name: Option<String>,
}

#[derive(Deserialize)]
struct YahooIndicators {
    quote: Vec<YahooQuote>,
}

#[derive(Deserialize)]
struct YahooQuote {
    close: Vec<Option<f64>>,
}

pub fn fetch_chart(symbol: &str, range: TimeRange) -> Result<ChartData> {
    let sym = urlencoding::encode(symbol);
    let interval = range.yahoo_interval();
    let now = chrono::Utc::now().timestamp();
    let period2 = now.max(1);
    let period1 = (period2 - range.trailing_window_secs()).max(0);
    let url = format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{sym}?period1={period1}&period2={period2}&interval={interval}"
    );
    let body = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .context("http chart")?
        .into_string()
        .context("read body")?;
    parse_chart_json(&body, symbol)
}

pub fn fetch_chart_with_retry(symbol: &str, range: TimeRange) -> Result<ChartData> {
    let mut last = None;
    for attempt in 0..4 {
        if attempt > 0 {
            let ms = [250u64, 1000, 4000][attempt - 1];
            std::thread::sleep(Duration::from_millis(ms));
        }
        match fetch_chart(symbol, range) {
            Ok(v) => return Ok(v),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

fn parse_chart_json(body: &str, fallback_symbol: &str) -> Result<ChartData> {
    let root: YahooChartRoot = serde_json::from_str(body).context("json parse chart")?;
    if let Some(err) = root.chart.error {
        let msg = err.description.unwrap_or_else(|| "Yahoo chart error".into());
        anyhow::bail!(msg);
    }
    let res = root
        .chart
        .result
        .as_ref()
        .and_then(|v| v.first())
        .context("empty chart result")?;
    let meta = &res.meta;
    let prev = meta
        .previous_close
        .or(meta.chart_previous_close)
        .unwrap_or(0.0);
    let price = meta
        .regular_market_price
        .filter(|p| p.is_finite())
        .context("missing regularMarketPrice")?;
    let symbol = meta
        .symbol
        .clone()
        .unwrap_or_else(|| fallback_symbol.to_uppercase());
    let long_name = meta
        .long_name
        .clone()
        .or_else(|| meta.short_name.clone())
        .filter(|s| !s.trim().is_empty());

    let times = res.timestamp.clone().unwrap_or_default();
    let closes_raw = res
        .indicators
        .quote
        .first()
        .map(|q| q.close.clone())
        .unwrap_or_default();

    let mut closes = Vec::new();
    let mut aligned_times = Vec::new();
    for (t, c) in times.iter().zip(closes_raw.iter()) {
        if let Some(v) = c {
            if v.is_finite() {
                aligned_times.push(*t);
                closes.push(*v);
            }
        }
    }

    Ok(ChartData {
        symbol,
        long_name,
        price,
        previous_close: prev,
        times: aligned_times,
        closes,
    })
}

#[derive(Deserialize)]
struct SearchRoot {
    quotes: Option<Vec<SearchQuote>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchQuote {
    symbol: String,
    #[allow(dead_code)]
    longname: Option<String>,
}

/// Validates that Yahoo search returns at least one equity-like symbol; returns canonical symbol.
pub fn search_symbol(query: &str) -> Result<String> {
    let q = urlencoding::encode(query.trim());
    if q.is_empty() {
        anyhow::bail!("empty query");
    }
    let url = format!("https://query1.finance.yahoo.com/v1/finance/search?q={q}");
    let body = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .context("http search")?
        .into_string()
        .context("read search body")?;
    let root: SearchRoot = serde_json::from_str(&body).context("json parse search")?;
    let sym = root
        .quotes
        .and_then(|q| q.into_iter().next())
        .map(|x| x.symbol)
        .context("no matching symbol")?;
    Ok(sym)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chart_fixture() {
        let fixture = include_str!("../tests/fixtures/aapl_1d.json");
        let d = parse_chart_json(fixture, "AAPL").unwrap();
        assert_eq!(d.symbol, "AAPL");
        assert!((d.price - 189.84).abs() < 0.01);
        assert!(!d.closes.is_empty());
        assert_eq!(d.closes.len(), d.times.len());
    }

    #[test]
    fn range_interval_mapping() {
        assert_eq!(TimeRange::D1.yahoo_interval(), "5m");
        assert_eq!(TimeRange::W1.yahoo_interval(), "15m");
        assert_eq!(TimeRange::M1.yahoo_interval(), "1h");
        assert_eq!(TimeRange::Y1.yahoo_interval(), "1d");
        assert_eq!(TimeRange::D1.trailing_window_secs(), 24 * 60 * 60);
        assert_eq!(TimeRange::W1.trailing_window_secs(), 7 * 24 * 60 * 60);
        assert_eq!(TimeRange::M1.trailing_window_secs(), 30 * 24 * 60 * 60);
        assert_eq!(TimeRange::Y1.trailing_window_secs(), 365 * 24 * 60 * 60);
    }
}
