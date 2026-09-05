# H3 CompactFill

This Library provides an early Algorithm for H3 pre-compacted polyfill.

The benefit of this algorithm is to bypass a 2-stage approach of polyfill + compact which is rather in-efficient as it requires materialising many more fine-resolution interior H3 Cells than are required in the final output.

To credit up front, this builds upon the great work by Uber in building [H3](https://h3geo.org/) and the excellent Rust Crate [h3o](https://github.com/HydroniumLabs/h3o) which implements the Core API.

## H3o Tiler

Whilst the H3o Tiler implementation is great, this could end up superseeding that implementation as a free sideeffect by just controlling whether or not to short-circuit at coarse cells vs uncompacting to children.

## Outstanding Work

- Fix all lat/lon geometries at the poles and transmeridian
- Investigate BBox (this seems to be how it's implemented in H3 C core) vs the current Boundary Disk (+ indexed R-Tree) for the coarse pruning
- Tests
- Benchmarks

## Algorithm

The basis of this algorithm is a recursive top-down (descent) search (very similar to Uber's H3 `polygonToCellsExperimental` Function). Where this deviates from `polygonToCellsExperimental` is that it can short-circuit at coarse cells to produce the compact output.

### Cell Containment

- Interior (fully-inside) Cells - short-circuit there and take these straight as the most compact output.
- Exterior (fully-outside) Cells - pruned from the search.
- Straddling - step to the next finest resolution and re-check all the children until the requested resolution is reached.

#### Bounding Disk

To assess Cell Containment, we use a Bounding Disk and not the Cell's Polygon. This is because H3 Cells don't directly nest children into parents, so instead we use an approximated disk of radius `1.2 (arbitrary scalar) × circumradius` about the centroid to ensure that the representation contains the cell and *every* one of it's children.

The Containment Algorithm is an R-Tree (`O(log edges)`) of the Polygon Edges: a nearest-neighbour query for the distance, and a +x ray cast for inside/outside.

#### Target Resolution

For Leaf Cells (Cells at the target Resolution), the exact Hexagon is used (children are no longer a concern).

| Containment Mode | Cell Check Criteria |
|---|---|
| `ContainsCentroid` | the cell centre is inside the polygon |
| `ContainsBoundary` | the polygon **covers** the cell (fully contained) |
| `IntersectsBoundary` / `Covers` | the polygon **intersects** the cell (any overlap) |

Only straddling (Intersecting) Cells are checked with the same DE-9IM `geo::relate` predicate as h3o's `Tiler`.

### Optimisations

#### Seeding

Instead of always starting with the 122 Resolution-0 Base Cells, a suitable starting point is chosen based on the given Polygon's size.

1. Calculate the finest resolution whose average Cell is still at least as large as the polygon's bounding box.
2. Generate a Cell Covering of the Boundary at that Resolution - using h3o's `Tiler` (implements `polygon_to_cells`) in `Covers` Containment Mode, dilated by one grid-ring (`grid_disk(1)`).
