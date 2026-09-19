use geo::{BoundingRect, Coord, MapCoords, MultiPolygon, Point};
use h3o::{CellIndex, LatLng};

use crate::bbox::LlBBox;

pub struct CoordMap {
    transmeridian: bool, // polygon crosses the antimeridian?
}

impl CoordMap {
    pub fn from_polygons(polygons: &MultiPolygon) -> Option<Self> {
        let Some(bbox) = polygons.bounding_rect() else {
            return None; // Empty / degenerate geometry
        };

        // Shape spans >180deg Longitudinally so it crosses antimeridian
        let transmeridian: bool = (bbox.max().x - bbox.min().x) > 180.0;

        return Some(Self { transmeridian });
    }

    /// Antimeridian Handing - Shifts negative Longitudes into a continuous [0, 360].
    fn antimeridian_lng(&self, lng: f64) -> f64 {
        if self.transmeridian && lng < 0.0 {
            lng + 360.0
        } else {
            lng
        }
    }

    // normalise_coord provides antimeridian handling for a single coordinate.
    //
    // Classification uses axis-aligned bounding boxes and a ray-cast point-in-polygon test,
    // both invariant under a uniform longitude scale, so no `cos(lat0)` compression is needed —
    // only the antimeridian shift, which removes the ±180 seam.
    fn normalise_coord(&self, lng: f64, lat: f64) -> Coord {
        Coord {
            x: self.antimeridian_lng(lng),
            y: lat,
        }
    }

    /// Cell Centroid as a Point in the normalised frame
    pub fn cellindex_centroid_point(&self, cell: CellIndex) -> Point {
        let c = LatLng::from(cell);
        let coord = self.normalise_coord(c.lng(), c.lat());
        Point::new(coord.x, coord.y)
    }

    /// Cell Boundary as Vertices of normalised Coords
    pub fn cellindex_boundary_ring(&self, cell: CellIndex) -> Vec<Coord> {
        cell.boundary()
            .iter()
            .map(|ll| self.normalise_coord(ll.lng(), ll.lat()))
            .collect()
    }

    /// Cell's axis-aligned Bounding Box in the normalised frame
    /// Inflated by `scale` about its centre, as `(min, max)` corners.
    pub fn cell_aabb(&self, cell: CellIndex, scale: f64) -> ([f64; 2], [f64; 2]) {
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for c in self.cellindex_boundary_ring(cell) {
            min_x = min_x.min(c.x);
            min_y = min_y.min(c.y);
            max_x = max_x.max(c.x);
            max_y = max_y.max(c.y);
        }
        // Scale about the box centre (H3's `scaleBBox`).
        let cx = (min_x + max_x) / 2.0;
        let cy = (min_y + max_y) / 2.0;
        (
            [cx + (min_x - cx) * scale, cy + (min_y - cy) * scale],
            [cx + (max_x - cx) * scale, cy + (max_y - cy) * scale],
        )
    }

    /// Apply antimeridian handling to a whole multipolygon, moving it into the seam-free frame.
    pub fn normalise_polygons(&self, polygons: MultiPolygon) -> MultiPolygon {
        return polygons.map_coords(|c| self.normalise_coord(c.x, c.y));
    }

    /// Polygon's Bounding Box
    ///
    /// In **raw degrees** (no `cos(lat0)` compression)
    /// Antimeridian shift is applied so a transmeridian polygon is encoded like H3 with `east < west`
    ///
    /// Must be called on the polygons *before* `normalise_polygons`.
    pub fn polygon_ll_bbox(&self, polygons: &MultiPolygon) -> LlBBox {
        let mut south = f64::INFINITY;
        let mut north = f64::NEG_INFINITY;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for poly in polygons {
            for c in poly.exterior().0.iter() {
                let lng = self.antimeridian_lng(c.x);
                south = south.min(c.y);
                north = north.max(c.y);
                lo = lo.min(lng);
                hi = hi.max(lng);
            }
        }
        let wrap = |l: f64| if l > 180.0 { l - 360.0 } else { l };
        LlBBox {
            south,
            north,
            west: wrap(lo),
            east: wrap(hi),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{BoundingRect, LineString, Polygon};

    /// Axis-aligned rectangle as a single-polygon `MultiPolygon`.
    fn rect(min_lng: f64, min_lat: f64, max_lng: f64, max_lat: f64) -> MultiPolygon {
        let ring = LineString::new(vec![
            Coord {
                x: min_lng,
                y: min_lat,
            },
            Coord {
                x: max_lng,
                y: min_lat,
            },
            Coord {
                x: max_lng,
                y: max_lat,
            },
            Coord {
                x: min_lng,
                y: max_lat,
            },
            Coord {
                x: min_lng,
                y: min_lat,
            },
        ]);
        MultiPolygon::new(vec![Polygon::new(ring, Vec::new())])
    }

    #[test]
    fn empty_geometry_is_none() {
        assert!(CoordMap::from_polygons(&MultiPolygon::new(Vec::new())).is_none());
    }

    #[test]
    fn longitude_map_shifts_only_when_transmeridian() {
        assert_eq!(
            CoordMap {
                transmeridian: false
            }
            .antimeridian_lng(-179.0),
            -179.0
        );
        assert_eq!(
            CoordMap {
                transmeridian: true
            }
            .antimeridian_lng(-179.0),
            181.0
        );
        assert_eq!(
            CoordMap {
                transmeridian: true
            }
            .antimeridian_lng(10.0),
            10.0
        ); // positive lng untouched
    }

    #[test]
    fn non_transmeridian_box_is_unchanged() {
        // With no cos(lat0) compression and no seam to shift, a normal box is passed through verbatim.
        let p = rect(0.0, 0.0, 1.0, 1.0);
        let coord_map = CoordMap::from_polygons(&p).expect("non-empty");
        let n = coord_map.normalise_polygons(p);
        let b = n.bounding_rect().expect("has bbox");
        assert!((b.min().y - 0.0).abs() < 1e-9 && (b.max().y - 1.0).abs() < 1e-9);
        assert!(b.min().x.abs() < 1e-9);
        assert!((b.max().x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn antimeridian_box_is_made_contiguous() {
        // A box from 179.6°E to -179.6°E spans >180°, so negative longitudes are
        // shifted into a contiguous range with no seam.
        let p = rect(179.6, 0.4, -179.6, 0.8);
        let coord_map = CoordMap::from_polygons(&p).expect("non-empty");
        let n = coord_map.normalise_polygons(p);
        let b = n.bounding_rect().expect("has bbox");
        assert!(
            b.min().x > 179.0,
            "min lng {} should be shifted past the seam",
            b.min().x
        );
        assert!(
            b.max().x > 180.0,
            "max lng {} should exceed 180 after shift",
            b.max().x
        );
    }
}
