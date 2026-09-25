use h3o::{CellIndex, LatLng};

/// Axis-aligned lat/lng Bounding Box in **raw degrees**
#[derive(Clone, Copy, Debug)]
pub struct LlBBox {
    pub south: f64,
    pub north: f64,
    pub west: f64,
    pub east: f64,
}

impl LlBBox {
    // Crosses the AntiMeridian
    pub fn is_transmeridian(&self) -> bool {
        self.east < self.west
    }

    /// Two boxes overlap in BOTH Latitude and Longitude.
    pub fn overlaps(&self, other: &LlBBox) -> bool {
        if self.south > other.north || other.south > self.north {
            return false;
        }
        lng_overlap(self.west, self.east, other.west, other.east)
    }
}

fn lng_overlap(a_west: f64, a_east: f64, b_west: f64, b_east: f64) -> bool {
    let a_tm = a_east < a_west;
    let b_tm = b_east < b_west;
    match (a_tm, b_tm) {
        // Neither wraps antimeridian -> interval overlap
        (false, false) => a_west <= b_east && b_west <= a_east,
        // Both wrap the antimeridian -> both include ±180 → always overlap
        (true, true) => true,
        // Exactly one wraps antimeridian
        // Wrapping box covers `[tw, 180] ∪ [-180, te]` -> other box `[nw, ne]` overlaps it if it reaches either segment.
        _ => {
            let (tw, te, nw, ne) = if a_tm {
                (a_west, a_east, b_west, b_east)
            } else {
                (b_west, b_east, a_west, a_east)
            };
            nw <= te || ne >= tw
        }
    }
}

/// Bounding Box of Cell, inflated by `scale` about its centre
///
/// Handles per-cell antimeridian wrap and the (degenerate) polar cells.
pub fn cell_bbox(cell: CellIndex, scale: f64) -> LlBBox {
    let res = cell.resolution();
    let boundary = cell.boundary();

    // Polar cells are degenerate in lat/lng (the pole is present at all longitudes).
    let north_pole = LatLng::new(90.0, 0.0)
        .expect("north pole is valid")
        .to_cell(res);
    let south_pole = LatLng::new(-90.0, 0.0)
        .expect("south pole is valid")
        .to_cell(res);
    if cell == north_pole {
        let south = boundary
            .iter()
            .map(|v| v.lat())
            .fold(f64::INFINITY, f64::min);
        return inflate(
            LlBBox {
                south,
                north: 90.0,
                west: -180.0,
                east: 180.0,
            },
            scale,
        );
    }
    if cell == south_pole {
        let north = boundary
            .iter()
            .map(|v| v.lat())
            .fold(f64::NEG_INFINITY, f64::max);
        return inflate(
            LlBBox {
                south: -90.0,
                north,
                west: -180.0,
                east: 180.0,
            },
            scale,
        );
    }

    let south = boundary
        .iter()
        .map(|v| v.lat())
        .fold(f64::INFINITY, f64::min);
    let north = boundary
        .iter()
        .map(|v| v.lat())
        .fold(f64::NEG_INFINITY, f64::max);
    let min_lng = boundary
        .iter()
        .map(|v| v.lng())
        .fold(f64::INFINITY, f64::min);
    let max_lng = boundary
        .iter()
        .map(|v| v.lng())
        .fold(f64::NEG_INFINITY, f64::max);

    let (west, east) = if max_lng - min_lng > 180.0 {
        // Transmeridian cell: recompute the bounds in a contiguous [0, 360) frame,
        // then re-encode into [-180, 180] (which yields east < west).
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for v in boundary.iter() {
            let l = if v.lng() < 0.0 {
                v.lng() + 360.0
            } else {
                v.lng()
            };
            lo = lo.min(l);
            hi = hi.max(l);
        }
        let wrap = |l: f64| if l > 180.0 { l - 360.0 } else { l };
        (wrap(lo), wrap(hi))
    } else {
        (min_lng, max_lng)
    };

    inflate(
        LlBBox {
            south,
            north,
            west,
            east,
        },
        scale,
    )
}

/// Inflate a box about its centre by `scale`
fn inflate(b: LlBBox, scale: f64) -> LlBBox {
    // Latitude
    let lat_c = (b.south + b.north) / 2.0;
    let lat_half = (b.north - b.south) / 2.0 * scale;
    let south = (lat_c - lat_half).max(-90.0);
    let north = (lat_c + lat_half).min(90.0);

    // Longitude
    let width = if b.is_transmeridian() {
        (b.east + 360.0) - b.west
    } else {
        b.east - b.west
    };
    let new_width = width * scale;
    if new_width >= 360.0 {
        // Inflated past the full domain → cover everything.
        return LlBBox {
            south,
            north,
            west: -180.0,
            east: 180.0,
        };
    }
    let lng_c = b.west + width / 2.0;
    let wrap = |l: f64| {
        let mut x = l;
        while x > 180.0 {
            x -= 360.0;
        }
        while x < -180.0 {
            x += 360.0;
        }
        x
    };
    let west = wrap(lng_c - new_width / 2.0);
    let east = wrap(lng_c + new_width / 2.0);
    LlBBox {
        south,
        north,
        west,
        east,
    }
}
