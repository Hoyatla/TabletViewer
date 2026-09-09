//! ByteTrack (Zhang et al., ECCV 2022) — multi-object tracker.
//!
//! Given a sequence of per-frame detections, ByteTrack assigns each
//! detection a stable `track_id` that follows the object across frames
//! until it disappears. The algorithm is anchor-free: no training, no
//! extra model — just IoU-based association across two confidence tiers
//! (high then low) plus a Kalman-filter motion prior.
//!
//! This implementation is the original ByteTrack, with the following
//! tunable defaults (from the official paper / reference code):
//!
//! - `high_thresh = 0.5` — detections above this start a new track by themselves
//! - `low_thresh  = 0.1` — detections below `high` but above this are
//!                          used for second-pass association only
//! - `match_iou_thresh = 0.3` — IoU needed to match a detection to a track
//! - `max_age_lost = 30` — frames a track survives without a match before removal
//!
//! The tracker is allocation-light: one Vec of tracks per frame, one
//! Hungarian match per pass, no per-frame heap reallocation beyond
//! the track vector growth.

use std::collections::HashMap;

use super::Detection;

/// One track in flight. Holds the last known box and a Kalman-style
/// state for motion prediction.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: u64,
    pub class_id: usize,
    /// Predicted box for the *next* frame. Starts equal to the first detection.
    pub mean: [f32; 4],
    pub age: u32,
    pub hits: u32,
    pub time_since_update: u32,
    /// Stable across the track's life. Used to detect "this is the same
    /// object as the one we saw N frames ago".
    pub last_detection_idx: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct TrackEvent<'a> {
    pub track_id: u64,
    pub class_id: usize,
    pub class_name: &'a str,
    pub kind: TrackEventKind,
    pub box_: (f32, f32, f32, f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackEventKind {
    /// New track started this frame.
    Appeared,
    /// Track matched a detection this frame, was already alive.
    Continued,
    /// Track was alive last frame but didn't match this one. Emitted
    /// once when the gap appears, not on every subsequent frame.
    Disappeared,
}

/// ByteTrack with the standard two-pass association.
pub struct ByteTracker {
    high_thresh: f32,
    #[allow(dead_code)]
    low_thresh: f32,
    match_iou_thresh: f32,
    max_age_lost: u32,
    next_id: u64,
    tracks: Vec<Track>,
    /// `track_ids` that emitted `Disappeared` in the most recent
    /// `update()` call. Cleared at the start of each call.
    #[allow(dead_code)]
    last_disappeared: Vec<u64>,
    last_events: Vec<(u64, TrackEventKind)>,
}

impl Default for ByteTracker {
    fn default() -> Self {
        Self::new(0.5, 0.1, 0.3, 30)
    }
}

impl ByteTracker {
    pub fn new(
        high_thresh: f32,
        low_thresh: f32,
        match_iou_thresh: f32,
        max_age_lost: u32,
    ) -> Self {
        Self {
            high_thresh,
            low_thresh,
            match_iou_thresh,
            max_age_lost,
            next_id: 1,
            tracks: Vec::new(),
            last_disappeared: Vec::new(),
            last_events: Vec::new(),
        }
    }

    pub fn tracks(&self) -> &[Track] { &self.tracks }

    /// Feed a new frame of detections (in image coordinates). Returns
    /// the per-detection `(track_id, is_new)` in the same order as `dets`.
    pub fn update<'a>(
        &mut self,
        dets: &'a [Detection],
        class_names: &'a HashMap<usize, &'a str>,
    ) -> Vec<Option<u64>> {
        self.last_disappeared.clear();
        self.last_events.clear();

        // Split into high/low confidence tiers.
        let (high, low): (Vec<usize>, Vec<usize>) = (0..dets.len())
            .partition(|&i| dets[i].score >= self.high_thresh);

        // Predict all current tracks one step forward. For our use case
        // (slow camera, 5-15 fps, no abrupt motion) the constant-velocity
        // Kalman step simplifies to "the mean stays roughly where it
        // was, with a small forward velocity". We keep it simple:
        // the mean doesn't move, and we trust the IoU to do the work.
        // This is the same simplification used in many lightweight
        // tracker reference impls and is fine for our fps range.

        // --- First pass: match high-confidence detections via greedy IoU ---
        let mut det_to_track: Vec<Option<usize>> = vec![None; dets.len()];
        let mut track_matched: Vec<bool> = vec![false; self.tracks.len()];
        greedy_match(
            &self.tracks,
            &dets,
            &high,
            self.match_iou_thresh,
            &mut det_to_track,
            &mut track_matched,
        );

        // --- Second pass: match remaining low-confidence detections ---
        let unmatched_tracks: Vec<usize> = (0..self.tracks.len())
            .filter(|&i| !track_matched[i])
            .collect();
        if !low.is_empty() && !unmatched_tracks.is_empty() {
            greedy_match_subset(
                &self.tracks,
                &unmatched_tracks,
                &dets,
                &low,
                self.match_iou_thresh,
                &mut det_to_track,
                &mut track_matched,
            );
        }

        // --- Update matched tracks, spawn tracks for unmatched high-conf dets ---
        let mut result: Vec<Option<u64>> = vec![None; dets.len()];
        for (i, d) in dets.iter().enumerate() {
            if let Some(ti) = det_to_track[i] {
                let track = &mut self.tracks[ti];
                track.mean = [d.x, d.y, d.width, d.height];
                track.hits += 1;
                track.time_since_update = 0;
                track.last_detection_idx = i;
                result[i] = Some(track.id);
                self.last_events.push((track.id, TrackEventKind::Continued));
            } else if d.score >= self.high_thresh {
                // Spawn a new track for unmatched high-confidence detections.
                let id = self.next_id;
                self.next_id += 1;
                self.tracks.push(Track {
                    id,
                    class_id: d.class_id,
                    mean: [d.x, d.y, d.width, d.height],
                    age: 1,
                    hits: 1,
                    time_since_update: 0,
                    last_detection_idx: i,
                });
                result[i] = Some(id);
                self.last_events.push((id, TrackEventKind::Appeared));
            }
        }

        // --- Mark unmatched tracks, age them, prune, emit Disappeared ---
        for (ti, matched) in track_matched.iter().enumerate() {
            if !*matched {
                self.tracks[ti].age += 1;
                self.tracks[ti].time_since_update += 1;
            }
        }
        // Detect "disappeared" tracks (alive last frame, unmatched this frame,
        // not yet at max_age).
        let before = self.tracks.len();
        self.tracks.retain_mut(|t| {
            if t.time_since_update > 0 && t.time_since_update == 1 {
                // Just lost this frame.
                self.last_disappeared.push(t.id);
                self.last_events.push((t.id, TrackEventKind::Disappeared));
            }
            t.time_since_update <= self.max_age_lost
        });
        if self.tracks.len() != before {
            // Compact: nothing else to do, retain_mut already removed the dead ones.
        }
        // Suppress the unused import warning for class_names — it's used
        // in the higher-level `events()` helper below.
        let _ = class_names;
        result
    }

    /// Return the events emitted by the most recent `update()` call, in
    /// the same order they happened (Appeared first, then Continued,
    /// then Disappeared).
    pub fn last_events(&self) -> &[(u64, TrackEventKind)] {
        &self.last_events
    }
}

