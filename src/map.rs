use geo::{BoundingRect, Centroid, Coord, MapCoords, MultiPolygon};

/// Antimeridian Handing - Shifts negative Longitudes into a continuous [0, 360].
fn longitude_map(lng: f64, transmeridian: bool) -> f64 {
    return if transmeridian && lng < 0.0 {
        lng + 360.0
    } else {
        lng
    };
}

/// Longitude Compression by `k = cos(lat₀)`, which makes Euclidean distances isotropic.
fn longitude_compression(lng: f64, lat0: f64) -> f64 {
    return lng * lat0.to_radians().cos().max(1e-6);
}

fn normalise_coord(lng: f64, lat: f64, transmeridian: bool, lat0: f64) -> Coord {
    let nlng = longitude_compression(longitude_map(lng, transmeridian), lat0);
    Coord { x: nlng, y: lat }
}

/// normalise_polygons provides meridian handling and latitude normalisation
pub fn normalise_polygons(polygons: MultiPolygon) -> Option<MultiPolygon> {
    let Some(bbox) = polygons.bounding_rect() else {
        return None; // Empty / degenerate geometry
    };

    // Shape spans >180deg Longitudinally so it crosses antimeridian
    let transmeridian: bool = (bbox.max().x - bbox.min().x) > 180.0;

    let lat0: f64 = polygons
        .centroid()
        .map_or((bbox.min().y + bbox.max().y) / 2.0, |c| c.y());

    return Some(polygons.map_coords(|c| normalise_coord(c.x, c.y, transmeridian, lat0)));
}

/// Rough bounding-box area in km²
pub fn multipolygon_bbox_area(polygons: &MultiPolygon) -> f64 {
    let mbox = polygons.bounding_rect().expect("non-empty geometry");
    let width_km = (mbox.max().x - mbox.min().x) * 111.320;
    let height_km = (mbox.max().y - mbox.min().y) * 110.574;
    return (width_km * height_km).max(f64::MIN_POSITIVE);
}
