//! High-level end-to-end accuracy tests for the public `compact_fill` API.
//!
//! The strategy is *differential testing against a trusted oracle*: for every
//! input we compute three things over the **same** geometry and check the
//! invariants that must hold for the algorithm to be correct.
//!
//!   1. Compact  = `compact_fill(.., FillKind::Compact)`
//!   2. Fill     = `compact_fill(.., FillKind::Full)`
//!   3. Tiler    = h3o's own polyfill (the reference implementation / oracle)
//!
//!   - Compact ⇔ Fill   : uncompacting Compact to the target resolution equals Fill.
//!   - Fill    ⇔ Tiler  : Fill equals the h3o Tiler coverage (the accuracy check).
//!   - Compact ⇔ compact(Fill) : Compact is the *canonical maximal* compaction.
//!
//! All comparisons are set-based (order-independent) so they survive internal
//! refactors that only change emission order.
//!
//! Coverage is spread deliberately across the parts of the domain where an H3
//! polyfill is most likely to break: all four containment modes, holes,
//! multi-polygons, both hemispheres, high latitudes, the antimeridian seam, a
//! wide range of resolutions, and a randomized fuzzing sweep over the globe.

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
    MultiPolygon::new(vec![Polygon::new(rect_ring(min_lng, min_lat, max_lng, max_lat), Vec::new())])
}

/// A closed CCW ring for the given bounds.
fn rect_ring(min_lng: f64, min_lat: f64, max_lng: f64, max_lat: f64) -> LineString {
    LineString::new(vec![
        Coord { x: min_lng, y: min_lat },
        Coord { x: max_lng, y: min_lat },
        Coord { x: max_lng, y: max_lat },
        Coord { x: min_lng, y: max_lat },
        Coord { x: min_lng, y: min_lat },
    ])
}

