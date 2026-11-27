//! The Pine scripts on a chart card: their instances and settings, the outputs the host's runs
//! hand back, the pane buttons, and the own pane an `overlay=false` script draws in.

use super::*;
use crate::pine::time::bucket::seconds_of;
use crate::pine::outputs::{PlotOutput, DISPLAY_STATUS_LINE, DISPLAY_PRICE_SCALE};
use crate::ui::pine::geometry::{series_index, rgba};

impl ChartCard {
    pub fn pine_instances(&self) -> &[PineInstance] {
        &self.instances
    }

    /// Replaces every instance (restoring a saved chart).
    pub fn set_pine_instances(&mut self, instances: Vec<PineInstance>) {
        self.pane_ids.retain(|id| instances.iter().any(|instance| instance.id == *id));
        if self.maximized_pane.is_some_and(|id| !instances.iter().any(|instance| instance.id == id)) {
            self.maximized_pane = None;
        }
        self.pane_measure = Default::default();
        self.pane_drawings = Default::default();
        self.instances = instances;
        self.pine.reset();
    }

    /// Applies a script to the chart, after the ones already there. Returns the new instance's id.
    pub fn add_pine_instance(
        &mut self,
        script_id: Option<String>,
        name: &str,
        source: &str,
    ) -> u64 {
        let id = self
            .instances
            .iter()
            .map(|i| i.id)
            .max()
            .map_or(1, |max| max + 1);
        self.instances
            .push(PineInstance::new(id, script_id, name, source));
        id
    }

    pub fn remove_pine_instance(&mut self, id: u64) {
        self.pane_measure.forget(id);
        self.instances.retain(|instance| instance.id != id);
        self.pine.remove(id);
    }

    /// Moves an instance to `index` (clamped); draw order follows list order.
    pub fn move_pine_instance(&mut self, id: u64, index: usize) {
        if let Some(from) = self.instances.iter().position(|i| i.id == id) {
            let instance = self.instances.remove(from);
            let to = index.min(self.instances.len());
            self.instances.insert(to, instance);
        }
    }

