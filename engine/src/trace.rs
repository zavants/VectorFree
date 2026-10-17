//! Boundary extraction on the pixel-crack grid.
//!
//! Every unit edge between two pixels with different labels is a boundary
//! edge. Vertices where three or more boundary edges meet are junctions; the
//! boundary is split into chains between junctions. Each chain separates
//! exactly two regions, so fitting it once gives both neighbours the same
//! curve: the output has no slivers between shapes.

use crate::fit::Curve;
use crate::geom::{Path, Pt, Seg, Shape, VectorImage};
use crate::segment::Segmentation;

/// Label used for "outside the image" and for every transparent region.
pub const OUTSIDE: u16 = u16::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    E,
    W,
    S,
    N,
}

impl Dir {
    fn delta(self) -> (i32, i32) {
        match self {
            Dir::E => (1, 0),
            Dir::W => (-1, 0),
            Dir::S => (0, 1),
            Dir::N => (0, -1),
        }
    }
    /// Direction obtained by turning left (in y-down screen coordinates the
    /// "left" of East is North).
    fn left(self) -> Dir {
        match self {
            Dir::E => Dir::N,
            Dir::N => Dir::W,
            Dir::W => Dir::S,
            Dir::S => Dir::E,
        }
    }
    fn from_delta(dx: i32, dy: i32) -> Dir {
        match (dx, dy) {
            (1, 0) => Dir::E,
            (-1, 0) => Dir::W,
            (0, 1) => Dir::S,
            _ => Dir::N,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chain {
    /// Integer grid vertices; for closed chains the first point is not repeated.
    pub pts: Vec<(i32, i32)>,
    /// Region on the left when walking from first to last point.
    pub left: u16,
    pub right: u16,
    pub closed: bool,
}

pub struct BoundaryGraph {
    pub width: i32,
    pub height: i32,
    pub chains: Vec<Chain>,
}

struct Grid<'a> {
    w: i32,
    h: i32,
    lab: Vec<u16>,
    _seg: &'a Segmentation,
}

impl<'a> Grid<'a> {
    fn new(seg: &'a Segmentation) -> Self {
        let lab = seg
            .labels
            .iter()
            .enumerate()
            .map(|(i, &l)| if seg.palette[l as usize].transparent || seg.is_hidden(i) { OUTSIDE } else { l })
            .collect();
        Grid { w: seg.width as i32, h: seg.height as i32, lab, _seg: seg }
    }

    #[inline]
    fn px(&self, x: i32, y: i32) -> u16 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            OUTSIDE
        } else {
            self.lab[(y * self.w + x) as usize]
        }
    }

    /// (left, right) labels for a unit step from vertex (x,y) in `d`.
    fn sides(&self, x: i32, y: i32, d: Dir) -> (u16, u16) {
        match d {
            Dir::E => (self.px(x, y - 1), self.px(x, y)),
            Dir::W => (self.px(x - 1, y), self.px(x - 1, y - 1)),
            Dir::S => (self.px(x, y), self.px(x - 1, y)),
            Dir::N => (self.px(x - 1, y - 1), self.px(x, y - 1)),
        }
    }

    fn is_edge(&self, x: i32, y: i32, d: Dir) -> bool {
        let (nx, ny) = (x + d.delta().0, y + d.delta().1);
        if nx < 0 || ny < 0 || nx > self.w || ny > self.h {
            return false;
        }
        let (l, r) = self.sides(x, y, d);
        l != r
    }

    /// Undirected edge id: horizontal edges first, then vertical.
    fn edge_id(&self, x: i32, y: i32, d: Dir) -> usize {
        let hcount = (self.w * (self.h + 1)) as usize;
        match d {
            Dir::E => (y * self.w + x) as usize,
            Dir::W => (y * self.w + x - 1) as usize,
            Dir::S => hcount + (y * (self.w + 1) + x) as usize,
            Dir::N => hcount + ((y - 1) * (self.w + 1) + x) as usize,
        }
    }

    fn degree(&self, x: i32, y: i32) -> usize {
        [Dir::E, Dir::W, Dir::S, Dir::N].iter().filter(|&&d| self.is_edge(x, y, d)).count()
    }
}

