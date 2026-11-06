//! A dressing set: which pieces dress a stage, and every number about them.
//! Each set is a RON file in `assets/dressing/`; the passes are the same for
//! every set. Distances are world pixels (48 px = 1 m), colors sRGB 0..1.

use serde::Deserialize;

use crate::visuals::tuning::Rgb;
use crate::visuals::wfc::Weights;

/// sRGB with alpha.
pub type Rgba = (f32, f32, f32, f32);

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct DressingSet {
    pub name: String,
    pub background: Rgb,
    pub body: Body,
    pub interior: Interior,
    pub strips: Strips,
    pub corners: Corners,
    pub hanging: Hanging,
    pub skyline: Skyline,
}

/// How a solid is colored by how deep inside it a point is.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Body {
    pub fill: Rgb,
    /// The band along every exposed edge.
    pub trim: Rgb,
    pub trim_depth: f32,
    /// A darker line where the band meets the fill.
    pub seam: Rgb,
    pub seam_width: f32,
    /// Ragged band edge: amplitude and feature size of the noise.
    pub wobble: f32,
    pub wobble_scale: f32,
    /// Panels: size, and how much each one's shade varies.
    pub panel: (f32, f32),
    pub panel_variation: f32,
    /// Fine speckle: amount and grain size.
    pub grain: f32,
    pub grain_scale: f32,
    /// Darker deeper inside: amount, and the depth where it is full.
    pub depth_darken: f32,
    pub depth_range: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum InteriorStyle {
    /// Straight square-ended bars.
    Girders,
    /// Curved strokes of varying width.
    Roots,
}

/// The WFC fill deep inside solids.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Interior {
    pub style: InteriorStyle,
    pub color: Rgb,
    pub width: (f32, f32),
    /// Roots only: how far the middle of a stroke may bend.
    pub bend: f32,
    pub cell: f32,
    /// Cells must sit at least this far beyond the trim band.
    pub margin: f32,
    pub weights: Weights,
    /// Dots in empty cells: bolts or pebbles.
    pub dot_chance: f32,
    pub dot: Dot,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Dot {
    pub radius: (f32, f32),
    /// Width over height; 1 is round.
    pub squash: f32,
    pub color: Rgb,
    /// Drawn offset under the dot; alpha 0 for none.
    pub shadow: Rgba,
}