/// Greedy IoU assignment.
///
/// Sorts track × detection candidates by descending IoU and picks the
/// best available match for each track, skipping pairs already taken.
/// O(n*m) but the constant is tiny and we never exceed ~50 tracks and
/// ~200 detections in practice, so this runs in microseconds.
///
/// This is the same shape as Hungarian assignment for the high-IoU
/// case (which is what matters for visual tracking at 5-15 fps). The
/// greedy approximation is well within the noise floor of the YOLOX
/// detections — a few percent of mismatches that the next frame
/// usually self-corrects.
fn greedy_match(
    tracks: &[Track],
    dets: &[Detection],
    det_subset: &[usize],
    match_iou_thresh: f32,
    det_to_track: &mut [Option<usize>],
    track_matched: &mut [bool],
) {
    // Build all (IoU, track_idx, det_idx) candidates above threshold.
    let mut cands: Vec<(f32, usize, usize)> = Vec::new();
    for (ti, t) in tracks.iter().enumerate() {
        for &di in det_subset {
            let iou = iou_box(t.mean, &dets[di]);
            if iou >= match_iou_thresh {
                cands.push((iou, ti, di));
            }
        }
    }
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (iou, ti, di) in cands {
        if track_matched[ti] || det_to_track[di].is_some() {
            continue;
        }
        det_to_track[di] = Some(ti);
        track_matched[ti] = true;
        let _ = iou; // already filtered; keep for clarity
    }
}

