//! High-level end-to-end regression tests for the public `compact_fill` API.
//!
//! These are deliberately few and coarse-grained. They exist to lock the
//! *output* of the algorithm before the `BASE_CELL_DESCENT` change (which
//! replaces Tiler seeding + the bounding-disk classifier). One matrix-driven
//! test walks a set of `(shape, resolution, mode)` combos and, per combo,
//! computes three things over the *same* inputs and checks two invariants:
//!
//!   1. Compact  = `compact_fill(.., FillKind::Compact)`
//!   2. Fill     = `compact_fill(.., FillKind::Full)`
//!   3. Tiler    = h3o's own polyfill (the reference implementation)
//!
//!   - Compact ⇔ Fill : uncompacting Compact to the target resolution equals Fill.
//!   - Fill    ⇔ Tiler: Fill equals the h3o Tiler coverage.
//!
//! All comparisons are set-based (order-independent) so they survive internal
//! refactors that only change emission order.

use std::collections::HashSet;

use geo::{Coord, LineString, MultiPolygon, Polygon};
use h3o::{
    CellIndex, Resolution,
    geom::{ContainmentMode, TilerBuilder},
};

use h3_compactfill::{FillKind, compact_fill};

/// Axis-aligned rectangle as a single-polygon `MultiPolygon`
/// (mirrors the `rect` helper in `src/map.rs`; `x = lng`, `y = lat`).
fn rect(min_lng: f64, min_lat: f64, max_lng: f64, max_lat: f64) -> MultiPolygon {
    let ring = LineString::new(vec![
        Coord { x: min_lng, y: min_lat },
        Coord { x: max_lng, y: min_lat },
        Coord { x: max_lng, y: max_lat },
        Coord { x: min_lng, y: max_lat },
        Coord { x: min_lng, y: min_lat },
    ]);
    MultiPolygon::new(vec![Polygon::new(ring, Vec::new())])
}

/// h3o's own polyfill for the same polygon/resolution/mode — the reference oracle.
fn tiler_coverage(
    polygons: &MultiPolygon,
    resolution: Resolution,
    mode: ContainmentMode,
) -> HashSet<CellIndex> {
    let mut tiler = TilerBuilder::new(resolution)
        .containment_mode(mode)
        .build();
    tiler
        .add_batch(polygons.clone())
        .expect("test geometry should be valid");
    tiler.into_coverage().collect()
}

/// Mid-latitude, non-seam rectangles kept small enough that every combo runs
/// fast, but large enough to exercise real descent (hundreds–thousands of cells).
fn shapes() -> Vec<(&'static str, MultiPolygon, Resolution)> {
    vec![
        ("small", rect(0.0, 51.0, 0.5, 51.5), Resolution::Nine),
        ("medium", rect(-2.0, 50.0, 3.0, 53.0), Resolution::Seven),
        ("wide", rect(-10.0, 40.0, 5.0, 50.0), Resolution::Six),
    ]
}

const MODES: [ContainmentMode; 2] = [ContainmentMode::ContainsCentroid, ContainmentMode::Covers];

#[test]
fn compact_and_fill_match_each_other_and_the_h3o_tiler() {
    for (name, poly, resolution) in shapes() {
        for mode in MODES {
            let label = format!("{name} @ {resolution:?} / {mode:?}");

            // 1. Compact, 2. Fill, 3. Tiler reference — all on the same inputs.
            let compact = compact_fill(poly.clone(), resolution, mode, FillKind::Compact);
            let fill: HashSet<CellIndex> =
                compact_fill(poly.clone(), resolution, mode, FillKind::Full)
                    .into_iter()
                    .collect();
            let tiler = tiler_coverage(&poly, resolution, mode);

            // Sanity: the combos should actually produce work.
            assert!(!fill.is_empty(), "{label}: fill produced no cells");

            // Invariant A — Compact ⇔ Fill.
            let uncompacted: HashSet<CellIndex> =
                CellIndex::uncompact(compact.iter().copied(), resolution).collect();
            assert_eq!(
                uncompacted,
                fill,
                "{label}: uncompacted Compact != Full (sym-diff {})",
                sym_diff(&uncompacted, &fill),
            );

            // Invariant B — Fill ⇔ h3o Tiler.
            assert_eq!(
                fill,
                tiler,
                "{label}: Full != h3o Tiler coverage (sym-diff {})",
                sym_diff(&fill, &tiler),
            );

            // Compact must never emit more cells than the fully-expanded fill.
            assert!(
                compact.len() <= fill.len(),
                "{label}: compact ({}) larger than full ({})",
                compact.len(),
                fill.len(),
            );
        }
    }
}

fn sym_diff(a: &HashSet<CellIndex>, b: &HashSet<CellIndex>) -> usize {
    a.symmetric_difference(b).count()
}