    /// Sends an instance's visuals in front of or behind the chart's own candles; a restyle, so it
    /// never reruns the script.
    pub fn set_pine_visual_order(&mut self, id: u64, order: VisualOrder) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) {
            instance.visual_order = order;
        }
    }

    pub fn set_pine_visible(&mut self, id: u64, visible: bool) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) {
            instance.visible = visible;
        }
    }

    /// Sets an instance's style overrides; the next frame draws with them.
    pub fn set_pine_instance_style(&mut self, id: u64, overrides: HashMap<String, String>) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) {
            instance.style_overrides = overrides;
        }
    }

    /// What the user can restyle in an instance's output, as the script itself draws it. Empty
    /// until the script has run.
    pub fn pine_style_rows(&self, id: u64) -> Vec<StyleRow> {
        self.pine.style_rows(id)
    }

    /// Sets an instance's input overrides; its next run uses them.
    pub fn set_pine_instance_inputs(&mut self, id: u64, inputs: &HashMap<String, InputValue>) {
        if let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) {
            instance.set_input_values(inputs);
        }
    }

    /// The messages an instance's script wrote with `log.*()` in its current run.
    pub fn pine_logs(&self, id: u64) -> Option<PineLogs<'_>> {
        self.pine.logs(id)
    }

    /// Forgets the messages kept for an instance's run. The run goes on and writes more.
    pub fn clear_pine_logs(&mut self, id: u64) {
        self.pine.clear_logs(id);
    }

    /// Whether an instance keeps the messages it writes in realtime.
    pub fn pine_logging(&self, id: u64) -> bool {
        self.pine.logging(id)
    }

    /// Turns the keeping of an instance's realtime messages on or off. Messages already kept stay.
    pub fn set_pine_logging(&mut self, id: u64, enabled: bool) {
        self.pine.set_logging(id, enabled);
    }

    /// Whether a script on this chart is visible, so it runs: the chart is asked for the history
    /// scripts are meant to run over.
    pub fn runs_scripts(&self) -> bool {
        self.instances.iter().any(|instance| instance.visible)
    }

    pub fn pine_status(&self, id: u64) -> Option<InstanceStatus<'_>> {
        self.pine.status(id)
    }

    /// Whether the instance's latest run has been going on long enough to show a busy state. A
    /// hidden instance has no run, so it is never busy.
    pub fn pine_busy(&self, id: u64, now: Instant) -> bool {
        self.pine.busy(id, now)
    }

    // The single-script preview (Pine Studio): one instance, id 0, that keeps its last good
    // output while a newer source fails to run.

    /// Sets (or clears) the one script the preview chart runs.
    pub fn set_pine_source(&mut self, source: Option<String>) {
        match source {
            Some(source) => match self.instances.first_mut() {
                Some(instance) if instance.id == PREVIEW_INSTANCE => instance.source = source,
                _ => {
                    let mut instance =
                        PineInstance::new(PREVIEW_INSTANCE, None, "Preview", &source);
                    instance.set_input_values(&self.preview_inputs);
                    self.instances = vec![instance];
                    self.pine.stop_all();
                }
            },
            None => {
                self.instances.clear();
                self.pine.stop_all();
            }
        }
    }

    /// Sets the input overrides the preview script runs with.
    pub fn set_pine_inputs(&mut self, inputs: HashMap<String, InputValue>) {
        self.set_pine_instance_inputs(PREVIEW_INSTANCE, &inputs);
        self.preview_inputs = inputs;
    }

    /// The preview run's runtime failure, if it stopped early.
    pub fn pine_error(&self) -> Option<&Diagnostic> {
        self.pine.error(PREVIEW_INSTANCE)
    }

    /// Whether a preview run has finished cleanly, so a later failure leaves real output up.
    pub fn has_valid_pine_output(&self) -> bool {
        self.pine.has_valid_output(PREVIEW_INSTANCE)
    }

    /// The preview run's output (strategy report, alerts), for the Report tab.
    pub fn pine_visual(&self) -> Option<&VisualOutput> {
        self.pine.output(PREVIEW_INSTANCE)
    }

    /// The visible instances' outputs in order, from the runs the host keeps for them: this frame
    /// hands them the bars it draws, so a run starts again when what the script reads changed.
    pub(super) fn pine_outputs(
        &mut self,
        ctx: &egui::Context,
        symbol_info: &SymbolInfo,
        bars: &[Kline],
        bar_seconds: Option<f64>,
    ) -> Vec<frame::Placed> {
        let chart = ChartFrame {
            bars,
            symbol: symbol_info,
            bar_seconds,
            timeframe_seconds: self.timeframe.map(seconds_of),
            // What `chart.bg_color` and `chart.fg_color` read: the colours the chart is drawn on.
            theme: if theme::is_light() {
                ChartTheme::LIGHT
            } else {
                ChartTheme::DARK
            },
            visible_range: self.visible_open_range(bars),
        };
        self.pine.update(ctx, &chart, &self.instances)
    }

    /// Applies a pane button. Order, collapse and removal act on the script's own instance, so
    /// there is one list of what is on the chart and in what order.
    pub(super) fn apply_pane_action(&mut self, id: u64, action: PaneAction) {
        match action {
            PaneAction::MoveUp | PaneAction::MoveDown => {
                let Some(position) = self.pane_ids.iter().position(|pane| *pane == id) else {
                    return;
                };
                let neighbour = if action == PaneAction::MoveUp {
                    position.checked_sub(1)
                } else {
                    Some(position + 1)
                }
                .and_then(|index| self.pane_ids.get(index).copied());
                // Panes are drawn in the order of their scripts, so a pane trades places with its
                // neighbour by taking the neighbour's place in the instance list.
                if let Some(other) = neighbour
                    && let Some(index) = self.instances.iter().position(|i| i.id == other)
                {
                    self.move_pine_instance(id, index);
                    // The next click, before the next frame, sees the new order.
                    if let Some(at) = self.pane_ids.iter().position(|pane| *pane == other) {
                        self.pane_ids.swap(position, at);
                    }
                }
            }
            PaneAction::ToggleCollapse => {
                if let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) {
                    instance.pane.collapsed = !instance.pane.collapsed;
                }
                if self.maximized_pane == Some(id) {
                    self.maximized_pane = None;
                }
            }
            PaneAction::ToggleMaximize => {
                if self.maximized_pane == Some(id) {
                    self.maximized_pane = None;
                } else {
                    // A maximized pane is expanded: that is the state it returns to.
                    if let Some(instance) = self.instances.iter_mut().find(|i| i.id == id) {
                        instance.pane.collapsed = false;
                    }
                    self.maximized_pane = Some(id);
                }
            }
            PaneAction::Remove => {
                self.remove_pine_instance(id);
                if self.maximized_pane == Some(id) {
                    self.maximized_pane = None;
                }
            }
        }
    }
}

