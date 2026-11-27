//! A transient ruler for the indicator's own value scale. It never creates a price drawing.
use super::{frame::{FrameData, FrameLayout}, scripts::{self, PaneGeometry, ReadoutAt}, *};
use crate::model::{MagnetMode, Tool};
use crate::pine::outputs::DISPLAY_PANE;

#[derive(Default)]
pub(super) struct State {
    measure: Option<Measure>,
}

struct Measure {
    source: DataSource,
    symbol: String,
    timeframe: Option<Timeframe>,
    pane: u64,
    a: Anchor,
    b: Anchor,
    dragging: bool,
}

impl Measure {
    fn matches(&self, data: &FrameData, timeframe: Option<Timeframe>) -> bool {
        self.source == data.source && self.symbol == data.symbol && self.timeframe == timeframe
    }
}

pub(super) fn projection<'a>(view: &'a View, data: &'a FrameData<'_>, geometry: &'a PaneGeometry, ctx: &egui::Context) -> Projection<'a> {
    Projection { scale: &geometry.scale, ..view.projection(geometry.plot, data.bars, PriceFormat::AUTO, ctx) }
}

pub(super) fn anchor(proj: &Projection, point: Pos2, pine: &VisualOutput, bars: &[Kline], magnet: MagnetMode) -> Anchor {
    let point = Pos2::new(point.x.clamp(proj.plot.left(), proj.plot.right()), point.y.clamp(proj.plot.top(), proj.plot.bottom()));
    let original = proj.anchor(point);
    if magnet == MagnetMode::Off { return original; }
    let index = proj.index_at(point.x).round();
    if !index.is_finite() || index < 0.0 || index >= bars.len() as f64 { return original; }
    let index = index as usize;
    let nearest = pine.plots.iter().filter(|p| p.display & DISPLAY_PANE != 0)
        .filter_map(|p| scripts::plot_sample(p, ReadoutAt::Bar { count_with_tail: bars.len(), index }))
        .filter(|(_, color)| color.a() != 0)
        .map(|(value, _)| (value, (proj.scale.y(value) - point.y).abs()))
        .filter(|(_, distance)| distance.is_finite())
        .min_by(|a,b| a.1.total_cmp(&b.1));
    match nearest {
        Some((value, distance)) if magnet == MagnetMode::Strong || distance <= 12.0 =>
            Anchor { time_ms: bars[index].open_time, price: value },
        _ => original,
    }
}

