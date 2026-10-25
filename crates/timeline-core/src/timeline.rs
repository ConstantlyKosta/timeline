//! Timeline zoom maths, clustering and lane layout. Pure functions, no I/O, no platform types.
//!
//! The UI keeps a line-for-line TypeScript port of this module (`ui/src/lib/timeline.ts`) so zoom
//! gestures can be handled per frame without crossing the IPC boundary. Both implementations are
//! checked against the same golden fixture (`fixtures/timeline_golden.json`), see the tests below.

use crate::models::{EntryKind, Ms, TimelineEntry, MS_PER_DAY};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoomLevel {
    Month,
    Week,
    Day,
}

pub const MIN_POINTS_PER_DAY: f64 = 12.0;
pub const MAX_POINTS_PER_DAY: f64 = 2400.0;

impl ZoomLevel {
    /// Canonical scale for the level (points per calendar day).
    pub fn points_per_day(self) -> f64 {
        match self {
            ZoomLevel::Month => 36.0,
            ZoomLevel::Week => 160.0,
            ZoomLevel::Day => 1200.0,
        }
    }
    pub fn zoomed_in(self) -> Option<ZoomLevel> {
        match self {
            ZoomLevel::Month => Some(ZoomLevel::Week),
            ZoomLevel::Week => Some(ZoomLevel::Day),
            ZoomLevel::Day => None,
        }
    }
    pub fn zoomed_out(self) -> Option<ZoomLevel> {
        match self {
            ZoomLevel::Day => Some(ZoomLevel::Week),
            ZoomLevel::Week => Some(ZoomLevel::Month),
            ZoomLevel::Month => None,
        }
    }
    /// Which level a continuous scale belongs to. Boundaries are the geometric means of the
    /// neighbouring canonical scales, so thresholds feel symmetric under pinch zooming.
    pub fn for_points_per_day(ppd: f64) -> ZoomLevel {
        let month_week = (ZoomLevel::Month.points_per_day() * ZoomLevel::Week.points_per_day()).sqrt();
        let week_day = (ZoomLevel::Week.points_per_day() * ZoomLevel::Day.points_per_day()).sqrt();
        if ppd < month_week {
            ZoomLevel::Month
        } else if ppd < week_day {
            ZoomLevel::Week
        } else {
            ZoomLevel::Day
        }
    }
    pub fn clamp(ppd: f64) -> f64 {
        ppd.max(MIN_POINTS_PER_DAY).min(MAX_POINTS_PER_DAY)
    }
}

/// Maps instants to horizontal positions on the single continuous timeline surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeMapping {
    /// Instant at x = 0.
    pub origin: Ms,
    pub points_per_day: f64,
}

