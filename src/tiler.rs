use crate::map::CoordMap;
use geo::{Coord, LineString, Polygon};
use h3o::CellIndex;

/// Cell's Hexagon as a Polygon
/// TODO - Could use from H3o but cell_boundary is not Public
pub fn cell_polygon(cell: CellIndex, coord_map: &CoordMap) -> Polygon {
    let ring: Vec<Coord> = coord_map.cellindex_boundary_ring(cell);
    Polygon::new(LineString::new(ring), Vec::new())
}
