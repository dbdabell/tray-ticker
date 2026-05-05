//! `egui_plot` helpers.

use crate::data::ChartData;
use egui_plot::{Line, PlotPoints};

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
