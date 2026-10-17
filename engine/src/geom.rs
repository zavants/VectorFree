//! Vector output model.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn new(x: f64, y: f64) -> Self {
        Pt { x, y }
    }
    pub fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
    pub fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
    pub fn scale(self, s: f64) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
    pub fn dot(self, o: Pt) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dist(self, o: Pt) -> f64 {
        self.sub(o).len()
    }
    pub fn norm(self) -> Pt {
        let l = self.len();
        if l < 1e-12 {
            self
        } else {
            self.scale(1.0 / l)
        }
    }
}

/// A segment continuing from the previous end point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Seg {
    Line(Pt),
    Cubic(Pt, Pt, Pt),
}

impl Seg {
    pub fn end(&self) -> Pt {
        match *self {
            Seg::Line(p) | Seg::Cubic(_, _, p) => p,
        }
    }
}

/// A closed sub-path.
#[derive(Debug, Clone, Serialize)]
pub struct Path {
    pub start: Pt,
    pub segs: Vec<Seg>,
}

impl Path {
    /// Signed area (shoelace on the control polygon, good enough for sign and
    /// sorting). Positive = clockwise in y-down image coordinates.
    pub fn signed_area(&self) -> f64 {
        let mut a = 0.0;
        let mut prev = self.start;
        for s in &self.segs {
            let e = s.end();
            a += prev.x * e.y - e.x * prev.y;
            prev = e;
        }
        a += prev.x * self.start.y - self.start.x * prev.y;
        a * 0.5
    }

    pub fn bbox(&self) -> (Pt, Pt) {
        let mut lo = self.start;
        let mut hi = self.start;
        let mut grow = |p: Pt| {
            lo = Pt::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Pt::new(hi.x.max(p.x), hi.y.max(p.y));
        };
        for s in &self.segs {
            match *s {
                Seg::Line(p) => grow(p),
                Seg::Cubic(a, b, c) => {
                    grow(a);
                    grow(b);
                    grow(c);
                }
            }
        }
        (lo, hi)
    }

    /// Flatten to a polyline (closed; last point != first).
    pub fn flatten(&self, tolerance: f64) -> Vec<Pt> {
        let mut out = vec![self.start];
        let mut prev = self.start;
        for s in &self.segs {
            match *s {
                Seg::Line(p) => out.push(p),
                Seg::Cubic(a, b, c) => {
                    let est = prev.dist(a) + a.dist(b) + b.dist(c);
                    let n = ((est / tolerance.max(0.01)).sqrt() * 2.0).ceil().clamp(1.0, 64.0) as usize;
                    for i in 1..=n {
                        out.push(bezier(prev, a, b, c, i as f64 / n as f64));
                    }
                }
            }
            prev = s.end();
        }
        if out.len() > 1 && out.last().unwrap().dist(out[0]) < 1e-9 {
            out.pop();
        }
        out
    }
}

pub fn bezier(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f64) -> Pt {
    let u = 1.0 - t;
    p0.scale(u * u * u)
        .add(p1.scale(3.0 * u * u * t))
        .add(p2.scale(3.0 * u * t * t))
        .add(p3.scale(t * t * t))
}

/// One palette color's geometry.
#[derive(Debug, Clone, Serialize)]
pub struct Shape {
    pub rgb: [u8; 3],
    /// Fill opacity (255 = opaque).
    pub alpha: u8,
    /// Palette index this shape was traced from.
    pub label: u16,
    /// Outer boundaries (positive area).
    pub outers: Vec<Path>,
    /// Holes (negative area). With even-odd filling, outers+holes in one path
    /// reproduce the region exactly ("cutout" layout).
    pub holes: Vec<Path>,
    /// For each hole: true when the hole shows a transparent region and must
    /// be kept even in the "stacked" layout.
    pub hole_is_transparent: Vec<bool>,
    pub area: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct VectorImage {
    pub width: u32,
    pub height: u32,
    /// Shapes ordered largest area first (background first).
    pub shapes: Vec<Shape>,
}

impl Path {
    fn scale(&mut self, f: f64) {
        self.start = self.start.scale(f);
        for s in &mut self.segs {
            *s = match *s {
                Seg::Line(p) => Seg::Line(p.scale(f)),
                Seg::Cubic(a, b, c) => Seg::Cubic(a.scale(f), b.scale(f), c.scale(f)),
            };
        }
    }
}

impl VectorImage {
    /// Apply color/opacity edits without re-fitting any geometry. Shapes whose
    /// entry became transparent are dropped (re-vectorize to bring them back).
    pub fn apply_palette(&mut self, palette: &[crate::segment::PaletteEntry]) {
        self.shapes.retain(|s| palette.get(s.label as usize).is_some_and(|e| !e.transparent));
        for s in &mut self.shapes {
            let e = &palette[s.label as usize];
            s.rgb = e.rgb;
            s.alpha = e.alpha;
        }
    }

    pub fn scale(&mut self, f: f64) {
        for s in &mut self.shapes {
            for p in s.outers.iter_mut().chain(s.holes.iter_mut()) {
                p.scale(f);
            }
        }
    }
}