impl BoundaryGraph {
    pub fn build(seg: &Segmentation) -> Self {
        let g = Grid::new(seg);
        let (w, h) = (g.w, g.h);
        let nedges = (w * (h + 1) + (w + 1) * h) as usize;
        let mut visited = vec![false; nedges];
        let mut chains = Vec::new();
        let is_junction = |x: i32, y: i32| g.degree(x, y) > 2;

        let walk = |sx: i32, sy: i32, d0: Dir, visited: &mut Vec<bool>, stop_at_start: bool| -> Chain {
            let (left, right) = g.sides(sx, sy, d0);
            let mut pts = vec![(sx, sy)];
            let (mut x, mut y, mut d) = (sx, sy, d0);
            loop {
                visited[g.edge_id(x, y, d)] = true;
                x += d.delta().0;
                y += d.delta().1;
                if (x, y) == (sx, sy) && stop_at_start {
                    return Chain { pts, left, right, closed: true };
                }
                pts.push((x, y));
                if is_junction(x, y) {
                    return Chain { pts, left, right, closed: false };
                }
                // Degree-2 vertex: continue along the single other edge.
                let back = Dir::from_delta(-d.delta().0, -d.delta().1);
                let next = [Dir::E, Dir::W, Dir::S, Dir::N]
                    .into_iter()
                    .find(|&nd| nd != back && g.is_edge(x, y, nd))
                    .expect("degree-2 vertex has a continuation");
                d = next;
            }
        };

        // Open chains, starting from every junction.
        for y in 0..=h {
            for x in 0..=w {
                if !is_junction(x, y) {
                    continue;
                }
                for d in [Dir::E, Dir::W, Dir::S, Dir::N] {
                    if g.is_edge(x, y, d) && !visited[g.edge_id(x, y, d)] {
                        chains.push(walk(x, y, d, &mut visited, false));
                    }
                }
            }
        }
        // Remaining edges form closed loops without junctions (islands).
        for y in 0..=h {
            for x in 0..=w {
                for d in [Dir::E, Dir::S] {
                    if g.is_edge(x, y, d) && !visited[g.edge_id(x, y, d)] {
                        chains.push(walk(x, y, d, &mut visited, true));
                    }
                }
            }
        }
        BoundaryGraph { width: w, height: h, chains }
    }
}

/// A chain used in a region loop, possibly reversed.
#[derive(Clone, Copy)]
struct Use {
    chain: usize,
    rev: bool,
}

fn endpoints(c: &Chain, rev: bool) -> ((i32, i32), (i32, i32)) {
    let (a, b) = (c.pts[0], *c.pts.last().unwrap());
    if rev { (b, a) } else { (a, b) }
}

/// Direction of the first step out of the start vertex and last step into the end.
fn dirs(c: &Chain, rev: bool) -> (Dir, Dir) {
    let n = c.pts.len();
    let d = |a: (i32, i32), b: (i32, i32)| Dir::from_delta(b.0 - a.0, b.1 - a.1);
    if !rev {
        (d(c.pts[0], c.pts[1]), d(c.pts[n - 2], c.pts[n - 1]))
    } else {
        (d(c.pts[n - 1], c.pts[n - 2]), d(c.pts[1], c.pts[0]))
    }
}

fn reverse_segs(start: Pt, segs: &[Seg]) -> (Pt, Vec<Seg>) {
    let end = segs.last().map_or(start, |s| s.end());
    let mut out = Vec::with_capacity(segs.len());
    for i in (0..segs.len()).rev() {
        let target = if i == 0 { start } else { segs[i - 1].end() };
        out.push(match segs[i] {
            Seg::Line(_) => Seg::Line(target),
            Seg::Cubic(a, b, _) => Seg::Cubic(b, a, target),
        });
    }
    (end, out)
}

/// Build shapes from chains and their fitted curves (`curves[i]` traces chain i
/// in its own direction).
pub fn assemble(seg: &Segmentation, graph: &BoundaryGraph, curves: &[Curve]) -> VectorImage {
    let areas = seg.label_area();
    let any_transparent = seg.palette.iter().zip(&areas).any(|(e, &a)| e.transparent && a > 0);
    let mut shapes = Vec::new();
    // Group chain uses by label.
    let mut uses: Vec<Vec<Use>> = vec![Vec::new(); seg.palette.len()];
    for (i, c) in graph.chains.iter().enumerate() {
        if c.left != OUTSIDE {
            uses[c.left as usize].push(Use { chain: i, rev: false });
        }
        if c.right != OUTSIDE {
            uses[c.right as usize].push(Use { chain: i, rev: true });
        }
    }
    for (label, list) in uses.iter().enumerate() {
        let entry = &seg.palette[label];
        if entry.transparent || list.is_empty() {
            continue;
        }
        let loops = link_loops(graph, list);
        let mut shape = Shape {
            rgb: entry.rgb,
            alpha: entry.alpha,
            label: label as u16,
            outers: vec![],
            holes: vec![],
            hole_is_transparent: vec![],
            area: areas[label],
        };
        for lp in loops {
            let grid_area = loop_grid_area(graph, &lp);
            let path = build_path(curves, &lp);
            // With "label on the left" walking, outer loops have negative
            // shoelace area in y-down coordinates.
            if grid_area < 0 {
                shape.outers.push(path);
            } else {
                shape.holes.push(path);
                shape.hole_is_transparent.push(any_transparent && encloses_transparent(seg, graph, &lp));
            }
        }
        shapes.push(shape);
    }
    shapes.sort_by(|a, b| b.area.cmp(&a.area));
    VectorImage { width: seg.width, height: seg.height, shapes }
}

