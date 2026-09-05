use geo::{MultiPolygon, Point, PreparedGeometry, Relate};
use h3o::{CellIndex, LatLng, Resolution, geom::ContainmentMode};

mod map;
use crate::map::{multipolygon_bbox_area, normalise_polygons};
mod seed;
use crate::seed::{build_seeds, seed_resolution};
mod disk;
use crate::disk::{CoarseClassification, cell_disk};
mod descend;
use crate::descend::{Descended, descend};
mod compact_multiresolution;
use crate::compact_multiresolution::compact_multiresolution;
mod index;
use crate::index::PolygonIndex;
mod tiler;
use crate::tiler::cell_polygon;

pub fn compact_fill(
    polygons: MultiPolygon,
    resolution: Resolution,
    mode: ContainmentMode,
) -> Vec<CellIndex> {
    // Polygon Handling
    let Some(normalised_polygons) = normalise_polygons(polygons) else {
        return Vec::new(); // Empty geometry
    };

    let prepared_geometry = PreparedGeometry::from(&normalised_polygons);
    let polygon_index = PolygonIndex::build(&normalised_polygons);

    // Seeding
    let bbox_area = multipolygon_bbox_area(&normalised_polygons);
    let seeds = build_seeds(&normalised_polygons, seed_resolution(bbox_area, resolution));

    // Containment Tests - `classify_disk` (R-Tree test) is quicker than the `leaf_included` (relate or pip) Test

    // Coarse Cell Test with Bounding Disk
    // Define a closure over the R-Tree of Edges
    // TODO - Investigate a BBOX for this like Uber H3 Reference Implementation
    let classify_disk = |cell: CellIndex, margin: f64| -> CoarseClassification {
        // TODO - Handle map normalisation of cell coordinates
        let disk = cell_disk(cell, margin);
        if polygon_index.nearest_distance(disk.centre) <= disk.radius {
            // The shortest distance between the Polygon Boundary and the Disk Centre is less than the Disk's Radius
            // Hence the Disk and Polygon Boundary Cross
            CoarseClassification::Straddle
        } else {
            // Disk is wholly inside the or wholly outside the Polygon, use Point-in-Polygon to classify.
            if polygon_index.contains_point(disk.centre) {
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
                let ll = LatLng::from(cell);
                // TODO - Handle map normalisation of cell coordinates
                prepared_geometry
                    .relate(&Point::new(ll.lng(), ll.lat()))
                    .is_contains()
            }
            // Other ContainmentModes need the Cell's exact DE-9IM against the Polygon
            _ => {
                // TODO - Handle map normalisation of cell coordinates
                let im = prepared_geometry.relate(&cell_polygon(cell));
                match mode {
                    // ContainsBoundary must be Fully Contained
                    ContainmentMode::ContainsBoundary => im.is_covers(),
                    // IntersectsBoundary/Covers just need Intersect
                    _ => im.is_intersects(),
                }
            }
        }
    };

    // Top-Down Search
    // Each Seed Cell Descent streams its maximally-compacted cells into `out` the moment a sibling group is known incomplete,
    // and returns its root only if the whole subtree collapsed clean.
    // The interior fine band never accumulates — the live frontier is a single root-to-leaf path.
    let mut out: Vec<CellIndex> = Vec::new();
    let mut roots: Vec<CellIndex> = Vec::new();
    for seed in seeds {
        // Push all returned cells to the output vec
        match descend(seed, resolution, &classify_disk, &leaf_included, &mut |c| {
            out.push(c)
        }) {
            Descended::Included(root) => roots.push(root),
            Descended::Pruned => {}
        }
    }

    // Merge the SubTree Cells and Root Cells into an optimally Compact Set.
    out.extend(compact_multiresolution(roots));
    out
}