/// Plot arrays are already shifted by the runtime. Read the same slot as PineLayer,
/// without running the script or filling missing historical samples with a previous value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum ReadoutAt {
    Latest,
    Bar { count_with_tail: usize, index: usize },
    Missing,
}

pub(super) fn plot_sample(plot: &PlotOutput, at: ReadoutAt) -> Option<(f64, Color32)> {
    let (index, count) = match at {
        ReadoutAt::Latest => {
            let index = plot.values.iter().rposition(|v| v.is_some_and(f64::is_finite))?;
            (index, plot.values.len())
        }
        ReadoutAt::Bar { count_with_tail, index } => (index, count_with_tail),
        ReadoutAt::Missing => return None,
    };
    let slot = series_index(plot.values.len(), count, index)?;
    let value = plot.values.get(slot).copied().flatten().filter(|v| v.is_finite())?;
    let color = series_index(plot.colors.len(), count, index)
        .and_then(|i| plot.colors.get(i).copied().flatten()).unwrap_or(plot.color);
    Some((value, rgba(color)))
}

pub(super) fn readout_at(x: Option<f32>, plot: Rect, slot_width: f32, lead: f32, shown: usize, tail: usize) -> ReadoutAt {
    let Some(x) = x else { return ReadoutAt::Latest; };
    if !x.is_finite() || !slot_width.is_finite() || slot_width <= 0.0 { return ReadoutAt::Missing; }
    let index = ((x - plot.left()) / slot_width - lead).floor() as isize;
    match usize::try_from(index) {
        Ok(index) if index < shown => ReadoutAt::Bar { count_with_tail: shown + tail, index },
        _ => ReadoutAt::Missing,
    }
}

/// Indicator / plot-specific numeric presentation. A plot may override its declaration.
/// Percent values are already in percent units; never multiply the series by 100.
pub(super) fn format_value(pine: &VisualOutput, id: Option<usize>, value: f64, fallback: PriceFormat) -> String {
    let per_plot = id.and_then(|id| pine.presentation.plots.get(&id));
    let precision = per_plot.and_then(|f| f.precision).or(pine.presentation.number.precision)
        .map(|p| p.clamp(0, 12) as usize);
    let inherited = |name: &str| !matches!(name, "inherit" | "format.inherit");
    let kind = per_plot.and_then(|f| f.format.as_deref()).filter(|s| inherited(s))
        .or(pine.presentation.number.format.as_deref().filter(|s| inherited(s)));
    if !value.is_finite() { return "—".into(); }
    let fixed = |v: f64, p: usize| {
        let rounded = format!("{v:.p$}");
        // Avoid negative zero on precision=0 and other small values.
        if rounded.starts_with('-') && rounded[1..].bytes().all(|c| c == b'0' || c == b'.') {
            rounded[1..].to_owned()
        } else { rounded }
    };
    match kind {
        Some("volume" | "format.volume") => {
            let (divisor, suffix) = [(1e12,"T"), (1e9,"B"), (1e6,"M"), (1e3,"K")]
                .into_iter().find(|(n,_)| value.abs() >= *n).unwrap_or((1.0,""));
            format!("{}{suffix}", fixed(value / divisor, precision.unwrap_or(2)))
        }
        Some("percent" | "format.percent") => format!("{}%", fixed(value, precision.unwrap_or(2))),
        _ => precision.map_or_else(|| fallback.format(value), |p| fixed(value,p)),
    }
}

pub(super) fn status_title<'a>(pine: &'a VisualOutput, fallback: &'a str) -> &'a str {
    if pine.presentation.title.is_empty() { fallback } else { &pine.presentation.title }
}