impl TimeMapping {
    pub fn new(origin: Ms, points_per_day: f64) -> Self {
        Self { origin, points_per_day: ZoomLevel::clamp(points_per_day) }
    }
    pub fn points_per_ms(&self) -> f64 {
        self.points_per_day / MS_PER_DAY
    }
    pub fn level(&self) -> ZoomLevel {
        ZoomLevel::for_points_per_day(self.points_per_day)
    }
    pub fn x(&self, t: Ms) -> f64 {
        (t - self.origin) as f64 * self.points_per_ms()
    }
    pub fn time_at(&self, x: f64) -> Ms {
        self.origin + (x / self.points_per_ms()).round() as Ms
    }
    /// Width in points of a span given in seconds.
    pub fn width_for_seconds(&self, seconds: f64) -> f64 {
        seconds * 1000.0 * self.points_per_ms()
    }
    pub fn with_points_per_day(&self, ppd: f64) -> Self {
        Self::new(self.origin, ppd)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoomResult {
    pub mapping: TimeMapping,
    /// New horizontal scroll offset (content x of the viewport's left edge).
    pub scroll_offset: f64,
}

pub struct ZoomMath;

impl ZoomMath {
    /// Zoom by `factor` (>1 zooms in) keeping the instant under `anchor_viewport_x` fixed on screen.
    pub fn zoom(m: &TimeMapping, factor: f64, anchor_viewport_x: f64, scroll_offset: f64) -> ZoomResult {
        let nm = m.with_points_per_day(m.points_per_day * factor);
        // Keep sub-millisecond precision: work from the exact anchor position (ms since origin).
        let anchor_exact = (scroll_offset + anchor_viewport_x) / m.points_per_ms();
        let new_x = anchor_exact * nm.points_per_ms();
        ZoomResult { mapping: nm, scroll_offset: new_x - anchor_viewport_x }
    }

    /// Jump to a canonical level keeping the viewport centre fixed.
    pub fn snap(m: &TimeMapping, level: ZoomLevel, viewport_width: f64, scroll_offset: f64) -> ZoomResult {
        let factor = level.points_per_day() / m.points_per_day;
        Self::zoom(m, factor, viewport_width / 2.0, scroll_offset)
    }

    /// Zoom in one level centred on `t` (used when opening a cluster).
    pub fn zoom_in(m: &TimeMapping, t: Ms, viewport_width: f64) -> ZoomResult {
        let target = m.level().zoomed_in().unwrap_or(m.level()).points_per_day();
        let nm = m.with_points_per_day(target);
        ZoomResult { mapping: nm, scroll_offset: nm.x(t) - viewport_width / 2.0 }
    }
}

// ---------------------------------------------------------------- clustering

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryCluster {
    /// Id of the first entry.
    pub id: String,
    pub entry_ids: Vec<String>,
    pub start: Ms,
    pub end: Ms,
    /// Theme label.
    pub title: String,
    pub recording_count: usize,
    pub note_count: usize,
    pub media_count: usize,
}

impl EntryCluster {
    pub fn count(&self) -> usize {
        self.entry_ids.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterConfig {
    /// Entries separated by more than this gap (ms) start a new cluster.
    pub max_gap: f64,
    /// A cluster never spans more than this (ms).
    pub max_span: f64,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self { max_gap: 2.0 * MS_PER_DAY, max_span: 14.0 * MS_PER_DAY }
    }
}

fn sorted_entries(entries: &[TimelineEntry]) -> Vec<&TimelineEntry> {
    let mut v: Vec<&TimelineEntry> = entries.iter().collect();
    v.sort_by(|a, b| (a.start, &a.id).cmp(&(b.start, &b.id)));
    v
}

/// Groups entries by temporal proximity and labels each group with its dominant tag (or the
/// first entry's title). Deterministic: input order does not matter.
pub fn cluster(entries: &[TimelineEntry], cfg: ClusterConfig) -> Vec<EntryCluster> {
    let sorted = sorted_entries(entries);
    let mut groups: Vec<Vec<&TimelineEntry>> = Vec::new();
    let mut current: Vec<&TimelineEntry> = Vec::new();
    let mut current_end = i64::MIN;
    for e in sorted {
        if let Some(first) = current.first() {
            let gap = (e.start - current_end) as f64;
            let span = (e.end() - first.start) as f64;
            if gap > cfg.max_gap || span > cfg.max_span {
                groups.push(std::mem::take(&mut current));
            }
        }
        current.push(e);
        current_end = current.iter().map(|x| x.end()).max().unwrap_or(e.end()).max(e.end());
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups.iter().map(|g| make_cluster(g)).collect()
}

fn make_cluster(g: &[&TimelineEntry]) -> EntryCluster {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in g {
        let mut seen: Vec<&str> = Vec::new();
        for t in &e.tags {
            if !seen.contains(&t.as_str()) {
                seen.push(t);
                *counts.entry(t.as_str()).or_default() += 1;
            }
        }
    }
    // Most common tag; ties -> alphabetical (BTreeMap iterates in key order, max_by keeps the last max,
    // so iterate reversed to keep the alphabetically first).
    let top = counts.iter().rev().max_by_key(|(_, c)| **c).map(|(k, _)| *k);
    let title = match top {
        Some(t) => format!("#{t}"),
        None => g.first().map(|e| e.title.clone()).unwrap_or_else(|| "Untitled".into()),
    };
    let kinds = |f: fn(EntryKind) -> bool| g.iter().filter(|e| f(e.kind)).count();
    EntryCluster {
        id: g[0].id.clone(),
        entry_ids: g.iter().map(|e| e.id.clone()).collect(),
        start: g.iter().map(|e| e.start).min().unwrap(),
        end: g.iter().map(|e| e.end()).max().unwrap(),
        title,
        recording_count: kinds(|k| k == EntryKind::Recording),
        note_count: kinds(|k| k == EntryKind::Note),
        media_count: kinds(|k| k == EntryKind::Photo || k == EntryKind::Video),
    }
}

// -------------------------------------------------------------------- layout

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LayoutRect {
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
    pub fn intersects(&self, o: &LayoutRect) -> bool {
        self.x < o.max_x() && o.x < self.max_x() && self.y < o.max_y() && o.y < self.max_y()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedItem {
    pub id: String,
    pub kind: EntryKind,
    pub frame: LayoutRect,
    pub lane: usize,
    pub title: String,
    pub snippet: String,
    pub shows_snippet: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacedCluster {
    pub id: String,
    pub frame: LayoutRect,
    pub lane: usize,
    pub title: String,
    pub count: usize,
    pub recording_count: usize,
    pub note_count: usize,
    pub media_count: usize,
    pub entry_ids: Vec<String>,
    pub start: Ms,
    pub end: Ms,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutResult {
    pub level: ZoomLevel,
    /// Week + Day.
    pub items: Vec<PlacedItem>,
    /// Month.
    pub clusters: Vec<PlacedCluster>,
    pub lane_count: usize,
    pub content_height: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutConfig {
    pub lane_gap: f64,
    pub horizontal_gap: f64,
    pub top_inset: f64,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self { lane_gap: 8.0, horizontal_gap: 6.0, top_inset: 12.0 }
    }
}

/// Block heights per level.
pub fn item_height(level: ZoomLevel) -> f64 {
    match level {
        ZoomLevel::Month => 40.0,
        ZoomLevel::Week => 52.0,
        ZoomLevel::Day => 76.0,
    }
}

/// Width of an entry block at the given mapping.
///
/// Week recordings are "duration-sized": `40 + 18·√minutes`, clamped to 40…480pt (pure
/// time-proportional width would make a 3-minute memo invisible at 160pt/day). Day uses real
/// time-proportional widths with a readable minimum.
pub fn item_width(e: &TimelineEntry, m: &TimeMapping) -> f64 {
    match m.level() {
        ZoomLevel::Month => 40.0,
        ZoomLevel::Week => match e.kind {
            EntryKind::Recording | EntryKind::Video => {
                let minutes = e.duration.max(0.0) / 60.0;
                (40.0 + 18.0 * minutes.sqrt()).max(40.0).min(480.0)
            }
            EntryKind::Note => 120.0,
            EntryKind::Photo => 56.0,
        },
        ZoomLevel::Day => {
            let prop = m.width_for_seconds(e.duration);
            match e.kind {
                EntryKind::Recording | EntryKind::Video => prop.max(180.0),
                EntryKind::Note => 200.0,
                EntryKind::Photo => 96.0,
            }
        }
    }
}

/// Greedy interval-graph colouring: rects must be sorted by x. Returns lane per rect and lane count.
/// Guarantees no two rects in the same lane overlap (including `horizontal_gap`).
pub fn assign_lanes(rects: &[(f64, f64)], horizontal_gap: f64) -> (Vec<usize>, usize) {
    let mut lane_ends: Vec<f64> = Vec::new();
    let mut result = vec![0usize; rects.len()];
    for (i, (x, w)) in rects.iter().enumerate() {
        if let Some(lane) = lane_ends.iter().position(|end| end + horizontal_gap <= *x) {
            result[i] = lane;
            lane_ends[lane] = x + w;
        } else {
            result[i] = lane_ends.len();
            lane_ends.push(x + w);
        }
    }
    (result, lane_ends.len())
}

pub fn layout(entries: &[TimelineEntry], m: &TimeMapping, cfg: LayoutConfig, ccfg: ClusterConfig) -> LayoutResult {
    let level = m.level();
    let h = item_height(level);
    let y_of = |lane: usize| cfg.top_inset + lane as f64 * (h + cfg.lane_gap);

    if level == ZoomLevel::Month {
        let clusters = cluster(entries, ccfg);
        let rects: Vec<(f64, f64)> = clusters
            .iter()
            .map(|c| {
                let x0 = m.x(c.start);
                (x0, (m.x(c.end) - x0).max(72.0))
            })
            .collect();
        let (lanes, lane_count) = assign_lanes(&rects, cfg.horizontal_gap);
        let placed = clusters
            .iter()
            .enumerate()
            .map(|(i, c)| PlacedCluster {
                id: c.id.clone(),
                frame: LayoutRect { x: rects[i].0, y: y_of(lanes[i]), width: rects[i].1, height: h },
                lane: lanes[i],
                title: c.title.clone(),
                count: c.count(),
                recording_count: c.recording_count,
                note_count: c.note_count,
                media_count: c.media_count,
                entry_ids: c.entry_ids.clone(),
                start: c.start,
                end: c.end,
            })
            .collect();
        return LayoutResult {
            level,
            items: vec![],
            clusters: placed,
            lane_count,
            content_height: y_of(lane_count) + cfg.top_inset,
        };
    }

    let sorted = sorted_entries(entries);
    let rects: Vec<(f64, f64)> = sorted.iter().map(|e| (m.x(e.start), item_width(e, m))).collect();
    let (lanes, lane_count) = assign_lanes(&rects, cfg.horizontal_gap);
    let items = sorted
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let shows = level == ZoomLevel::Day && !e.snippet.is_empty();
            PlacedItem {
                id: e.id.clone(),
                kind: e.kind,
                frame: LayoutRect { x: rects[i].0, y: y_of(lanes[i]), width: rects[i].1, height: h },
                lane: lanes[i],
                title: e.title.clone(),
                snippet: if shows { e.snippet.clone() } else { String::new() },
                shows_snippet: shows,
            }
        })
        .collect();
    LayoutResult { level, items, clusters: vec![], lane_count, content_height: y_of(lane_count) + cfg.top_inset }
}

/// Instants covering all entries with padding. Falls back to [now-14d, now+3d] when empty.
pub fn extent(entries: &[TimelineEntry], padding_ms: i64, now: Ms) -> (Ms, Ms) {
    let first = entries.iter().map(|e| e.start).min();
    let last = entries.iter().map(|e| e.end()).max();
    match (first, last) {
        (Some(f), Some(l)) => (f - padding_ms, l.max(now) + padding_ms),
        _ => (now - 14 * 86_400_000, now + 3 * 86_400_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;
    const T0: Ms = 1_760_000_000_000; // 2025-10-09

    fn entry(id: &str, kind: EntryKind, start: Ms, duration: f64, tags: &[&str]) -> TimelineEntry {
        TimelineEntry {
            id: id.into(),
            kind,
            start,
            duration,
            title: format!("T-{id}"),
            snippet: format!("snippet {id}"),
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn level_boundaries_are_geometric_means() {
        assert_eq!(ZoomLevel::for_points_per_day(36.0), ZoomLevel::Month);
        assert_eq!(ZoomLevel::for_points_per_day(160.0), ZoomLevel::Week);
        assert_eq!(ZoomLevel::for_points_per_day(1200.0), ZoomLevel::Day);
        let mw = (36.0f64 * 160.0).sqrt();
        assert_eq!(ZoomLevel::for_points_per_day(mw - 0.01), ZoomLevel::Month);
        assert_eq!(ZoomLevel::for_points_per_day(mw), ZoomLevel::Week);
        let wd = (160.0f64 * 1200.0).sqrt();
        assert_eq!(ZoomLevel::for_points_per_day(wd - 0.01), ZoomLevel::Week);
        assert_eq!(ZoomLevel::for_points_per_day(wd), ZoomLevel::Day);
    }

    #[test]
    fn clamp_bounds() {
        assert_eq!(ZoomLevel::clamp(1.0), MIN_POINTS_PER_DAY);
        assert_eq!(ZoomLevel::clamp(1e9), MAX_POINTS_PER_DAY);
        assert_eq!(ZoomLevel::clamp(100.0), 100.0);
        assert_eq!(TimeMapping::new(0, 1.0).points_per_day, MIN_POINTS_PER_DAY);
    }

    #[test]
    fn mapping_round_trips() {
        let m = TimeMapping::new(T0, 160.0);
        assert!((m.x(T0 + DAY) - 160.0).abs() < 1e-9);
        assert_eq!(m.time_at(m.x(T0 + 12345678)), T0 + 12345678);
        assert!((m.width_for_seconds(86_400.0) - 160.0).abs() < 1e-9);
        assert!(m.x(T0 - DAY) < 0.0);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let m = TimeMapping::new(T0, 160.0);
        for (factor, anchor, offset) in [(1.5, 300.0, 1000.0), (0.5, 0.0, 250.0), (3.0, 777.0, 0.0), (0.1, 500.0, 5000.0)] {
            let before = m.time_at(offset + anchor);
            let r = ZoomMath::zoom(&m, factor, anchor, offset);
            let after = r.mapping.time_at(r.scroll_offset + anchor);
            assert!((before - after).abs() <= 1, "anchor drifted for factor {factor}: {before} vs {after}");
        }
    }

    #[test]
    fn zoom_clamps_scale_but_keeps_anchor() {
        let m = TimeMapping::new(T0, 2000.0);
        let r = ZoomMath::zoom(&m, 10.0, 400.0, 1234.0);
        assert_eq!(r.mapping.points_per_day, MAX_POINTS_PER_DAY);
        let before = m.time_at(1234.0 + 400.0);
        let after = r.mapping.time_at(r.scroll_offset + 400.0);
        assert!((before - after).abs() <= 1);
    }

    #[test]
    fn snap_to_levels() {
        let m = TimeMapping::new(T0, 160.0);
        for level in [ZoomLevel::Month, ZoomLevel::Week, ZoomLevel::Day] {
            let r = ZoomMath::snap(&m, level, 1000.0, 400.0);
            assert_eq!(r.mapping.level(), level);
            assert!((r.mapping.points_per_day - level.points_per_day()).abs() < 1e-9);
            // centre stays put
            assert!((m.time_at(400.0 + 500.0) - r.mapping.time_at(r.scroll_offset + 500.0)).abs() <= 1);
        }
    }

    #[test]
    fn zoom_in_centres_on_date() {
        let m = TimeMapping::new(T0, 36.0);
        let target = T0 + 10 * DAY;
        let r = ZoomMath::zoom_in(&m, target, 1000.0);
        assert_eq!(r.mapping.level(), ZoomLevel::Week);
        let centre = r.mapping.time_at(r.scroll_offset + 500.0);
        assert!((centre - target).abs() <= 1);
        // At Day zoom there is nothing deeper: stays Day.
        let d = TimeMapping::new(T0, 1200.0);
        assert_eq!(ZoomMath::zoom_in(&d, target, 1000.0).mapping.level(), ZoomLevel::Day);
    }

    #[test]
    fn clusters_split_on_gap_and_span() {
        let es = vec![
            entry("a", EntryKind::Note, T0, 0.0, &["trip"]),
            entry("b", EntryKind::Recording, T0 + DAY, 600.0, &["trip", "family"]),
            entry("c", EntryKind::Photo, T0 + 5 * DAY, 0.0, &[]), // gap > 2d -> new cluster
        ];
        let cs = cluster(&es, ClusterConfig::default());
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].entry_ids, vec!["a", "b"]);
        assert_eq!(cs[0].title, "#trip");
        assert_eq!(cs[0].recording_count, 1);
        assert_eq!(cs[0].note_count, 1);
        assert_eq!(cs[1].title, "T-c");
        assert_eq!(cs[1].media_count, 1);
        assert_eq!(cs[0].end, T0 + DAY + 600_000);
    }

    #[test]
    fn clusters_respect_max_span() {
        // One note a day for 40 days: no gap ever exceeds 2 days, but spans are capped at 14.
        let es: Vec<_> = (0..40).map(|i| entry(&format!("{i:02}"), EntryKind::Note, T0 + i * DAY, 0.0, &[])).collect();
        let cs = cluster(&es, ClusterConfig::default());
        assert!(cs.len() >= 3);
        for c in &cs {
            assert!((c.end - c.start) as f64 <= 14.0 * MS_PER_DAY);
        }
        assert_eq!(cs.iter().map(|c| c.count()).sum::<usize>(), 40);
    }

    #[test]
    fn clustering_is_order_independent_and_tie_breaks_tags_alphabetically() {
        let a = entry("a", EntryKind::Note, T0, 0.0, &["zeta", "alpha"]);
        let b = entry("b", EntryKind::Note, T0 + 1000, 0.0, &["alpha", "zeta"]);
        let fwd = cluster(&[a.clone(), b.clone()], ClusterConfig::default());
        let rev = cluster(&[b, a], ClusterConfig::default());
        assert_eq!(fwd, rev);
        assert_eq!(fwd[0].title, "#alpha");
    }

    #[test]
    fn long_recording_extends_cluster_end() {
        let es = vec![
            entry("a", EntryKind::Recording, T0, 3.0 * 86_400.0, &[]),
            entry("b", EntryKind::Note, T0 + 4 * DAY, 0.0, &[]), // gap measured from recording end (3d) = 1d
        ];
        assert_eq!(cluster(&es, ClusterConfig::default()).len(), 1);
    }

    #[test]
    fn week_widths_scale_with_duration_and_clamp() {
        let m = TimeMapping::new(T0, 160.0);
        let short = item_width(&entry("a", EntryKind::Recording, T0, 60.0, &[]), &m);
        let long = item_width(&entry("b", EntryKind::Recording, T0, 3600.0, &[]), &m);
        let huge = item_width(&entry("c", EntryKind::Recording, T0, 10.0 * 3600.0, &[]), &m);
        assert!((short - 58.0).abs() < 1e-9);
        assert!(long > short);
        assert_eq!(huge, 480.0);
        assert_eq!(item_width(&entry("n", EntryKind::Note, T0, 0.0, &[]), &m), 120.0);
        assert_eq!(item_width(&entry("p", EntryKind::Photo, T0, 0.0, &[]), &m), 56.0);
    }

    #[test]
    fn day_widths_are_time_proportional_with_minimum() {
        let m = TimeMapping::new(T0, 1200.0);
        // 1 h at 50 pt/h is below the 180 pt readable minimum.
        assert_eq!(item_width(&entry("a", EntryKind::Recording, T0, 3600.0, &[]), &m), 180.0);
        assert_eq!(item_width(&entry("s", EntryKind::Recording, T0, 10.0, &[]), &m), 180.0);
        assert_eq!(item_width(&entry("h", EntryKind::Recording, T0, 7200.0, &[]), &m), 180.0);
        let five = item_width(&entry("f", EntryKind::Recording, T0, 5.0 * 3600.0, &[]), &m);
        assert!((five - 250.0).abs() < 1e-9);
    }

    #[test]
    fn lanes_never_overlap_within_a_lane() {
        let rects: Vec<(f64, f64)> = (0..200).map(|i| ((i * 37 % 500) as f64, 20.0 + (i * 13 % 90) as f64)).collect();
        let mut sorted = rects.clone();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let (lanes, count) = assign_lanes(&sorted, 6.0);
        assert!(count >= 1);
        for i in 0..sorted.len() {
            for j in (i + 1)..sorted.len() {
                if lanes[i] == lanes[j] {
                    assert!(sorted[i].0 + sorted[i].1 + 6.0 <= sorted[j].0 + 1e-9, "overlap in lane {}", lanes[i]);
                }
            }
        }
    }

    #[test]
    fn month_layout_uses_clusters_week_day_use_items() {
        let es = vec![
            entry("a", EntryKind::Note, T0, 0.0, &["x"]),
            entry("b", EntryKind::Recording, T0 + 3600_000, 1800.0, &["x"]),
            entry("c", EntryKind::Note, T0 + 30 * DAY, 0.0, &[]),
        ];
        let origin = T0 - 3 * DAY;
        let month = layout(&es, &TimeMapping::new(origin, 36.0), LayoutConfig::default(), ClusterConfig::default());
        assert_eq!(month.level, ZoomLevel::Month);
        assert!(month.items.is_empty());
        assert_eq!(month.clusters.len(), 2);
        assert!(month.clusters[0].frame.width >= 72.0);
        let week = layout(&es, &TimeMapping::new(origin, 160.0), LayoutConfig::default(), ClusterConfig::default());
        assert_eq!(week.level, ZoomLevel::Week);
        assert!(week.clusters.is_empty());
        assert_eq!(week.items.len(), 3);
        assert!(week.items.iter().all(|i| !i.shows_snippet && i.snippet.is_empty()));
        let day = layout(&es, &TimeMapping::new(origin, 1200.0), LayoutConfig::default(), ClusterConfig::default());
        assert!(day.items.iter().all(|i| i.shows_snippet));
        assert_eq!(day.items[0].frame.height, 76.0);
    }

    #[test]
    fn layout_frames_in_lane_do_not_intersect() {
        let es: Vec<_> = (0..60)
            .map(|i| entry(&format!("{i:03}"), if i % 3 == 0 { EntryKind::Recording } else { EntryKind::Note }, T0 + i * 3_600_000, 900.0, &[]))
            .collect();
        let r = layout(&es, &TimeMapping::new(T0 - DAY, 160.0), LayoutConfig::default(), ClusterConfig::default());
        for (i, a) in r.items.iter().enumerate() {
            for b in r.items.iter().skip(i + 1) {
                assert!(!a.frame.intersects(&b.frame), "{} overlaps {}", a.id, b.id);
            }
        }
    }

    #[test]
    fn extent_pads_and_handles_empty() {
        let now = T0 + 100 * DAY;
        let (s, e) = extent(&[], 3 * DAY, now);
        assert_eq!(s, now - 14 * DAY);
        assert_eq!(e, now + 3 * DAY);
        let es = vec![entry("a", EntryKind::Note, T0, 0.0, &[])];
        let (s, e) = extent(&es, 3 * DAY, now);
        assert_eq!(s, T0 - 3 * DAY);
        assert_eq!(e, now + 3 * DAY);
    }

    /// Golden fixture shared with the TypeScript port: Rust regenerates it when
    /// `UPDATE_GOLDEN=1`, otherwise it must match byte-for-byte after JSON normalisation.
    #[test]
    fn golden_fixture_matches() {
        let es = vec![
            entry("a", EntryKind::Note, T0, 0.0, &["trip"]),
            entry("b", EntryKind::Recording, T0 + 3_600_000, 1800.0, &["trip"]),
            entry("c", EntryKind::Recording, T0 + 2 * DAY + 5_000_000, 5400.0, &["work"]),
            entry("d", EntryKind::Photo, T0 + 2 * DAY + 6_000_000, 0.0, &[]),
            entry("e", EntryKind::Note, T0 + 9 * DAY, 0.0, &["work", "health"]),
            entry("f", EntryKind::Video, T0 + 9 * DAY + 1000, 120.0, &["health"]),
            entry("g", EntryKind::Note, T0 + 40 * DAY, 0.0, &[]),
        ];
        let origin = T0 - 3 * DAY;
        let mut cases = Vec::new();
        for ppd in [12.0, 36.0, 80.0, 160.0, 400.0, 1200.0, 2400.0] {
            let m = TimeMapping::new(origin, ppd);
            let l = layout(&es, &m, LayoutConfig::default(), ClusterConfig::default());
            cases.push(serde_json::json!({ "pointsPerDay": ppd, "layout": l }));
        }
        let zooms: Vec<_> = [(1.5, 300.0, 1000.0), (0.5, 0.0, 250.0), (3.0, 777.0, 0.0), (0.1, 500.0, 5000.0), (50.0, 100.0, 100.0)]
            .iter()
            .map(|(f, a, o)| {
                let r = ZoomMath::zoom(&TimeMapping::new(origin, 160.0), *f, *a, *o);
                serde_json::json!({ "factor": f, "anchor": a, "offset": o, "result": r })
            })
            .collect();
        let snaps: Vec<_> = [ZoomLevel::Month, ZoomLevel::Week, ZoomLevel::Day]
            .iter()
            .map(|lv| serde_json::json!({ "level": lv, "result": ZoomMath::snap(&TimeMapping::new(origin, 160.0), *lv, 1000.0, 400.0) }))
            .collect();
        let zoom_in = ZoomMath::zoom_in(&TimeMapping::new(origin, 36.0), T0 + 9 * DAY, 1000.0);
        let doc = serde_json::json!({
            "entries": es, "origin": origin, "layouts": cases, "zooms": zooms, "snaps": snaps,
            "zoomIn": { "target": T0 + 9 * DAY, "viewportWidth": 1000.0, "result": zoom_in },
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/timeline_golden.json");
        if std::env::var("UPDATE_GOLDEN").is_ok() || !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        }
        let on_disk: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk, doc, "golden fixture out of date; run with UPDATE_GOLDEN=1 and update the TS port");
    }
}
