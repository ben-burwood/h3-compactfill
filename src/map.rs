use geo::{BoundingRect, Centroid, Coord, MapCoords, MultiPolygon, Point};
use h3o::{CellIndex, LatLng};

pub struct CoordMap {
    transmeridian: bool, // polygon crosses the antimeridian?
    lat0_cos: f64,       // cosine(latitude0) for latitude compression
}

impl CoordMap {
    pub fn from_polygons(polygons: &MultiPolygon) -> Option<Self> {
        let Some(bbox) = polygons.bounding_rect() else {
            return None; // Empty / degenerate geometry
        };

        // Shape spans >180deg Longitudinally so it crosses antimeridian
        let transmeridian: bool = (bbox.max().x - bbox.min().x) > 180.0;

        let lat0: f64 = polygons
            .centroid()
            .map_or((bbox.min().y + bbox.max().y) / 2.0, |c| c.y());

        return Some(Self {
            transmeridian,
            lat0_cos: lat0.to_radians().cos().max(1e-6),
        });
    }

    /// Antimeridian Handing - Shifts negative Longitudes into a continuous [0, 360].
    fn antimeridian_lng(&self, lng: f64) -> f64 {
        if self.transmeridian && lng < 0.0 {
            lng + 360.0
        } else {
            lng
        }
    }

    /// Longitude Compression by `k = cos(lat₀)`, which makes Euclidean distances isotropic.
    fn compress_lng(&self, lng: f64) -> f64 {
        lng * self.lat0_cos
    }

    // normalise_cood provides meridian handling and latitude normalisation for a single coordinate
    fn normalise_coord(&self, lng: f64, lat: f64) -> Coord {
        let nlng = self.compress_lng(self.antimeridian_lng(lng));
        Coord { x: nlng, y: lat }
    }

    fn normalise_point(&self, lng: f64, lat: f64) -> Point {
        let coord = self.normalise_coord(lng, lat);
        Point::new(coord.x, coord.y)
    }

    pub fn cellindex_centroid_point(&self, cell: CellIndex) -> Point {
        let c = LatLng::from(cell);
        self.normalise_point(c.lng(), c.lat())
    }

    /// Cell Boundary as Vertices of normalised Coords
    pub fn cellindex_boundary_ring(&self, cell: CellIndex) -> Vec<Coord> {
        cell.boundary()
            .iter()
            .map(|ll| self.normalise_coord(ll.lng(), ll.lat()))
            .collect()
    }

    /// normalise_polygons provides meridian handling and latitude normalisation for multipolygon
    pub fn normalise_polygons(&self, polygons: MultiPolygon) -> MultiPolygon {
        return polygons.map_coords(|c| self.normalise_coord(c.x, c.y));
    }
}

/// Rough bounding-box area in km²
pub fn multipolygon_bbox_area(polygons: &MultiPolygon) -> f64 {
    let mbox = polygons.bounding_rect().expect("non-empty geometry");
    let width_km = (mbox.max().x - mbox.min().x) * 111.320;
    let height_km = (mbox.max().y - mbox.min().y) * 110.574;
    return (width_km * height_km).max(f64::MIN_POSITIVE);
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
                transmeridian: false,
                lat0_cos: 0.0
            }
            .antimeridian_lng(-179.0),
            -179.0
        );
        assert_eq!(
            CoordMap {
                transmeridian: true,
                lat0_cos: 0.0
            }
            .antimeridian_lng(-179.0),
            181.0
        );
        assert_eq!(
            CoordMap {
                transmeridian: true,
                lat0_cos: 0.0
            }
            .antimeridian_lng(10.0),
            10.0
        ); // positive lng untouched
    }

    #[test]
    fn near_equator_box_keeps_latitude_and_near_unit_longitude() {
        // At lat₀ ≈ 0 the cos(lat₀) compression is ~1, so coords barely move
        let p = rect(0.0, 0.0, 1.0, 1.0);
        let coord_map = CoordMap::from_polygons(&p).expect("non-empty");
        let n = coord_map.normalise_polygons(p);
        let b = n.bounding_rect().expect("has bbox");
        assert!((b.min().y - 0.0).abs() < 1e-9 && (b.max().y - 1.0).abs() < 1e-9);
        assert!(b.min().x.abs() < 1e-6);
        assert!(b.max().x > 0.99 && b.max().x <= 1.0);
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

    #[test]
    fn bbox_area_of_unit_box_is_about_12000_km2() {
        let area = multipolygon_bbox_area(&rect(0.0, 0.0, 1.0, 1.0));
        assert!(
            (12_000.0..12_500.0).contains(&area),
            "unexpected area {area}"
        );
    }
}