impl State {
    pub(super) fn forget(&mut self, pane: u64) {
        if self.measure.as_ref().is_some_and(|m| m.pane == pane) { self.measure = None; }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(&mut self, ui: &egui::Ui, response: &Response, data: &FrameData, layout: &FrameLayout, view: &View, timeframe: Option<Timeframe>, frame: &mut DrawingFrame, blocked: bool) -> bool {
        let (origin, pointer, down, pressed, focused, escape) = ui.input(|i| (
            i.pointer.press_origin(), i.pointer.latest_pos(), i.pointer.primary_down(),
            i.pointer.primary_pressed(), i.focused, i.key_pressed(egui::Key::Escape)
        ));
        if self.measure.as_ref().is_some_and(|m| !m.matches(data, timeframe)) {
            self.measure = None;
        }
        if blocked || frame.keys_blocked || !focused || frame.hide_all {
            if self.measure.as_ref().is_some_and(|m| m.dragging) { self.measure = None; }
            return false;
        }
        if escape && self.measure.take().is_some() {
            frame.escape_consumed = true;
            return true;
        }
        let primary = egui::PointerButton::Primary;
        let shown = view.end.saturating_sub(view.start);
        let tail = data.bars.len().saturating_sub(view.end);
        let pane_at = |point: Pos2| {
            layout.pane_rects.iter().zip(&layout.pane_outputs).zip(&layout.pane_specs)
                .find_map(|((rect, (id, pine)), spec)| {
                    if spec.collapsed || *id == PREVIEW_INSTANCE { return None; }
                    let geometry = PaneGeometry::new(*rect, layout.plot, pine, shown, tail)?;
                    geometry.plot.contains(point).then_some((*id, &***pine, geometry))
                })
        };
        let active = self.measure.as_ref().is_some_and(|m| m.dragging);
        let starts = frame.tool == Tool::Ruler && response.drag_started_by(primary);
        if starts && let Some((pane, pine, geometry)) = origin.and_then(pane_at) {
            let proj = projection(view, data, &geometry, ui.ctx());
            let a = anchor(&proj, origin.unwrap(), pine, data.bars, frame.magnet);
            self.measure = Some(Measure { source: data.source, symbol: data.symbol.to_owned(), timeframe, pane, a, b: a, dragging: true });
        } else if !active && pressed && response.hovered() {
            // A completed measurement is kept until the next click, just like the price ruler.
            self.measure = None;
        }
        if let Some(m) = self.measure.as_mut().filter(|m| m.dragging) {
            if frame.tool != Tool::Ruler {
                self.measure = None;
                return false;
            }
            let pane = layout.pane_rects.iter().zip(&layout.pane_outputs).zip(&layout.pane_specs)
                .find(|((_, (id, _)), spec)| *id == m.pane && !spec.collapsed);
            if let Some(((rect, (_, pine)), _)) = pane {
                if let Some(geometry) = PaneGeometry::new(*rect, layout.plot, pine, shown, tail) {
                    let proj = projection(view, data, &geometry, ui.ctx());
                    if let Some(point) = pointer {
                        m.b = anchor(&proj, point, pine, data.bars, frame.magnet);
                    }
                    if response.drag_stopped_by(primary) {
                        m.dragging = false;
                        frame.put_down |= !frame.keep_drawing;
                    } else if !down {
                        self.measure = None;
                    }
                    return true;
                }
            }
            self.measure = None;
            return true;
        }
        // Claim a ruler press over a pane before the drag threshold, preventing chart navigation.
        frame.tool == Tool::Ruler && down && origin.and_then(pane_at).is_some() && response.hovered()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(&self, painter: &egui::Painter, data: &FrameData, view: &View, geometry: &PaneGeometry, id: u64, pine: &VisualOutput, timeframe: Option<Timeframe>) {
        let Some(m) = self.measure.as_ref().filter(|m| m.pane == id && m.matches(data, timeframe)) else { return; };
        let proj = projection(view, data, geometry, painter.ctx());
        let a = proj.point(m.a);
        let b = proj.point(m.b);
        if !a.x.is_finite() || !a.y.is_finite() || !b.x.is_finite() || !b.y.is_finite() { return; }
        let rect = Rect::from_two_pos(a,b);
        if !rect.intersects(geometry.plot) { return; }
        let clipped = painter.with_clip_rect(geometry.plot.intersect(painter.clip_rect()));
        let color = Color32::from_rgb(66,145,230);
        clipped.rect_filled(rect, 0.0, color.gamma_multiply(0.16));
        clipped.rect_stroke(rect, 0.0, Stroke::new(1.0,color), StrokeKind::Inside);
        clipped.line_segment([a,b], Stroke::new(1.0,color));
        for p in [a,b] { clipped.circle_filled(p, 3.0, color); }
        let count = (proj.index_at(b.x) - proj.index_at(a.x)).abs().round() as usize;
        let duration = duration_text(m.a.time_ms.abs_diff(m.b.time_ms));
        let value = scripts::format_value(pine, None, m.b.price - m.a.price, PriceFormat::AUTO);
        let text = tr!("Δ {value} · {bars} bars · {duration}", value = value, bars = count, duration = duration);
        let galley = clipped.layout_no_wrap(text, FontId::proportional(11.0), Color32::WHITE);
        let size = galley.size() + Vec2::new(12.0,8.0);
        if size.x > geometry.plot.width() || size.y > geometry.plot.height() { return; }
        let x = (rect.center().x - size.x / 2.0).clamp(geometry.plot.left(), geometry.plot.right()-size.x);
        let y = (rect.bottom()+4.0).clamp(geometry.plot.top(), geometry.plot.bottom()-size.y);
        let label = Rect::from_min_size(Pos2::new(x,y),size);
        clipped.rect_filled(label, 4.0, Color32::from_rgb(35,89,155));
        clipped.galley(label.min + Vec2::new(6.0,4.0),galley,Color32::WHITE);
    }
}

fn duration_text(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = seconds / 86400;
    let hours = seconds / 3600 % 24;
    let minutes = seconds / 60 % 60;
    if days > 0 { format!("{days}d {hours}h") }
    else if hours > 0 { format!("{hours}h {minutes}m") }
    else if minutes > 0 { format!("{minutes}m {}s", seconds % 60) }
    else { format!("{seconds}s") }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_is_elapsed_time_not_a_count_of_trading_days() {
        assert_eq!(duration_text(3 * 86_400_000 + 2 * 3_600_000), "3d 2h");
        assert_eq!(duration_text(90_000), "1m 30s");
    }
}
