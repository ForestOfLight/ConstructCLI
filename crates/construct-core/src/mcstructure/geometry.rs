//! Sizes, origins, and the flattened block index.
//!
//! A `.mcstructure` stores its blocks in one flat list per layer, in **ZYX**
//! order: `index = SZ*SY*X + SZ*Y + Z`. Getting this order wrong produces a
//! structure that loads without error and is transposed, so it is isolated
//! here and tested exhaustively rather than open-coded at each use.

/// A block coordinate, also used for world origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A structure's dimensions in blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Size {
    /// Total block count. Computed in `i64`: sizes come from a file this tool
    /// did not write, and a wrapped `i32` would silently under-allocate.
    /// Saturates at `i64::MAX` rather than overflowing: an absurd volume is
    /// refused downstream anyway (decoder compares against file's declared layer
    /// length, merge compares against a max-volume cap).
    pub fn volume(&self) -> i64 {
        i64::from(self.x.max(0))
            .saturating_mul(i64::from(self.y.max(0)))
            .saturating_mul(i64::from(self.z.max(0)))
    }

    /// The flattened index of a coordinate, or `None` if it lies outside.
    pub fn index_of(&self, c: Coord) -> Option<usize> {
        if c.x < 0 || c.y < 0 || c.z < 0 || c.x >= self.x || c.y >= self.y || c.z >= self.z {
            return None;
        }
        let sz = i64::from(self.z);
        let sy = i64::from(self.y);
        let cx = i64::from(c.x);
        let cy = i64::from(c.y);
        let cz = i64::from(c.z);
        let i = sz
            .checked_mul(sy)?
            .checked_mul(cx)?
            .checked_add(sz.checked_mul(cy)?)?
            .checked_add(cz)?;
        usize::try_from(i).ok()
    }

    /// The coordinate a flattened index refers to, or `None` if out of range.
    pub fn coord_of(&self, i: usize) -> Option<Coord> {
        let i = i64::try_from(i).ok()?;
        if i < 0 || i >= self.volume() {
            return None;
        }
        let sz = i64::from(self.z);
        let sy = i64::from(self.y);
        Some(Coord {
            x: (i / sz / sy) as i32,
            y: (i / sz % sy) as i32,
            z: (i % sz) as i32,
        })
    }
}

/// A half-open box in world space: `min` inclusive, `max_exclusive` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    pub min: Coord,
    pub max_exclusive: Coord,
}

impl BoundingBox {
    pub fn of(origin: Coord, size: Size) -> Self {
        Self {
            min: origin,
            max_exclusive: Coord {
                x: origin.x.saturating_add(size.x),
                y: origin.y.saturating_add(size.y),
                z: origin.z.saturating_add(size.z),
            },
        }
    }

    pub fn size(&self) -> Size {
        Size {
            x: self.max_exclusive.x.saturating_sub(self.min.x),
            y: self.max_exclusive.y.saturating_sub(self.min.y),
            z: self.max_exclusive.z.saturating_sub(self.min.z),
        }
    }