/// One readout row, shared by the live chart, Pine Studio and snapshots. No dependence on
/// whether the panel controls happen to be hovered. Reserve their area to keep text steady.
pub(super) fn paint_status(
    painter: &egui::Painter, area: Rect, pine: &VisualOutput, fallback_name: &str,
    at: ReadoutAt, fallback: PriceFormat, style: &CanvasStyle,
) {
    if area.width() <= 8.0 || area.height() < 12.0 { return; }
    let prefs = style.indicator_readouts;
    let font = FontId::proportional(11.0);
    let mut job = egui::text::LayoutJob::default();
    let mut append = |text: &str, color: Color32| {
        if !text.is_empty() {
            job.append(text, if job.text.is_empty() { 0.0 } else { 9.0 },
                egui::TextFormat::simple(font.clone(), color));
        }
    };
    if prefs.names { append(status_title(pine, fallback_name), style.text_color); }
    if prefs.parameters && !pine.presentation.parameters.is_empty() {
        append(&pine.presentation.parameters.join(" "), style.text_color);
    }
    if prefs.values {
        for plot in pine.plots.iter().filter(|p| p.display & DISPLAY_STATUS_LINE != 0).take(64) {
            if let Some((value, color)) = plot_sample(plot, at).filter(|(_, color)| color.a() > 0) {
                append(&format_value(pine, Some(plot.id), value, fallback), color);
            }
        }
    }
    if job.text.is_empty() { return; }
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(area));
    let galley = painter.layout_job(job);
    painter.galley(area.min + Vec2::new(6.0, 3.0), galley, style.text_color);
}

/// The exact pane plot rectangle used for painting and pointer hit tests.
pub(super) fn pane_plot(pane: Rect, price_plot: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(price_plot.left(), pane.top() + 8.0),
        Pos2::new(price_plot.right(), pane.bottom() - 8.0),
    )
}

/// The same linear transform is used for the script, its grid, last values, and the inverse
/// mapping of the pointer's Y. Construct it only once for a pane in the painted frame.
pub(super) struct PaneGeometry {
    pub plot: Rect,
    pub scale: PriceScale,
}

impl PaneGeometry {
    pub(super) fn new(pane: Rect, price_plot: Rect, pine: &VisualOutput, shown: usize, tail: usize) -> Option<Self> {
        let plot = pane_plot(pane, price_plot);
        if plot.height() <= 1.0 || plot.width() <= 0.0 { return None; }
        let (low, high) = crate::ui::pine::geometry::pane_value_range_at(pine, shown, tail, 0.1);
        let scale = PriceScale::linear(low, high, plot.top(), plot.height());
        Some(Self { plot, scale })
    }
}

fn range_labels(pine: &VisualOutput, low: f64, high: f64, height: f32, style: &CanvasStyle) -> Vec<(f64, String)> {
    let budget = chart::canvas::tick_budget(height, style.pane_tick_spacing()).max(2);
    let ticks = chart::canvas::price_ticks(low, high, false, budget);
    let step = tick_step(&ticks);
    ticks.into_iter().map(|value| {
        let formatted = pine.presentation.number.precision.is_some()
            || pine.presentation.number.format.as_deref().is_some_and(|s| !matches!(s, "inherit" | "format.inherit"));
        let label = if formatted {
            format_value(pine, None, value, PriceFormat::AUTO)
        } else if value.abs() < (high-low).abs()*1e-9 {
            "0".to_owned()
        } else {
            PriceFormat::AUTO.format_tick(value, step)
        };
        (value, label)
    }).collect()
}

pub(super) fn pane_labels(pine: &VisualOutput, shown: usize, tail: usize, plot_height: f32, style: &CanvasStyle) -> Vec<(f64, String)> {
    let (low, high) = crate::ui::pine::geometry::pane_value_range_at(pine, shown, tail, 0.1);
    range_labels(pine, low, high, plot_height, style)
}

pub(super) type IndicatorTag = (f32, Color32, String);

/// Permanent values use the latest available finite sample, not the cursor's historical bar.
pub(super) fn last_tags(pine: &VisualOutput, fallback: PriceFormat, scale: &PriceScale, plot: Rect) -> Vec<IndicatorTag> {
    pine.plots.iter().filter(|p| p.display & DISPLAY_PRICE_SCALE != 0).take(64)
        .filter_map(|p| {
            let (value, color) = plot_sample(p, ReadoutAt::Latest)?;
            if color.a() == 0 { return None; }
            let y = scale.y(value);
            if !y.is_finite() { return None; }
            Some((y.clamp(plot.top(), plot.bottom()), color, format_value(pine, Some(p.id), value, fallback)))
        }).collect()
}

