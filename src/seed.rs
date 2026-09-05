use geo::MultiPolygon;
use h3o::{
    CellIndex, Resolution,
    geom::{ContainmentMode, TilerBuilder},
};
use std::collections::HashSet;

/// Seed Resolution is picked as the finest resolution whose average cell is
/// still at least as large as the MultiPolygon's `bbox_area_km2`.
/// Clamped to the target Resolution (never need smaller than this)
pub fn seed_resolution(bbox_area_km2: f64, target_resolution: Resolution) -> Resolution {
    Resolution::range(Resolution::Zero, target_resolution)
        .take_while(|res| res.area_km2() >= bbox_area_km2)
        .last()
        .unwrap_or(Resolution::Zero)
}

/// Seed Cells are built by Tiling (Covers) the MultiPolygon at the Seed Resolution and dilating Grid-Ring buffer
pub fn build_seeds(polygons: &MultiPolygon, seed_resolution: Resolution) -> Vec<CellIndex> {
    let mut tiler = TilerBuilder::new(seed_resolution)
        .containment_mode(ContainmentMode::Covers)
        .build();

    tiler
        .add_batch(polygons.clone())
        .expect("input geometry should be valid");

    let mut seen: HashSet<CellIndex> = HashSet::new();
    let mut seeds: Vec<CellIndex> = Vec::new();
    for cover_cell in tiler.into_coverage() {
        for cell in cover_cell.grid_disk::<Vec<CellIndex>>(1) {
            if seen.insert(cell) {
                seeds.push(cell);
            }
        }
    }
    seeds
}
