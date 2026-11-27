//! Calendar-aware time scale, independent of egui and market prices.
//! Natural boundaries outrank ordinary bars. Selection is stable across panning
//! because it uses the entire loaded series, not the visible window's first bar.
//! The cache checks every timestamp (not just first/last), so corrections and
//! inserted historical bars cannot leave a stale calendar.
use crate::model::Kline;
use std::collections::BTreeSet;
use std::ops::Range;

const DAY: i64 = 86_400_000;
const DIVISORS: [i64; 8] = [1000,60_000,300_000,1_800_000,3_600_000,10_800_000,21_600_000,43_200_000];
const DAY_WEIGHT: usize = 9;
const MONTH_WEIGHT: usize = 10;
const YEAR_WEIGHT: usize = 11;

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum MarkKind { Year, Month, Day, Time }

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct Mark { pub index: usize, pub kind: MarkKind }

fn weight(previous: i64,current: i64) -> usize {
    let p = previous.div_euclid(DAY);
    let c = current.div_euclid(DAY);
    if p != c {
        let (py,pm,_) = crate::format::civil_from_days(p);
        let (cy,cm,_) = crate::format::civil_from_days(c);
        if py != cy { return YEAR_WEIGHT; }
        if pm != cm { return MONTH_WEIGHT; }
        return DAY_WEIGHT;
    }
    for (index,divisor) in DIVISORS.iter().enumerate().rev() {
        if previous.div_euclid(*divisor) != current.div_euclid(*divisor) { return index+1; }
    }
    0
}

fn kind(weight: usize) -> MarkKind {
    match weight { YEAR_WEIGHT => MarkKind::Year, MONTH_WEIGHT => MarkKind::Month,
        DAY_WEIGHT => MarkKind::Day, _ => MarkKind::Time }
}

pub fn spacing_in_bars(slot: f32, minimum_points: f32) -> usize {
    if !slot.is_finite() || slot <= 0.0 || !minimum_points.is_finite() { return usize::MAX; }
    (minimum_points.max(1.0)/slot).ceil().max(1.0) as usize
}

#[derive(Default)]
pub struct CalendarMarks {
    timestamps: Vec<i64>,
    groups: [Vec<usize>; 12],
    spacing: Option<usize>,
    selected: Vec<Mark>,
}

impl CalendarMarks {
    fn refresh_times(&mut self, times: impl Iterator<Item=i64> + Clone) {
        if self.timestamps.iter().copied().eq(times.clone()) { return; }
        self.timestamps = times.collect();
        self.groups.iter_mut().for_each(Vec::clear);
        self.spacing = None;
        self.selected.clear();
        for (index,&current) in self.timestamps.iter().enumerate() {
            if index > 0 && current == self.timestamps[index-1] { continue; }
            let previous = if index == 0 {
                let interval = self.timestamps.get(1).map(|next| next.saturating_sub(current))
                    .unwrap_or(DAY).clamp(1,DAY);
                current.saturating_sub(interval)
            } else { self.timestamps[index-1] };
            self.groups[weight(previous,current)].push(index);
        }
    }

    fn rebuild(&mut self, spacing: usize) {
        let spacing = spacing.max(1);
        if self.spacing == Some(spacing) { return; }
        self.spacing = Some(spacing);
        let mut accepted = BTreeSet::<usize>::new();
        let mut marks = Vec::new();
        for (weight,indices) in self.groups.iter().enumerate().rev() {
            for &index in indices {
                let left_clear = accepted.range(..index).next_back()
                    .is_none_or(|&left| index-left >= spacing);
                let right_clear = accepted.range(index..).next()
                    .is_none_or(|&right| right-index >= spacing);
                if left_clear && right_clear {
                    accepted.insert(index);
                    marks.push(Mark { index, kind: kind(weight) });
                }
            }
        }
        marks.sort_unstable_by_key(|mark| mark.index);
        self.selected = marks;
    }

    pub fn select(&mut self,bars: &[Kline],spacing: usize,visible: Range<usize>) -> &[Mark] {
        self.refresh_times(bars.iter().map(|bar| bar.open_time));
        self.rebuild(spacing);
        let start = self.selected.partition_point(|mark| mark.index < visible.start);
        let end = self.selected.partition_point(|mark| mark.index < visible.end).max(start);
        &self.selected[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::parse_timestamp;
    fn ts(text: &str) -> i64 { parse_timestamp(text).unwrap() }

    #[test]
    fn calendar_boundaries_outrank_intraday_and_weekend_gaps() {
        assert_eq!(weight(ts("2025-12-31 21:00"),ts("2026-01-02 14:30")),YEAR_WEIGHT);
        assert_eq!(weight(ts("2026-01-30 21:00"),ts("2026-02-02 14:30")),MONTH_WEIGHT);
        assert_eq!(weight(ts("2026-11-20 21:00"),ts("2026-11-23 14:30")),DAY_WEIGHT);
        assert!(weight(ts("2026-11-23 09:45"),ts("2026-11-23 10:00")) > weight(ts("2026-11-23 10:00"),ts("2026-11-23 10:05")));
    }

    #[test]
    fn major_marks_win_and_spacing_is_respected_even_at_subpixel_zoom() {
        let start = ts("2025-12-20");
        let mut cache = CalendarMarks::default();
        cache.refresh_times((0..60).map(|i| start+i*DAY));
        let spacing = spacing_in_bars(0.5,5.0);
        cache.rebuild(spacing);
        assert!(cache.selected.contains(&Mark { index:12,kind:MarkKind::Year }));
        assert!(cache.selected.contains(&Mark { index:43,kind:MarkKind::Month }));
        assert!(cache.selected.windows(2).all(|pair| pair[1].index-pair[0].index >= spacing));
    }

    #[test]
    fn corrected_middle_timestamp_invalidates_cache_and_duplicates_have_one_label() {
        let start = ts("2026-01-01");
        let mut cache = CalendarMarks::default();
        cache.refresh_times([start,start+DAY,start+2*DAY].into_iter());
        cache.rebuild(1);
        assert_eq!(cache.selected.len(),3);
        cache.refresh_times([start,start,start+2*DAY].into_iter());
        cache.rebuild(1);
        assert_eq!(cache.selected.len(),2);
        assert!(cache.selected.iter().all(|mark| mark.index != 1));
    }
}