fn link_loops(graph: &BoundaryGraph, list: &[Use]) -> Vec<Vec<Use>> {
    use std::collections::HashMap;
    let mut loops = Vec::new();
    let mut used = vec![false; list.len()];
    let mut by_start: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (k, u) in list.iter().enumerate() {
        let c = &graph.chains[u.chain];
        if c.closed {
            used[k] = true;
            loops.push(vec![*u]);
        } else {
            by_start.entry(endpoints(c, u.rev).0).or_default().push(k);
        }
    }
    for k0 in 0..list.len() {
        if used[k0] {
            continue;
        }
        used[k0] = true;
        let start_v = endpoints(&graph.chains[list[k0].chain], list[k0].rev).0;
        let mut lp = vec![list[k0]];
        let mut cur = k0;
        loop {
            let (_, end_v) = endpoints(&graph.chains[list[cur].chain], list[cur].rev);
            let (_, in_dir) = dirs(&graph.chains[list[cur].chain], list[cur].rev);
            let cands: Vec<usize> = by_start.get(&end_v).map_or(vec![], |v| v.iter().copied().filter(|&k| !used[k]).collect());
            if cands.is_empty() {
                break; // back at start (or malformed; close anyway)
            }
            // Prefer the left-most turn: keeps each loop hugging 4-connected
            // pixels at checkerboard junctions so loops never self-cross.
            let rank = |k: usize| {
                let (out_dir, _) = dirs(&graph.chains[list[k].chain], list[k].rev);
                if out_dir == in_dir.left() {
                    0
                } else if out_dir == in_dir {
                    1
                } else {
                    2
                }
            };
            let next = *cands.iter().min_by_key(|&&k| rank(k)).unwrap();
            // Back at the start vertex: close if our own first chain is the
            // preferred continuation, otherwise keep hugging.
            if end_v == start_v && rank(k0) <= rank(next) {
                break;
            }
            used[next] = true;
            lp.push(list[next]);
            cur = next;
        }
        loops.push(lp);
    }
    loops
}

/// Does the area enclosed by a (hole) loop contain any transparent pixel?
/// In the stacked layout such holes must stay open even when other shapes are
/// drawn inside them, or the shape would show through the transparency.
fn encloses_transparent(seg: &Segmentation, graph: &BoundaryGraph, lp: &[Use]) -> bool {
    // Collect the loop's vertical unit edges per pixel row; a scanline through
    // pixel centers crosses exactly those.
    let mut rows: std::collections::BTreeMap<i32, Vec<i32>> = std::collections::BTreeMap::new();
    for u in lp {
        let c = &graph.chains[u.chain];
        let n = c.pts.len();
        let count = if c.closed { n } else { n - 1 };
        for i in 0..count {
            let (a, b) = (c.pts[i], c.pts[(i + 1) % n]);
            if a.0 == b.0 {
                rows.entry(a.1.min(b.1)).or_default().push(a.0);
            }
        }
    }
    for (y, mut xs) in rows {
        xs.sort_unstable();
        for pair in xs.chunks(2) {
            if let [x0, x1] = *pair {
                for x in x0..x1 {
                    let l = seg.labels[(y as u32 * seg.width + x as u32) as usize];
                    if seg.palette[l as usize].transparent {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn loop_grid_area(graph: &BoundaryGraph, lp: &[Use]) -> i64 {
    let mut a: i64 = 0;
    for u in lp {
        let c = &graph.chains[u.chain];
        let mut pts: Vec<(i32, i32)> = c.pts.clone();
        if c.closed {
            pts.push(pts[0]);
        }
        if u.rev {
            pts.reverse();
        }
        for w in pts.windows(2) {
            a += w[0].0 as i64 * w[1].1 as i64 - w[1].0 as i64 * w[0].1 as i64;
        }
    }
    a
}

fn build_path(curves: &[Curve], lp: &[Use]) -> Path {
    let mut start = None;
    let mut segs = Vec::new();
    for u in lp {
        let (s0, ref fwd) = curves[u.chain];
        let (s, part) = if u.rev { reverse_segs(s0, fwd) } else { (s0, fwd.clone()) };
        if start.is_none() {
            start = Some(s);
        }
        segs.extend(part);
    }
    Path { start: start.unwrap_or(Pt::new(0.0, 0.0)), segs }
}
