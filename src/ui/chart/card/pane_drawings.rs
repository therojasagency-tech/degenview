//! Persistent value-space drawings. Reuses the price drawing model and painter,
//! but never feeds a pane drawing into the price layer or the alert system.
use super::{frame::{FrameData, FrameLayout}, pane_measure::{anchor, projection}, scripts::PaneGeometry, *};
use crate::model::{Drawing, DrawingCommand, InstrumentId, Tool, MagnetMode};

#[derive(Default)]
pub(super) struct State {
    draft: Option<Draft>,
    movement: Option<Movement>,
    selected: Option<(String,u64)>,
}

struct Movement {
    key: String,
    original: Drawing,
    preview: Drawing,
    press: Anchor,
    endpoint: Option<usize>,
}

struct Draft {
    key: String,
    tool: Tool,
    a: Anchor,
    b: Anchor,
    dragging: bool,
}

fn moved(original: &Drawing, press: Anchor, target: Anchor, endpoint: Option<usize>) -> Option<Drawing> {
    let mut drawing = original.clone();
    if let Some(index) = endpoint {
        let mut anchors = drawing.anchors_mut();
        **anchors.get_mut(index)? = target;
    } else {
        let dt = target.time_ms.saturating_sub(press.time_ms);
        let dv = target.price - press.price;
        for at in drawing.anchors_mut() {
            at.time_ms = at.time_ms.checked_add(dt)?;
            at.price += dv;
        }
    }
    if let (Some(current),Some(before)) = (drawing.geometry_mut(),original.geometry()) {
        current.constrain(before);
        if !current.is_valid() { return None; }
    }
    drawing.anchors().iter().all(Anchor::is_finite).then_some(drawing)
}

fn single(tool: Tool) -> bool {
    matches!(tool, Tool::HorizontalLine | Tool::HorizontalRay | Tool::VerticalLine | Tool::Crossline)
}

fn supported(tool: Tool) -> bool {
    single(tool) || matches!(tool, Tool::TrendLine | Tool::Ray | Tool::ExtendedLine | Tool::Rectangle | Tool::FibRetracement)
}

/// A stable identity survives switching indicator templates, whose runtime IDs change.
/// Same scripts with different input overrides are isolated, as are identical duplicates.
/// The reserved namespace is not a valid provider-qualified market key.
fn key(data: &FrameData, timeframe: Option<Timeframe>, instances: &[PineInstance], id: u64) -> Option<String> {
    if data.chart_type.is_synthetic() { return None; }
    let instance = instances.iter().find(|i| i.id == id)?;
    let signature = |i: &PineInstance| {
        let inputs: std::collections::BTreeMap<_,_> = i.inputs.iter().collect();
        serde_json::to_vec(&(i.script_id.as_deref(), i.source.as_str(), inputs, timeframe, data.chart_type)).ok()
    };
    let bytes = signature(instance)?;
    let ordinal = instances.iter().take_while(|i| i.id != id)
        .filter(|i| signature(i).as_deref() == Some(bytes.as_slice())).count();
    let digest = crate::model::encoding::sha256_hex(&bytes);
    Some(format!("@pane/v1/{}/{digest}/{ordinal}", InstrumentId::new(data.source, data.symbol).key()))
}

fn build(draft: &Draft, id: u64, frame: &DrawingFrame) -> Option<Drawing> {
    let style = frame.defaults.and_then(|styles| styles.get(&draft.tool)).copied()
        .unwrap_or_else(|| draft.tool.default_style());
    let points = [draft.a,draft.b];
    Drawing::build(draft.tool, id, &points[..if single(draft.tool) { 1 } else { 2 }], style)
}

fn commit(draft: Draft, frame: &mut DrawingFrame) {
    let max_id = frame.drawings.iter().map(Drawing::stored_id).max().unwrap_or(0)
        .max(frame.pane_sets.and_then(|s| s.highest_id()).unwrap_or(0));
    let Some(id) = max_id.checked_add(1).map(|n| n.max(frame.next_id)) else { return; };
    let Some(next) = id.checked_add(1) else { return; };
    if let Some(drawing) = build(&draft,id,frame) {
        frame.pane_commands.push((draft.key, DrawingCommand::Add(drawing)));
        frame.next_id = next;
        frame.put_down |= !frame.keep_drawing;
        frame.reveal_all = true;
    }
}