/// Nearest free axis slot, with strict non-overlap even when there are more series than space.
/// Fixed bands (main price, crosshair) have priority. This is presentation only, never autoscale.
pub(super) fn place_indicator_tags(mut tags: Vec<IndicatorTag>, half: f32, plot: Rect, avoid: &[TagBand]) -> Vec<IndicatorTag> {
    let top = plot.top() + half;
    let bottom = plot.bottom() - half;
    if !top.is_finite() || !bottom.is_finite() || top > bottom { return Vec::new(); }
    let pitch = 2.0 * half + 1.0;
    tags.retain(|tag| tag.0.is_finite());
    tags.sort_by(|a,b| a.0.total_cmp(&b.0));
    let mut placed: Vec<IndicatorTag> = Vec::new();
    for (desired, color, text) in tags {
        let mut candidates = vec![desired.clamp(top,bottom), top, bottom];
        for (y, _, _) in &placed { candidates.extend([*y-pitch,*y+pitch]); }
        for band in avoid { candidates.extend([band.top-half-1.0,band.bottom+half+1.0]); }
        let free = candidates.into_iter().filter(|y| {
            *y >= top && *y <= bottom
                && placed.iter().all(|(other,_,_)| (*y-*other).abs() >= pitch-0.001)
                && avoid.iter().all(|band| *y+half <= band.top-0.999 || *y-half >= band.bottom+0.999)
        }).min_by(|a,b| (a-desired).abs().total_cmp(&(b-desired).abs()));
        if let Some(y) = free { placed.push((y,color,text)); }
    }
    placed
}

pub(super) fn paint_indicator_tags(painter: &egui::Painter, plot: Rect, right: f32, style: &CanvasStyle, tags: Vec<IndicatorTag>, avoid: &[TagBand]) {
    let half = style.tag_half_height();
    for (y, color, text) in place_indicator_tags(tags, half, plot, avoid) {
        let tag = Rect::from_min_max(Pos2::new(plot.right()+3.0,y-half), Pos2::new(right-3.0,y+half));
        if tag.width() <= 0.0 { continue; }
        let fill = Color32::from_rgb(color.r(),color.g(),color.b());
        painter.rect_filled(tag,3.0,fill);
        let clipped = painter.with_clip_rect(painter.clip_rect().intersect(tag));
        price_text::paint(&clipped, tag.center(), Align2::CENTER_CENTER, &text, style.price_font(), overlays::on_color(fill));
    }
}

/// A script pane and its independent value scale.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_script_pane(
    painter: &egui::Painter,
    pane: Rect,
    geometry: &PaneGeometry,
    pine: &VisualOutput,
    shown: &[Kline],
    tail: usize,
    slot_width: f32,
    x_for_index: &dyn Fn(i64) -> f32,
    style: &CanvasStyle,
    tick_xs: &[f32],
    pointer: Option<Pos2>,
) -> Option<HoverTarget> {
    let plot = geometry.plot;
    let scale = &geometry.scale;
    let y_for = |value: f64| scale.y(value);
    let labels = range_labels(pine, scale.minimum, scale.maximum, plot.height(), style);
    if let Some(grid) = style.vertical_grid {
        for x in tick_xs {
            if *x >= plot.left() && *x <= plot.right() {
                canvas::vertical(painter, *x, (plot.top(), plot.bottom()), grid);
            }
        }
    }
    for (value,label) in labels {
        let y = y_for(value);
        if !y.is_finite() || y < plot.top()-0.5 || y > plot.bottom()+0.5 { continue; }
        if let Some(grid) = style.horizontal_grid {
            canvas::horizontal(painter,(plot.left(),plot.right()),y,grid);
        }
        price_text::paint(painter,Pos2::new(pane.right()-7.0,y),Align2::RIGHT_CENTER,&label,style.price_font(),style.text_color);
    }
    let layer = PineLayer { pine, candles: shown, tail, in_pane: true, plot, slot_width, x_for_index, y_for: &y_for };
    let clipped = painter.with_clip_rect(plot.intersect(painter.clip_rect()));
    layer.draw_background(&clipped);
    layer.draw_foreground(&clipped);
    layer.draw_tables(painter);
    let pointer = pointer.filter(|p| plot.contains(*p) && layer.has_tooltips())?;
    chart_render::target_at(&layer.hover_targets(painter),pointer).cloned()
}


#[cfg(test)]
mod indicator_readout_tests {
    use super::*;
    use crate::pine::compiler::program::ValueFormat;
    use crate::pine::outputs::{DISPLAY_ALL, PlotStyle};