    /// The smallest box containing all of `boxes`, or `None` when empty.
    pub fn union(boxes: &[BoundingBox]) -> Option<BoundingBox> {
        let mut it = boxes.iter();
        let first = *it.next()?;
        Some(it.fold(first, |acc, b| BoundingBox {
            min: Coord {
                x: acc.min.x.min(b.min.x),
                y: acc.min.y.min(b.min.y),
                z: acc.min.z.min(b.min.z),
            },
            max_exclusive: Coord {
                x: acc.max_exclusive.x.max(b.max_exclusive.x),
                y: acc.max_exclusive.y.max(b.max_exclusive.y),
                z: acc.max_exclusive.z.max(b.max_exclusive.z),
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_order_is_zyx() {
        // From docs/bedrock-mcstructure-files.md: index = SZ*SY*X + SZ*Y + Z.
        // A 2x3x4 structure: z varies fastest, then y, then x.
        let s = Size { x: 2, y: 3, z: 4 };
        assert_eq!(s.index_of(Coord { x: 0, y: 0, z: 0 }), Some(0));
        assert_eq!(s.index_of(Coord { x: 0, y: 0, z: 1 }), Some(1));
        assert_eq!(s.index_of(Coord { x: 0, y: 1, z: 0 }), Some(4));
        assert_eq!(s.index_of(Coord { x: 1, y: 0, z: 0 }), Some(12));
        assert_eq!(s.index_of(Coord { x: 1, y: 2, z: 3 }), Some(23));
    }

    #[test]
    fn every_index_round_trips_through_coordinates() {
        let s = Size { x: 3, y: 5, z: 7 };
        for i in 0..s.volume() as usize {
            let c = s.coord_of(i).expect("index inside the volume must convert");
            assert_eq!(s.index_of(c), Some(i), "index {i} did not round-trip");
        }
    }

    #[test]
    fn coordinates_outside_the_size_have_no_index() {
        let s = Size { x: 2, y: 2, z: 2 };
        assert_eq!(s.index_of(Coord { x: 2, y: 0, z: 0 }), None);
        assert_eq!(s.index_of(Coord { x: 0, y: -1, z: 0 }), None);
        assert_eq!(s.coord_of(8), None);
    }

    #[test]
    fn a_bounding_box_spans_origin_to_origin_plus_size() {
        let b = BoundingBox::of(Coord { x: 10, y: 0, z: -5 }, Size { x: 2, y: 3, z: 4 });
        assert_eq!(b.min, Coord { x: 10, y: 0, z: -5 });
        assert_eq!(b.max_exclusive, Coord { x: 12, y: 3, z: -1 });
        assert_eq!(b.size(), Size { x: 2, y: 3, z: 4 });
    }

    #[test]
    fn a_union_covers_every_box_including_negative_coordinates() {
        let a = BoundingBox::of(Coord { x: 0, y: 0, z: 0 }, Size { x: 2, y: 2, z: 2 });
        let b = BoundingBox::of(Coord { x: -3, y: 5, z: 1 }, Size { x: 1, y: 1, z: 1 });
        let u = BoundingBox::union(&[a, b]).unwrap();
        assert_eq!(u.min, Coord { x: -3, y: 0, z: 0 });
        assert_eq!(u.max_exclusive, Coord { x: 2, y: 6, z: 2 });
        assert_eq!(u.size(), Size { x: 5, y: 6, z: 2 });
    }

    #[test]
    fn a_union_of_nothing_is_nothing() {
        assert_eq!(BoundingBox::union(&[]), None);
    }

    #[test]
    fn a_volume_that_overflows_i32_is_still_computed_in_i64() {
        // 2000^3 is 8e9, far past i32. Sizes come from a file we did not write,
        // so the arithmetic must not wrap silently.
        let s = Size {
            x: 2000,
            y: 2000,
            z: 2000,
        };
        assert_eq!(s.volume(), 8_000_000_000);
    }

    #[test]
    fn a_size_with_max_dimensions_saturates_volume_and_index_doesnt_panic() {
        // i32::MAX^3 = 9.9×10²⁷ far exceeds i64::MAX = 9.2×10¹⁸.
        // volume() must saturate to i64::MAX, and index_of() must not panic.
        let s = Size {
            x: i32::MAX,
            y: i32::MAX,
            z: i32::MAX,
        };
        assert_eq!(s.volume(), i64::MAX);
        // Test that index_of on an in-range coordinate either returns None or
        // a correct value, but does not panic.
        let result = s.index_of(Coord { x: 0, y: 0, z: 0 });
        // With i32::MAX dimensions, the coordinate (0,0,0) is in range and
        // should produce index 0.
        assert_eq!(result, Some(0));
    }

    #[test]
    fn a_bounding_box_spanning_extreme_coordinates_saturates_size_and_doesnt_panic() {
        // i32::MAX - i32::MIN = 4294967295, which overflows i32::MAX.
        // size() must use saturating_sub to avoid panic.
        let b = BoundingBox {
            min: Coord {
                x: i32::MIN,
                y: i32::MIN,
                z: i32::MIN,
            },
            max_exclusive: Coord {
                x: i32::MAX,
                y: i32::MAX,
                z: i32::MAX,
            },
        };
        let size = b.size();
        // With saturating_sub, each dimension saturates to i32::MAX.
        assert_eq!(size.x, i32::MAX);
        assert_eq!(size.y, i32::MAX);
        assert_eq!(size.z, i32::MAX);
    }

    #[test]
    fn a_size_with_negative_dimensions_has_zero_volume_and_rejects_all_coordinates() {
        // Negative dimensions are invalid but must not panic.
        // volume() treats them as 0 (via .max(0)).
        // index_of() rejects all coordinates via the boundary guard.
        let s = Size { x: -5, y: 3, z: 2 };
        assert_eq!(s.volume(), 0);
        assert_eq!(s.index_of(Coord { x: 0, y: 0, z: 0 }), None);
        assert_eq!(s.index_of(Coord { x: 1, y: 1, z: 1 }), None);
    }
}
