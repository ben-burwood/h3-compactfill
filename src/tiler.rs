use geo::{Coord, LineString, Polygon};
use h3o::CellIndex;

/// Cell's Hexagon as a Polygon
/// TODO - Could use from H3o but cell_boundary is not Public
pub fn cell_polygon(cell: CellIndex) -> Polygon {
    let ring: Vec<Coord> = cell
        .boundary()
        .iter()
        .map(|ll| Coord {
            x: ll.lng(),
            y: ll.lat(),
        })
        .collect();
    Polygon::new(LineString::new(ring), Vec::new())
}