    fn sample(values: Vec<Option<f64>>) -> PlotOutput {
        PlotOutput {
            id: 7, title: Some("COT".into()),
            colors: vec![Some(0x26a69aff); values.len()],
            values, color: 0x26a69aff, line_width: 1,
            style: PlotStyle::Line, display: DISPLAY_ALL,
            hist_base: 0.0, offset: 0, force_overlay: false,
        }
    }

    #[test]
    fn historical_readout_uses_shifted_series_and_never_fills_gaps() {
        let mut plot = sample(vec![Some(12.0), None, Some(95.69), Some(-3.0)]);
        plot.colors[2] = Some(0xf23645ff);
        plot.offset = 1;
        let plot = plot.shifted();
        assert_eq!(plot_sample(&plot, ReadoutAt::Bar { count_with_tail: 4, index: 0 }), None);
        assert_eq!(plot_sample(&plot, ReadoutAt::Bar { count_with_tail: 4, index: 1 }).unwrap().0, 12.0);
        assert_eq!(plot_sample(&plot, ReadoutAt::Bar { count_with_tail: 4, index: 2 }), None);
        assert_eq!(plot_sample(&plot, ReadoutAt::Latest), Some((95.69, rgba(0xf23645ff))));
        assert_eq!(plot_sample(&plot, ReadoutAt::Missing), None);
        let plot = sample(vec![Some(10.0), Some(20.0), Some(30.0), Some(40.0)]);
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        let at = readout_at(Some(15.0), area, 10.0, 0.0, 2, 1);
        assert_eq!(plot_sample(&plot, at).unwrap().0, 30.0);
    }

    #[test]
    fn cot_precision_and_plot_override_are_independent_of_market() {
        let mut pine = VisualOutput::default();
        pine.presentation.number.precision = Some(0);
        assert_eq!(format_value(&pine, None, 109292.0, PriceFormat::AUTO), "109292");
        pine.presentation.number.precision = Some(2);
        assert_eq!(format_value(&pine, None, 95.69, PriceFormat::AUTO), "95.69");
        assert_eq!(format_value(&pine, None, -0.001, PriceFormat::AUTO), "0.00");
        pine.presentation.plots.insert(7, ValueFormat { format: Some("format.percent".into()), precision: Some(1) });
        assert_eq!(format_value(&pine, Some(7), -3.24, PriceFormat::AUTO), "-3.2%");
        assert_eq!(format_value(&pine, None, -3.24, PriceFormat::AUTO), "-3.24");
    }

    #[test]
    fn cursor_level_is_not_the_plot_sample_and_historical_range_ignores_future() {
        let mut pine = VisualOutput::default();
        pine.bar_count = 4;
        pine.plots.push(sample(vec![Some(60.0), Some(95.69), Some(10000.0), Some(20000.0)]));
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 150.0));
        let geometry = PaneGeometry::new(area, area, &pine, 2, 2).unwrap();
        assert!(geometry.scale.maximum < 110.0);
        let cursor_y = geometry.scale.y(80.0);
        assert!((geometry.scale.price(cursor_y) - 80.0).abs() < 0.001);
        assert_eq!(plot_sample(&pine.plots[0], ReadoutAt::Bar { count_with_tail: 4, index: 1 }).unwrap().0, 95.69);
        pine.plots[0].display = DISPLAY_STATUS_LINE;
        assert!(last_tags(&pine, PriceFormat::AUTO, &geometry.scale, geometry.plot).is_empty());
        pine.plots[0].display = DISPLAY_PRICE_SCALE;
        assert_eq!(last_tags(&pine, PriceFormat::AUTO, &geometry.scale, geometry.plot).len(), 1);
    }

    #[test]
    fn crowded_labels_avoid_each_other_and_crosshair() {
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 150.0));
        let tags = (0..5).map(|n| (75.0, Color32::RED, n.to_string())).collect();
        let avoid = TagBand { top: 65.0, bottom: 85.0 };
        let placed = place_indicator_tags(tags, 8.0, area, &[avoid]);
        assert_eq!(placed.len(), 5);
        for (i, (y, _, _)) in placed.iter().enumerate() {
            assert!(*y >= 8.0 && *y <= 142.0);
            assert!(*y + 8.0 < avoid.top || *y - 8.0 > avoid.bottom);
            for (other, _, _) in placed.iter().skip(i + 1) {
                assert!((*y-*other).abs() >= 16.0);
            }
        }
    }
}
