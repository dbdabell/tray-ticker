//! `egui_plot` helpers.

use crate::data::{ChartData, TimeRange};
use chrono::{Datelike, TimeZone};
use eframe::egui::Color32;
use egui_plot::{Line, PlotPoints, VLine};

pub fn price_line(data: &ChartData) -> Line<'_> {
    let pts = PlotPoints::from_iter(
        data
            .times
            .iter()
            .zip(data.closes.iter())
            .map(|(t, c)| [*t as f64, *c]),
    );
    Line::new(pts).width(1.5)
}

pub fn boundary_vlines(data: &ChartData, range: TimeRange) -> Vec<VLine> {
    if data.times.len() < 2 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut prev = match chrono::Local.timestamp_opt(data.times[0], 0).single() {
        Some(dt) => dt,
        None => return Vec::new(),
    };

    for &ts in data.times.iter().skip(1) {
        let Some(curr) = chrono::Local.timestamp_opt(ts, 0).single() else {
            continue;
        };
        let is_boundary = match range {
            TimeRange::D1 | TimeRange::W1 => curr.date_naive() != prev.date_naive(),
            TimeRange::M1 => curr.iso_week() != prev.iso_week(),
            TimeRange::Y1 => {
                curr.year() != prev.year() || curr.month() != prev.month()
            }
        };
        if is_boundary {
            lines.push(
                VLine::new(ts as f64)
                    .width(1.0)
                    .color(Color32::from_gray(95)),
            );
        }
        prev = curr;
    }

    lines
}
