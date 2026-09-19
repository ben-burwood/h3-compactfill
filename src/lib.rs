use geo::{MultiPolygon, PreparedGeometry, Relate};
use h3o::{CellIndex, Resolution, geom::ContainmentMode};

mod map;
use crate::map::CoordMap;
mod bbox;
use crate::bbox::cell_bbox;
mod coarse;
use crate::coarse::{CoarseClassification, HANDOFF_RES};
mod descend;
use crate::descend::{Descended, descend, descend_compact};
mod compact_multiresolution;
use crate::compact_multiresolution::compact_multiresolution;
mod index;
use crate::index::PolygonIndex;
mod tiler;
use crate::tiler::cell_polygon;

// Full FillKind mirrors standard implementation of `polygonToCells`
// Compact FillKind Short-Circuits at Coarse Cells to efficiently produce the compacted output
pub enum FillKind {
    Compact,
    Full,
}

pub fn compact_fill(
    polygons: MultiPolygon,
    resolution: Resolution,
    mode: ContainmentMode,
    kind: FillKind,
) -> Vec<CellIndex> {
    let Some(coord_map) = CoordMap::from_polygons(&polygons) else {
        return Vec::new();
    };

    let poly_bbox = coord_map.polygon_ll_bbox(&polygons);

    let normalised_polygons = coord_map.normalise_polygons(polygons);

    let prepared_geometry = PreparedGeometry::from(&normalised_polygons);
    let polygon_index = PolygonIndex::build(&normalised_polygons);

    // Containment Tests - the coarse `classify` test is quicker than the `leaf_included` (relate or pip) Test.
    //
    // - Ultra-coarse cells (res < HANDOFF_RES) - raw-degrees BBox (seam/pole aware) and only
    //   prune (Outside) or subdivide (Straddle).
    // - Finer cells - axis-aligned Box in the normalised frame over the R-Tree of edges.
    //   If no edge touches the box the cell is wholly in/out (resolved by point-in-polygon,
    //   which drives Inside + compaction); otherwise it Straddles.
    let classify = |cell: CellIndex, scale: f64| -> CoarseClassification {
        if usize::from(cell.resolution()) < HANDOFF_RES {
            return if cell_bbox(cell, scale).overlaps(&poly_bbox) {
                CoarseClassification::Straddle
            } else {
                CoarseClassification::Outside
            };
        }

        let (min, max) = coord_map.cell_aabb(cell, scale);
        if polygon_index.any_edge_in_aabb(min, max) {
            // Edge lies within the Cell's BBox → boundary crosses the cell.
            CoarseClassification::Straddle
        } else {
            // No Edge touches the BBox -> cell wholly inside or wholly outside.
            //
            // Use Point-in-Polygon for final Classification
            if polygon_index.contains_point(coord_map.cellindex_centroid_point(cell)) {
                CoarseClassification::Inside
            } else {
                CoarseClassification::Outside
            }
        }
    };

    // Leaf (Target Resolution) Containment Test
    // Define a closure over the preparedGeometry
    let leaf_included = |cell: CellIndex| -> bool {
        match mode {
            // Centroid Mode is just Point-in-Polygon
            ContainmentMode::ContainsCentroid => {
                let centre = &coord_map.cellindex_centroid_point(cell);
                prepared_geometry.relate(centre).is_contains()
            }
            // Other ContainmentModes need the Cell's exact DE-9IM against the Polygon
            _ => {
                let im = prepared_geometry.relate(&cell_polygon(cell, &coord_map));
                match mode {
                    // ContainsBoundary must be Fully Contained
                    ContainmentMode::ContainsBoundary => im.is_covers(),
                    // IntersectsBoundary/Covers just need Intersect
                    _ => im.is_intersects(),
                }
            }
        }
    };

    // Top-Down Search from the 122 Res0 Base Cells.
    //
    // Each base-cell descent streams its Cells into `out` the moment a sibling group is known
    // incomplete, and returns its root only if the whole subtree collapsed clean.
    let mut out = Vec::new();
    match kind {
        FillKind::Full => {
            for root in CellIndex::base_cells() {
                descend(root, resolution, &classify, &leaf_included, &mut |c| {
                    out.push(c)
                });
            }
        }
        FillKind::Compact => {
            let mut roots = Vec::new();
            for root in CellIndex::base_cells() {
                match descend_compact(root, resolution, &classify, &leaf_included, &mut |c| {
                    out.push(c)
                }) {
                    Descended::Included(root) => roots.push(root),
                    Descended::Pruned => {}
                }
            }
            out.extend(compact_multiresolution(roots));
        }
    }
    out
}
