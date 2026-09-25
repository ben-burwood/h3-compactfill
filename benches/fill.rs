//! Direct timing + memory benchmark for `compact_fill` (no criterion).
//!
//! Runs a moderate matrix of polygon size × resolution over four runners:
//!   - `compact_fill` Compact  — this crate, short-circuiting at coarse cells,
//!   - `compact_fill` Full     — this crate, expanded to the target resolution,
//!   - h3o Tiler Full          — the reference polyfill (`polygonToCells`),
//!   - h3o Tiler + compact     — the naive two-stage baseline (polyfill then
//!                               `CellIndex::compact`) that this crate replaces.
//! Each is timed with `std::time::Instant`, and a custom global allocator reports
//! two memory figures so this crate can be compared head-to-head against both h3o
//! pipelines (Full vs Tiler-Full; Compact vs Tiler-compact):
//!   - `peak MB`  — high-water live bytes (what the process must hold at once),
//!   - `alloc MB` — cumulative bytes requested (allocation churn); a hot path of
//!                  small transient buffers can keep peak low while alloc is huge.
//!
//! Regression workflow:
//!   1. On `main`, before the change:  `cargo bench --bench fill`  → save the table.
//!   2. After the change: re-run and diff time + peak MB + alloc MB.
//!
//! Note: `harness = false` in Cargo.toml — this is a plain `main`, not libtest.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

use geo::{Coord, LineString, MultiPolygon, Polygon};
use h3o::{
    CellIndex, Resolution,
    geom::{ContainmentMode, TilerBuilder},
};

use h3_compactfill::{FillKind, compact_fill};

/// Allocator that both peaks (max live bytes) and *totals* (cumulative bytes
/// requested). Peak reflects the high-water live set; total reflects allocation
/// churn — many small transient buffers can keep peak low while total is huge.
///
/// `realloc` is intentionally left as the `GlobalAlloc` default (alloc + copy +
/// dealloc through these same methods) so a `Vec` growing by doubling is counted
/// the way the program actually allocates.
struct TrackingAlloc;

static TOTAL_BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let size = layout.size();
            TOTAL_BYTES.fetch_add(size, Relaxed);
            let live = LIVE_BYTES.fetch_add(size, Relaxed) + size;
            PEAK_BYTES.fetch_max(live, Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Relaxed);
    }
}

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc;

const MB: f64 = 1024.0 * 1024.0;

/// Zero the total counter and set peak to the current live set, so the next
/// measured region reports only its own allocations.
fn reset_stats() {
    TOTAL_BYTES.store(0, Relaxed);
    PEAK_BYTES.store(LIVE_BYTES.load(Relaxed), Relaxed);
}

/// Cumulative bytes requested since the last `reset_stats`, in MB.
fn total_mb() -> f64 {
    TOTAL_BYTES.load(Relaxed) as f64 / MB
}

/// High-water live bytes since the last `reset_stats`, in MB.
fn peak_mb() -> f64 {
    PEAK_BYTES.load(Relaxed) as f64 / MB
}

/// Timed iterations per variant (min + mean are reported).
const ITERS: u32 = 5;
/// Containment mode used across the matrix (kept fixed so runs are comparable).
const MODE: ContainmentMode = ContainmentMode::Covers;

/// Axis-aligned rectangle as a single-polygon `MultiPolygon` (`x = lng`, `y = lat`).
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

/// The four implementations timed side by side.
#[derive(Clone, Copy)]
enum Runner {
    /// `compact_fill` short-circuiting at coarse cells (mixed-resolution output).
    Compact,
    /// `compact_fill` fully expanded to the target resolution.
    Full,
    /// h3o's own `polygonToCells` (the reference implementation), full output.
    TilerFull,
    /// h3o `polygonToCells` followed by `CellIndex::compact` — the naive
    /// two-stage baseline this crate is designed to replace.
    TilerCompact,
}

impl Runner {
    fn label(self) -> &'static str {
        match self {
            Runner::Compact => "compact",
            Runner::Full => "full",
            Runner::TilerFull => "tiler-full",
            Runner::TilerCompact => "tiler-comp",
        }
    }

    /// Run once and return the produced cells (owned, so the allocation counts
    /// toward peak-memory measurement).
    fn run(self, poly: MultiPolygon, resolution: Resolution) -> Vec<CellIndex> {
        match self {
            Runner::Compact => compact_fill(poly, resolution, MODE, FillKind::Compact),
            Runner::Full => compact_fill(poly, resolution, MODE, FillKind::Full),
            Runner::TilerFull => tiler_coverage(poly, resolution),
            Runner::TilerCompact => {
                let mut cells = tiler_coverage(poly, resolution);
                CellIndex::compact(&mut cells).expect("tiler coverage should compact");
                cells
            }
        }
    }
}

/// h3o's `polygonToCells` coverage at the target resolution.
fn tiler_coverage(poly: MultiPolygon, resolution: Resolution) -> Vec<CellIndex> {
    let mut tiler = TilerBuilder::new(resolution).containment_mode(MODE).build();
    tiler.add_batch(poly).expect("input geometry should be valid");
    tiler.into_coverage().collect()
}

const RUNNERS: [Runner; 4] = [
    Runner::Compact,
    Runner::Full,
    Runner::TilerFull,
    Runner::TilerCompact,
];

/// A moderate-but-hard matrix targeting large cell counts per variant.
fn shapes() -> Vec<(&'static str, MultiPolygon, Resolution)> {
    vec![
        // city-scale, fine resolution
        ("small", rect(0.0, 51.0, 0.2, 51.2), Resolution::Thirteen),
        // country-scale, mid resolution
        ("medium", rect(-4.0, 50.0, 4.0, 56.0), Resolution::Nine),
        // sub-continent scale, coarse resolution
        ("large", rect(-20.0, 30.0, 20.0, 55.0), Resolution::Eight),
    ]
}

fn main() {
    println!(
        "{:<8} {:<11} {:<5} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "shape", "kind", "res", "cells", "min ms", "mean ms", "peak MB", "alloc MB"
    );
    println!("{}", "-".repeat(80));

    for (name, poly, resolution) in shapes() {
        for runner in RUNNERS {
            // Measure output size + peak/total heap on a clean run.
            // The input clone is done before resetting so only the runner's own
            // allocations are counted.
            let input = poly.clone();
            reset_stats();
            let out = runner.run(input, resolution);
            let cells = out.len();
            let peak_mb = peak_mb();
            let alloc_mb = total_mb();
            drop(out);

            // Time ITERS runs; the input clone is outside the timed region.
            let mut min = Duration::MAX;
            let mut total = Duration::ZERO;
            for _ in 0..ITERS {
                let p = poly.clone();
                let start = Instant::now();
                let out = runner.run(p, resolution);
                let elapsed = start.elapsed();
                std::hint::black_box(&out);
                min = min.min(elapsed);
                total += elapsed;
            }
            let mean = total / ITERS;

            println!(
                "{:<8} {:<11} {:<5} {:>10} {:>10.2} {:>10.2} {:>10.2} {:>10.2}",
                name,
                runner.label(),
                u8::from(resolution),
                cells,
                min.as_secs_f64() * 1e3,
                mean.as_secs_f64() * 1e3,
                peak_mb,
                alloc_mb,
            );
        }
        println!();
    }
}
