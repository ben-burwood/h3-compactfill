use geo::{MultiPolygon, PreparedGeometry, Relate};
use h3o::{CellIndex, Resolution, geom::ContainmentMode};

mod map;
use crate::map::{CoordMap, multipolygon_bbox_area};
mod seed;
use crate::seed::{build_seeds, seed_resolution};
mod disk;
use crate::disk::{CoarseClassification, cell_disk};
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

    let normalised_polygons = coord_map.normalise_polygons(polygons);

    let prepared_geometry = PreparedGeometry::from(&normalised_polygons);
    let polygon_index = PolygonIndex::build(&normalised_polygons);

    // Seeding
    // TODO - This relies on h3o Tiler so realistically can't be used if the intention is to superseed that implementation
    // This is likely a fairly micro-optimisation and using the 122 Res0 Coarse Cells should work fine
    let bbox_area = multipolygon_bbox_area(&normalised_polygons);
    let seeds = build_seeds(&normalised_polygons, seed_resolution(bbox_area, resolution));

    // Containment Tests - `classify_disk` (R-Tree test) is quicker than the `leaf_included` (relate or pip) Test

    // Coarse Cell Test with Bounding Disk
    // Define a closure over the R-Tree of Edges
    // TODO - Investigate a BBOX for this like Uber H3 Reference Implementation
    let classify_disk = |cell: CellIndex, margin: f64| -> CoarseClassification {
        let disk = cell_disk(cell, margin, &coord_map);
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

    // Top-Down Search
    // Each Seed Cell Descent streams its Cells into `out` the moment a sibling group is known incomplete,
    // and returns its root only if the whole subtree collapsed clean.
    let mut out = Vec::new();
    match kind {
        FillKind::Full => {
            for seed in seeds {
                descend(seed, resolution, &classify_disk, &leaf_included, &mut |c| {
                    out.push(c)
                });
            }
        }
        FillKind::Compact => {
            let mut roots = Vec::new();
            for seed in seeds {
                match descend_compact(seed, resolution, &classify_disk, &leaf_included, &mut |c| {
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