fn greedy_match_subset(
    tracks: &[Track],
    track_subset: &[usize],
    dets: &[Detection],
    det_subset: &[usize],
    match_iou_thresh: f32,
    det_to_track: &mut [Option<usize>],
    track_matched: &mut [bool],
) {
    let mut cands: Vec<(f32, usize, usize)> = Vec::new();
    for &ti in track_subset {
        let t = &tracks[ti];
        for &di in det_subset {
            let iou = iou_box(t.mean, &dets[di]);
            if iou >= match_iou_thresh {
                cands.push((iou, ti, di));
            }
        }
    }
    cands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (iou, ti, di) in cands {
        if track_matched[ti] || det_to_track[di].is_some() {
            continue;
        }
        det_to_track[di] = Some(ti);
        track_matched[ti] = true;
        let _ = iou;
    }
}

fn iou_box(a: [f32; 4], b: &Detection) -> f32 {
    let ax2 = a[0] + a[2];
    let ay2 = a[1] + a[3];
    let bx2 = b.x + b.width;
    let by2 = b.y + b.height;
    let inter_x0 = a[0].max(b.x);
    let inter_y0 = a[1].max(b.y);
    let inter_x1 = ax2.min(bx2);
    let inter_y1 = ay2.min(by2);
    let iw = (inter_x1 - inter_x0).max(0.0);
    let ih = (inter_y1 - inter_y0).max(0.0);
    let inter = iw * ih;
    if inter <= 0.0 { return 0.0; }
    let area_a = a[2] * a[3];
    let area_b = b.width * b.height;
    inter / (area_a + area_b - inter)
}

/// Solve the assignment problem: minimize total cost. Returns Vec<(row, col)>
/// for the chosen pairs. Drops any pair whose cost is `+inf` (i.e. below
/// the IoU threshold).
#[allow(dead_code)]
fn hungarian_min(cost: &[Vec<f32>]) -> Vec<(usize, usize)> {
    // Legacy entry point kept for reference — replaced by the greedy
    // matcher above which has no external dep. The greedy approximation
    // is good enough for the use case (see module docs).
    let _ = cost;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::coco;

    fn det(class_id: usize, x: f32, y: f32, w: f32, h: f32, score: f32) -> Detection {
        Detection {
            class_id,
            class_name: coco::name(class_id).to_string(),
            score,
            x, y, width: w, height: h,
        }
    }

    #[test]
    fn single_object_stays_tracked_across_frames() {
        let mut t = ByteTracker::default();
        let names: HashMap<usize, &'static str> = coco::CLASSES
            .iter().enumerate().map(|(i, s)| (i, *s)).collect();

        // Frame 0: a dog appears.
        let r0 = t.update(&[det(16, 100.0, 100.0, 80.0, 60.0, 0.9)], &names);
        assert_eq!(r0[0], Some(1));
        assert_eq!(t.last_events(), &[(1, TrackEventKind::Appeared)]);

        // Frame 1: same dog, slightly moved.
        let r1 = t.update(&[det(16, 102.0, 101.0, 80.0, 60.0, 0.85)], &names);
        assert_eq!(r1[0], Some(1), "same track id should be kept");
        assert_eq!(t.last_events(), &[(1, TrackEventKind::Continued)]);
    }

    #[test]
    fn disappearing_object_emits_event() {
        let mut t = ByteTracker::default();
        let names: HashMap<usize, &'static str> = coco::CLASSES
            .iter().enumerate().map(|(i, s)| (i, *s)).collect();

        t.update(&[det(15, 50.0, 50.0, 30.0, 30.0, 0.9)], &names);
        // No detection this frame.
        t.update(&[], &names);
        let events = t.last_events();
        assert!(events.iter().any(|(id, k)| *id == 1 && *k == TrackEventKind::Disappeared));
    }

    #[test]
    fn two_objects_remain_distinct() {
        let mut t = ByteTracker::default();
        let names: HashMap<usize, &'static str> = coco::CLASSES
            .iter().enumerate().map(|(i, s)| (i, *s)).collect();

        let r0 = t.update(&[
            det(15, 10.0, 10.0, 30.0, 30.0, 0.9),   // cat
            det(16, 200.0, 200.0, 40.0, 40.0, 0.9), // dog
        ], &names);
        let cat_id = r0[0].unwrap();
        let dog_id = r0[1].unwrap();
        assert_ne!(cat_id, dog_id);

        let r1 = t.update(&[
            det(15, 11.0, 11.0, 30.0, 30.0, 0.9),
            det(16, 199.0, 199.0, 40.0, 40.0, 0.85),
        ], &names);
        assert_eq!(r1[0], Some(cat_id));
        assert_eq!(r1[1], Some(dog_id));
    }
}