/// Rectangle with a rectangular hole punched out of the middle — exercises
/// interior rings, which drive far more `Straddle` classification than a solid.
fn rect_with_hole(
    min_lng: f64,
    min_lat: f64,
    max_lng: f64,
    max_lat: f64,
    inset: f64,
) -> MultiPolygon {
    let outer = rect_ring(min_lng, min_lat, max_lng, max_lat);
    let hole = rect_ring(
        min_lng + inset,
        min_lat + inset,
        max_lng - inset,
        max_lat - inset,
    );
    MultiPolygon::new(vec![Polygon::new(outer, vec![hole])])
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

/// Hand-picked shapes spanning the tricky regions of the domain. Kept small
/// enough that the full (shape × mode) matrix runs fast, but large enough to
/// exercise real descent (hundreds–thousands of cells).
fn shapes() -> Vec<(&'static str, MultiPolygon, Resolution)> {
    vec![
        // Baseline mid-latitude solids at a spread of resolutions.
        ("small", rect(0.0, 51.0, 0.2, 51.2), Resolution::Nine),
        ("medium", rect(-2.0, 50.0, 3.0, 53.0), Resolution::Six),
        ("wide", rect(-10.0, 40.0, 5.0, 50.0), Resolution::Five),
        // Coarse and fine resolutions to exercise the descent depth range.
        ("coarse", rect(-20.0, 20.0, 20.0, 45.0), Resolution::Three),
        ("fine", rect(0.0, 51.0, 0.05, 51.05), Resolution::Eleven),
        // Interior ring (hole).
        ("holed", rect_with_hole(-3.0, 48.0, 3.0, 53.0, 1.5), Resolution::Six),
        // Disjoint multi-polygon.
        (
            "multi",
            MultiPolygon::new(vec![
                rect(0.0, 51.0, 0.5, 51.5).0.pop().unwrap(),
                rect(4.0, 48.0, 4.6, 48.6).0.pop().unwrap(),
            ]),
            Resolution::Eight,
        ),
        // Southern hemisphere.
        ("southern", rect(-60.0, -35.0, -55.0, -30.0), Resolution::Six),
        // Equator-crossing.
        ("equator", rect(30.0, -3.0, 35.0, 3.0), Resolution::Six),
        // High latitude (near-polar), still a well-formed box.
        ("high_lat", rect(10.0, 78.0, 25.0, 82.0), Resolution::Five),
        // Antimeridian / transmeridian seam.
        ("antimeridian", rect(179.0, 0.0, -179.0, 1.0), Resolution::Six),
    ]
}

const MODES: [ContainmentMode; 4] = [
    ContainmentMode::ContainsCentroid,
    ContainmentMode::ContainsBoundary,
    ContainmentMode::IntersectsBoundary,
    ContainmentMode::Covers,
];

#[test]
fn compact_and_fill_match_each_other_and_the_h3o_tiler() {
    for (name, poly, resolution) in shapes() {
        for mode in MODES {
            let label = format!("{name} @ {resolution:?} / {mode:?}");
            check_all_invariants(&label, &poly, resolution, mode);
        }
    }
}

/// The shared invariant bundle used by both the hand-picked matrix and the fuzzer.
fn check_all_invariants(
    label: &str,
    poly: &MultiPolygon,
    resolution: Resolution,
    mode: ContainmentMode,
) {
    let compact = compact_fill(poly.clone(), resolution, mode, FillKind::Compact);
    let fill: HashSet<CellIndex> = compact_fill(poly.clone(), resolution, mode, FillKind::Full)
        .into_iter()
        .collect();
    let tiler = tiler_coverage(poly, resolution, mode);

    // Invariant B — Fill ⇔ h3o Tiler (the accuracy check).
    assert_eq!(
        fill,
        tiler,
        "{label}: Full != h3o Tiler coverage (sym-diff {})",
        sym_diff(&fill, &tiler),
    );

    // Invariant A — Compact ⇔ Fill (compaction is lossless).
    let uncompacted: HashSet<CellIndex> =
        CellIndex::uncompact(compact.iter().copied(), resolution).collect();
    assert_eq!(
        uncompacted,
        fill,
        "{label}: uncompacted Compact != Full (sym-diff {})",
        sym_diff(&uncompacted, &fill),
    );

    // Invariant C — Compact is the canonical *maximal* compaction of Fill.
    // (`compact.len() <= fill.len()` only proves "not bigger"; this proves optimal.)
    let compact_set: HashSet<CellIndex> = compact.iter().copied().collect();
    let mut canonical_vec: Vec<CellIndex> = fill.iter().copied().collect();
    CellIndex::compact(&mut canonical_vec).expect("fill is a valid cell set");
    let canonical: HashSet<CellIndex> = canonical_vec.into_iter().collect();
    assert_eq!(
        compact_set,
        canonical,
        "{label}: Compact != canonical compact(Full) (sym-diff {})",
        sym_diff(&compact_set, &canonical),
    );
}

/// Regression: a box that does **not** cross the antimeridian but sits close
/// enough that neighbouring cells individually straddle ±180°. Before the
/// per-coordinate seam unwrap, those cells were built into degenerate
/// globe-spanning polygons and spuriously matched under the polygon-based modes
/// (`Covers` / `IntersectsBoundary` / `ContainsBoundary`), inflating `Full`
/// with ~48 phantom cells sitting on the seam. `ContainsCentroid` was immune
/// because it never builds a cell polygon.
#[test]
fn near_antimeridian_box_does_not_leak_seam_cells() {
    let poly = rect(176.312, -14.017, 177.982, -9.671);
    for mode in MODES {
        check_all_invariants(
            &format!("near-antimeridian / {mode:?}"),
            &poly,
            Resolution::Five,
            mode,
        );
    }
}

/// Randomized differential fuzzing against the oracle. Because h3o's Tiler is a
/// trusted independent implementation, a random box anywhere on the globe is a
/// free correctness sample: `Full` must always equal the Tiler coverage.
///
/// Deterministic (fixed seed) so a failure is always reproducible.
#[test]
fn randomized_boxes_match_the_h3o_tiler() {
    let mut rng = Rng::new(0x00C0FFEE_D00D_u64);

    for i in 0..200 {
        // Roam a wide latitude band (clear of the exact poles, where an
        // axis-aligned box degenerates) and the full longitude range. Box edges
        // are clamped just inside ±180° so we never feed h3o's Tiler a
        // seam-crossing polygon (which is outside its input contract) — but
        // edges reaching ±179° still exercise *cells* that straddle the seam,
        // which is exactly the antimeridian path under test.
        let lat0 = rng.range(-80.0, 80.0);
        let lng0 = rng.range(-179.0, 179.0);
        let h = rng.range(0.05, 6.0);
        // Longitude degrees shrink with cos(lat); widen boxes toward the poles
        // so their ground span stays comparable, but cap the raw width.
        let w = (rng.range(0.05, 6.0) / lat0.to_radians().cos().max(0.15)).min(20.0);

        let min_lat = (lat0 - h / 2.0).clamp(-89.0, 89.0);
        let max_lat = (lat0 + h / 2.0).clamp(-89.0, 89.0);
        let min_lng = (lng0 - w / 2.0).max(-179.0);
        let max_lng = (lng0 + w / 2.0).min(179.0);
        if (max_lat - min_lat) < 1e-6 || (max_lng - min_lng) < 1e-6 {
            continue;
        }

        let poly = rect(min_lng, min_lat, max_lng, max_lat);

        // Resolution scaled loosely to box size so cell counts stay bounded.
        let resolution = pick_resolution(&mut rng, w.max(h));
        let mode = MODES[rng.next_usize(MODES.len())];

        let label = format!(
            "fuzz#{i}: box[{min_lng:.3},{min_lat:.3},{max_lng:.3},{max_lat:.3}] @ {resolution:?} / {mode:?}"
        );

        let fill: HashSet<CellIndex> =
            compact_fill(poly.clone(), resolution, mode, FillKind::Full)
                .into_iter()
                .collect();
        let tiler = tiler_coverage(&poly, resolution, mode);
        assert_eq!(
            fill,
            tiler,
            "{label}: Full != h3o Tiler coverage (sym-diff {})",
            sym_diff(&fill, &tiler),
        );

        // Also spot-check the compaction round-trip on non-empty results.
        if !fill.is_empty() {
            let compact = compact_fill(poly, resolution, mode, FillKind::Compact);
            let uncompacted: HashSet<CellIndex> =
                CellIndex::uncompact(compact.iter().copied(), resolution).collect();
            assert_eq!(uncompacted, fill, "{label}: uncompacted Compact != Full");
        }
    }
}

/// Pick a resolution that keeps the cell count sane for a box of the given
/// approximate span (in degrees): bigger boxes → coarser cells.
fn pick_resolution(rng: &mut Rng, span_deg: f64) -> Resolution {
    let base = if span_deg > 4.0 {
        4
    } else if span_deg > 1.0 {
        6
    } else if span_deg > 0.3 {
        8
    } else {
        10
    };
    let jitter = rng.next_usize(2) as u8; // base or base+1
    Resolution::try_from(base + jitter).expect("valid resolution")
}

fn sym_diff(a: &HashSet<CellIndex>, b: &HashSet<CellIndex>) -> usize {
    a.symmetric_difference(b).count()
}

/// Tiny deterministic PRNG (SplitMix64) — no external `rand` dependency, and a
/// fixed seed keeps fuzz failures reproducible.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform f64 in [0, 1).
    fn next_f64(&mut self) -> f64 {
        // Top 53 bits → [0, 1).
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    fn next_usize(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}