/// What dresses each kind of edge. Every piece listed is laid along it.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Strips {
    pub floor: Vec<Piece>,
    pub slope: Vec<Piece>,
    pub wall: Vec<Piece>,
    pub ceiling: Vec<Piece>,
    pub arc_concave: Vec<Piece>,
    pub arc_convex: Vec<Piece>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum Piece {
    /// Plates of random length with a seam between, a bolt near each end,
    /// and sometimes a vent instead (never two vents in a row).
    Plates {
        length: (f32, f32),
        seam: Rgb,
        seam_width: f32,
        vent_chance: f32,
        vent_length: f32,
        slit_spacing: f32,
        slit: Rgb,
        slit_width: f32,
        bolt_inset: f32,
        bolt: Dot,
    },
    /// Dots along the middle of the band.
    Bolts { spacing: f32, depth: f32, dot: Dot },
    /// Short ticks across the band (anti-slip ribs, radial ribs).
    Ribs {
        spacing: f32,
        from: f32,
        to: f32,
        slant: f32,
        width: f32,
        color: Rgb,
    },
    /// Lines that follow the edge at fixed depths.
    Hoops {
        depths: Vec<f32>,
        width: f32,
        color: Rgb,
    },
    /// Blades growing out of the edge.
    Grass {
        spacing: f32,
        length: (f32, f32),
        lean: f32,
        width: f32,
        colors: Vec<Rgb>,
        /// Only where the edge faces up at least this much (normal y).
        min_up: f32,
    },
    /// Small mushrooms standing on the edge.
    Mushrooms {
        spacing: (f32, f32),
        chance: f32,
        size: (f32, f32),
        stem: Rgb,
        cap: Rgb,
        spot: Rgb,
    },
    /// Strands hanging from the edge.
    Fringe {
        spacing: f32,
        length: (f32, f32),
        width: f32,
        sway: f32,
        color: Rgb,
        min_down: f32,
    },
    /// Leaf dabs on the edge.
    Leaves {
        spacing: f32,
        chance: f32,
        size: (f32, f32),
        colors: Vec<Rgb>,
    },
    /// Cracks reaching into the solid.
    Cracks {
        spacing: f32,
        depth: (f32, f32),
        width: f32,
        color: Rgb,
    },
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Corners {
    pub outer: Vec<CornerPiece>,
    pub inner: Vec<CornerPiece>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum CornerPiece {
    /// A square cap turned to sit on the corner, clipped to the solid.
    Cap {
        size: f32,
        inset: f32,
        color: Rgb,
        edge: Rgb,
        edge_width: f32,
        bolt_depth: f32,
        bolt: Dot,
    },
    /// A quarter-round bracket welded into the corner, clipped to the solid.
    Bracket {
        radius: f32,
        spread: f32,
        color: Rgb,
        edge: Rgb,
        edge_width: f32,
        bolts: usize,
        bolt: Dot,
    },
    /// A fan of blades out of the corner, into the open.
    Tuft {
        blades: usize,
        spread: f32,
        length: (f32, f32),
        width: f32,
        colors: Vec<Rgb>,
    },
    /// Fern fronds out of the corner, into the open.
    Fern {
        fronds: Vec<(f32, f32)>,
        width: f32,
        leaflet: f32,
        color: Rgb,
    },
    /// A few mushrooms at the foot of an inner corner.
    Mushrooms {
        count: usize,
        size: (f32, f32),
        stem: Rgb,
        cap: Rgb,
        spot: Rgb,
    },
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Hanging {
    /// Distance between anchors along ceilings.
    pub spacing: (f32, f32),
    /// Anchors only where the edge faces down at least this much.
    pub min_down: f32,
    /// Keep this much clear above the floor below.
    pub floor_gap: f32,
    pub kinds: Vec<(Hang, f32)>,
    /// Sway: angle (radians) and speed.
    pub sway: f32,
    pub sway_speed: f32,
    pub motes: Motes,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum Hang {
    Chain {
        length: (f32, f32),
        link: f32,
        width: f32,
        color: Rgb,
        hook: f32,
    },
    /// A cord with leaves along it.
    Vine {
        length: (f32, f32),
        width: f32,
        color: Rgb,
        leaves: LeafRow,
    },
    /// A cord ending in a light: a shade with a cone of light, or a pod with a halo.
    Light {
        length: (f32, f32),
        cord: Rgb,
        cord_width: f32,
        shade: Option<(f32, Rgb)>,
        bulb: (f32, f32, Rgb),
        glow: Rgba,
        glow_size: f32,
        /// Cone of light below (lamps) instead of a round halo (pods).
        cone: bool,
        pulse: f32,
    },
    /// A sagging line to the next anchor along the ceiling.
    Span {
        reach: f32,
        sag: (f32, f32),
        width: f32,
        color: Rgb,
        leaves: Option<LeafRow>,
    },
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct LeafRow {
    pub spacing: f32,
    pub size: (f32, f32),
    pub colors: Vec<Rgb>,
}

/// Small glowing specks drifting in the open (fireflies, dust).
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Motes {
    pub count: usize,
    pub radius: f32,
    pub color: Rgba,
    pub drift: f32,
    pub twinkle: f32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Skyline {
    /// Painted back to front.
    pub layers: Vec<SkyLayer>,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct SkyLayer {
    pub color: Rgb,
    /// 0 = fixed to the level, 1 = fixed to the screen.
    pub parallax: f32,
    /// Lot width and gap between lots.
    pub lot: (f32, f32),
    pub gap: (f32, f32),
    pub structures: Vec<(Structure, f32)>,
    /// Chance to bridge two neighbouring towers.
    pub bridge_chance: f32,
    pub bridge: Span2,
    /// Long spans across the room, held by columns.
    pub spans: (u32, u32),
    pub span_height: (f32, f32),
    pub span_length: (f32, f32),
    pub span: Span2,
    pub column: Column,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum Structure {
    Tank {
        height: (f32, f32),
        width: (f32, f32),
        hoop_spacing: f32,
        hoop_width: f32,
        hoop: Rgb,
    },
    Stack {
        height: (f32, f32),
        width: (f32, f32),
        lip: f32,
    },
    Tower {
        height: (f32, f32),
        width: (f32, f32),
        line: f32,
    },
    Pine {
        height: (f32, f32),
        width: (f32, f32),
        tiers: usize,
    },
    Tree {
        height: (f32, f32),
        width: (f32, f32),
        blobs: usize,
    },
    Snag {
        height: (f32, f32),
        width: (f32, f32),
        branches: usize,
        line: f32,
    },
    Gap,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum Span2 {
    Truss {
        depth: f32,
        line: f32,
    },
    Branch {
        width: f32,
        sag: f32,
        leaf_radius: (f32, f32),
        leaf_spacing: f32,
    },
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub enum Column {
    Lattice { width: f32, line: f32 },
    Trunk { width: f32, flare: f32 },
}

pub const BUILT_IN: [(&str, &str); 2] = [
    (
        "industrial",
        include_str!("../../../assets/dressing/industrial.ron"),
    ),
    (
        "forest",
        include_str!("../../../assets/dressing/forest.ron"),
    ),
];

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_sets_parse() {
        for (name, text) in super::BUILT_IN {
            if let Err(err) = ron::from_str::<super::DressingSet>(text) {
                panic!("{name}: {err}");
            }
        }
    }
}