impl State {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(&mut self, ui: &egui::Ui, response: &Response, data: &FrameData, layout: &FrameLayout, view: &View, timeframe: Option<Timeframe>, instances: &[PineInstance], frame: &mut DrawingFrame, blocked: bool) -> bool {
        let Some(sets) = frame.pane_sets else { self.draft = None; self.movement = None; self.selected = None; return false; };
        let (origin,pointer,down,escape,delete,modifiers) = ui.input(|i| (
            i.pointer.press_origin(), i.pointer.latest_pos(), i.pointer.primary_down(),
            i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace), i.modifiers
        ));
        if blocked || frame.keys_blocked || frame.hide_all || !ui.input(|i| i.focused) {
            self.draft = None;
            self.movement = None;
            return false;
        }
        if frame.tool != Tool::Cursor { self.movement = None; }
        if self.draft.as_ref().is_some_and(|d| d.tool != frame.tool) { self.draft = None; }
        if escape && (self.draft.is_some() || self.movement.is_some() || self.selected.is_some()) {
            self.draft = None; self.movement = None; self.selected = None;
            frame.escape_consumed = true;
            return true;
        }
        let primary = egui::PointerButton::Primary;
        let shown = view.end.saturating_sub(view.start);
        let tail = data.bars.len().saturating_sub(view.end);
        let panes: Vec<_> = layout.pane_rects.iter().zip(&layout.pane_outputs).zip(&layout.pane_specs)
            .filter_map(|((rect,(id,pine)),spec)| {
                if spec.collapsed || *id == PREVIEW_INSTANCE { return None; }
                Some((key(data,timeframe,instances,*id)?, &***pine, PaneGeometry::new(*rect,layout.plot,pine,shown,tail)?))
            }).collect();
        if self.selected.as_ref().is_some_and(|(key,_)| !panes.iter().any(|(k,_,_)| k == key)) {
            self.selected = None;
        }
        let pane_at = |point: Pos2| panes.iter().find(|(_,_,geometry)| geometry.plot.contains(point));
        let magnet = if modifiers.shift { MagnetMode::Off } else { frame.magnet.inverted(modifiers.command) };
        let pressed_pane = origin.and_then(pane_at);
        let hovered_pane = pointer.and_then(pane_at);
        let eraser = frame.cursor.caps().erases;
        if eraser { self.movement = None; }
        if let Some(movement) = &mut self.movement {
            let Some((_,pine,geometry)) = panes.iter().find(|(key,_,_)| *key == movement.key) else {
                self.movement = None;
                return false;
            };
            let proj = projection(view,data,geometry,ui.ctx());
            if let Some(point) = pointer {
                let target = anchor(&proj,point,pine,data.bars,magnet);
                if let Some(drawing) = moved(&movement.original,movement.press,target,movement.endpoint) {
                    movement.preview = drawing;
                }
            }
            if response.drag_stopped_by(primary) {
                let movement = self.movement.take().unwrap();
                if movement.preview != movement.original {
                    frame.pane_commands.push((movement.key,DrawingCommand::Replace { id: movement.original.id(), drawing: movement.preview }));
                }
            } else if !down { self.movement = None; }
            return true;
        }
        if frame.tool == Tool::Cursor && !eraser && frame.cursor.caps().edits_drawings && response.drag_started_by(primary)
            && let Some((key,pine,geometry)) = pressed_pane
            && let Some(point) = origin
        {
            let proj = projection(view,data,geometry,ui.ctx());
            let drawings = sets.get(key);
            let selected_handle = self.selected.as_ref().filter(|(k,_)| k == key)
                .and_then(|(_,id)| drawings.iter().find(|d| d.id() == *id && !d.locked() && d.is_visible(frame.timeframe)))
                .and_then(|d| d.anchors().iter().enumerate()
                    .map(|(index,a)| (index,proj.point(*a).distance(point)))
                    .filter(|(_,distance)| *distance <= 9.0)
                    .min_by(|a,b| a.1.total_cmp(&b.1))
                    .map(|(index,_)| (d.id(),Some(index))));
            let hit = selected_handle.or_else(|| {
                let local = DrawingFrame::new(drawings,Tool::Cursor,None,frame.next_id).with_view(frame.timeframe,false);
                DrawingLayer::default().hit_drawing_id(&local,point,&proj).map(|id| (id,None))
            });
            if let Some((id,endpoint)) = hit
                && let Some(drawing) = drawings.iter().find(|d| d.id() == id && !d.locked())
            {
                let press = if endpoint.is_some() { proj.anchor(point) } else { anchor(&proj,point,pine,data.bars,magnet) };
                self.selected = Some((key.clone(),id));
                self.movement = Some(Movement { key:key.clone(), original:drawing.clone(), preview:drawing.clone(), press, endpoint });
                frame.select = Some(Vec::new());
                return true;
            }
        }
        if (eraser || frame.tool == Tool::Cursor) && response.clicked_by(primary) {
            self.selected = None;
            if let Some((key,_,geometry)) = hovered_pane {
                let projection = projection(view,data,geometry,ui.ctx());
                let drawings = sets.get(key);
                let local = DrawingFrame::new(drawings,Tool::Cursor,None,frame.next_id).with_view(frame.timeframe,false);
                if let Some(id) = DrawingLayer::default().hit_drawing_id(&local,pointer.unwrap(),&projection) {
                    if eraser && drawings.iter().any(|d| d.id() == id && !d.locked()) {
                        frame.pane_commands.push((key.clone(),DrawingCommand::Remove(id)));
                    } else if !eraser && frame.cursor.caps().edits_drawings {
                        self.selected = Some((key.clone(),id));
                    }
                }
                frame.select = Some(Vec::new());
                return true;
            }
        }
        if delete && frame.tool == Tool::Cursor && !ui.ctx().egui_wants_keyboard_input()
            && response.hovered() && hovered_pane.is_some_and(|(key,_,_)| self.selected.as_ref().is_some_and(|(selected,_)| selected == key))
            && let Some((key,id)) = self.selected.take()
        {
            if sets.get(&key).iter().any(|d| d.id() == id && !d.locked()) {
                frame.pane_commands.push((key,DrawingCommand::Remove(id)));
            }
            return true;
        }
        if !supported(frame.tool) { return false; }
        if let Some(draft) = &mut self.draft {
            let Some((_,pine,geometry)) = panes.iter().find(|(key,_,_)| *key == draft.key) else {
                self.draft = None;
                return false;
            };
            let proj = projection(view,data,geometry,ui.ctx());
            if let Some(point) = pointer {
                draft.b = anchor(&proj,point,pine,data.bars,magnet);
            }
            if response.drag_started_by(primary) && pressed_pane.is_some_and(|(key,_,_)| *key == draft.key) {
                draft.dragging = true;
            }
            let finish = (draft.dragging && response.drag_stopped_by(primary))
                || (!draft.dragging && response.clicked_by(primary) && hovered_pane.is_some_and(|(key,_,_)| *key == draft.key));
            if finish {
                let draft = self.draft.take().unwrap();
                commit(draft,frame);
            } else if (draft.dragging && !down) || (response.clicked_by(primary) && !hovered_pane.is_some_and(|(key,_,_)| *key == draft.key)) {
                self.draft = None;
            }
            return true;
        }
        let drag = response.drag_started_by(primary);
        let click = response.clicked_by(primary);
        let chosen = if drag { pressed_pane } else if click { hovered_pane } else { None };
        if let Some((key,pine,geometry)) = chosen {
            let proj = projection(view,data,geometry,ui.ctx());
            let a = anchor(&proj,if drag { origin.unwrap() } else { pointer.unwrap() },pine,data.bars,magnet);
            let b = anchor(&proj,pointer.unwrap_or(origin.unwrap_or(geometry.plot.center())),pine,data.bars,magnet);
            let draft = Draft { key:key.clone(), tool:frame.tool, a,b,dragging:drag };
            self.selected = None;
            frame.select = Some(Vec::new());
            if single(frame.tool) { commit(draft,frame); } else { self.draft = Some(draft); }
            return true;
        }
        down && pressed_pane.is_some() && response.hovered()
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint(&self, painter: &egui::Painter, data: &FrameData, view: &View, geometry: &PaneGeometry, id: u64, timeframe: Option<Timeframe>, instances: &[PineInstance], frame: &DrawingFrame) {
        let Some(sets) = frame.pane_sets else { return; };
        let Some(key) = key(data,timeframe,instances,id) else { return; };
        let mut drawings = sets.get(&key).to_vec();
        if let Some(movement) = self.movement.as_ref().filter(|m| m.key == key) {
            if let Some(drawing) = drawings.iter_mut().find(|d| d.id() == movement.original.id()) {
                *drawing = movement.preview.clone();
            }
        }
        if let Some(draft) = self.draft.as_ref().filter(|d| d.key == key) {
            if let Some(d) = build(draft,0,frame) { drawings.push(d); }
        }
        if drawings.is_empty() { return; }
        let selected = self.selected.as_ref().filter(|(k,_)| k == &key).map(|(_,id)| *id);
        let local = DrawingFrame::new(&drawings,Tool::Cursor,selected,frame.next_id)
            .with_view(frame.timeframe,frame.hide_all);
        let proj = projection(view,data,geometry,painter.ctx());
        DrawingLayer::default().paint(painter,&proj,&local);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_a_trend_line_changes_value_and_time_but_not_its_id() {
        let a = Anchor { time_ms: 1_000, price: -2.0 };
        let b = Anchor { time_ms: 2_000, price: 8.0 };
        let original = Drawing::build(Tool::TrendLine, 7, &[a,b], Tool::TrendLine.default_style()).unwrap();
        let target = Anchor { time_ms: 1_500, price: 3.0 };
        let translated = moved(&original,a,target,None).unwrap();
        assert_eq!(translated.id(), 7);
        assert_eq!(translated.anchors(), vec![target, Anchor { time_ms: 2_500, price: 13.0 }]);
        let endpoint = moved(&original,a,target,Some(0)).unwrap();
        assert_eq!(endpoint.anchors(), vec![target,b]);
        assert_eq!(original.anchors(), vec![a,b]);
    }

    #[test]
    fn only_value_space_tools_are_enabled() {
        assert!(supported(Tool::TrendLine));
        assert!(supported(Tool::FibRetracement));
        assert!(supported(Tool::HorizontalLine));
        assert!(!supported(Tool::LongPosition));
        assert!(!supported(Tool::AnchoredVwap));
        assert!(!supported(Tool::FixedRangeVolumeProfile));
    }
}
