//! Turn pixel-staircase chains into smooth cubic Beziers.
//!
//! 1. Detect corners (sharp turns at a scale of a few pixels).
//! 2. Taubin-smooth the points between fixed points (corners, chain ends),
//!    which straightens staircases without shrinking curves.
//! 3. Fit cubic Beziers with Schneider's least-squares algorithm
//!    ("An Algorithm for Automatically Fitting Digitized Curves", Graphics Gems, 1990).

use crate::geom::{bezier, Pt, Seg};
use crate::snap::Snapper;
use crate::trace::{BoundaryGraph, Chain};
use crate::{CurveMode, Params};

/// Fitted curve for one chain: start point plus segments.
pub type Curve = (Pt, Vec<Seg>);

pub fn fit_chains(graph: &BoundaryGraph, p: &Params, snapper: Option<&Snapper>) -> Vec<Curve> {
    graph
        .chains
        .iter()
        .map(|c| {
            let pts: Vec<Pt> = c.pts.iter().map(|&(x, y)| Pt::new(x as f64, y as f64)).collect();
            fit_polyline(&pts, c, p, snapper)
        })
        .collect()
}

fn fit_polyline(grid: &[Pt], chain: &Chain, p: &Params, snapper: Option<&Snapper>) -> Curve {
    let closed = chain.closed;
    let n = grid.len();
    if !closed && n <= 2 {
        return (grid[0], vec![Seg::Line(grid[n - 1])]);
    }
    // Corners are found on the pixel grid, where they are unambiguous.
    // Supersampling and anti-aliasing round them off a little, so turns up
    // to LOOSE_DEG wider than the limit are only candidates, decided below
    // from the sub-pixel arms.
    let found = detect_corners(grid, closed, p.corner_angle_deg, p.corner_span);
    let loose: Vec<usize> = found.iter().filter(|c| c.1 > p.corner_angle_deg).map(|c| c.0).collect();
    let mut corners: Vec<usize> = found.iter().map(|c| c.0).collect();
    // Then move points to their sub-pixel edge positions, except right at
    // corners: the anti-aliasing iso-line rounds them off, and corner
    // sharpening rebuilds them from the (snapped) straight arms instead.
    let mut snapped = grid.to_vec();
    if let Some(s) = snapper {
        let snap = |corners: &[usize]| {
            let margin = (p.corner_span / 3).max(1) as isize;
            let mut keep = vec![false; n];
            for &c in corners {
                for o in -margin..=margin {
                    let j = c as isize + o;
                    let j = if closed { j.rem_euclid(n as isize) } else { j };
                    if (0..n as isize).contains(&j) {
                        keep[j as usize] = true;
                    }
                }
            }
            let mut snapped = grid.to_vec();
            s.snap(chain, &mut snapped, &keep);
            snapped
        };
        snapped = snap(&corners);
        if !loose.is_empty() {
            // A candidate is a corner if its straight sub-pixel arms meet at
            // less than the limit angle.
            let cos_thr = p.corner_angle_deg.to_radians().cos();
            let all = corners.clone();
            corners.retain(|c| {
                if !loose.contains(c) {
                    return true;
                }
                let i = all.iter().position(|a| a == c).unwrap();
                match corner_arms(&snapped, closed, &all, i, p.corner_span) {
                    Some(((ca, da, ra), (cb, db, rb))) if ra <= 0.45 && rb <= 0.45 => {
                        let out = |c0: Pt, d: Pt| if d.dot(c0.sub(snapped[*c])) < 0.0 { d.scale(-1.0) } else { d };
                        out(ca, da).dot(out(cb, db)) >= cos_thr
                    }
                    _ => false,
                }
            });
            if corners.len() < all.len() {
                snapped = snap(&corners);
            }
        }
    } else {
        // Nothing sub-pixel to decide candidates with.
        corners.retain(|c| !loose.contains(c));
    }
    let sharpened;
    let raw: &[Pt] = if corners.is_empty() {
        &snapped
    } else {
        sharpened = sharpen_corners(&snapped, closed, &corners, p.corner_span);
        &sharpened[..]
    };
    // Rotate closed chains so they start at a corner (if any).
    let (mut pts, closed_smooth) = if closed {
        let start = corners.first().copied().unwrap_or(0);
        let mut v: Vec<Pt> = raw[start..].iter().chain(&raw[..start]).copied().collect();
        corners = corners.iter().map(|&c| (c + n - start) % n).collect();
        v.push(v[0]);
        (v, corners.is_empty())
    } else {
        (raw.to_vec(), false)
    };
    let m = pts.len();
    let mut fixed = vec![false; m];
    fixed[0] = true;
    fixed[m - 1] = true;
    for &c in &corners {
        fixed[c] = true;
    }
    if closed_smooth {
        // No real corner: the seam is free, smooth it cyclically.
        fixed[0] = false;
        fixed[m - 1] = false;
    }
    taubin(&mut pts, &fixed, closed_smooth, p.smooth_passes);
    if closed_smooth {
        let last = m - 1;
        pts[last] = pts[0];
    }

    let mut breaks: Vec<usize> = (0..m).filter(|&i| fixed[i]).collect();
    if closed_smooth {
        breaks = vec![0, m - 1];
    }
    let tol2 = p.fit_tolerance * p.fit_tolerance;
    // Sub-pixel snapping leaves up to ~0.1px of periodic ripple along straight
    // edges; on small images the fitting tolerance drops below that and a
    // straight side would be fitted as a chain of wobbly cubics.
    let straight_tol = (p.fit_tolerance * 0.75).max(if snapper.is_some() { 0.15 * p.corner_span as f64 / 4.0 } else { 0.0 });
    let runs = if p.clean_lines && p.curves == CurveMode::Curves {
        let mut sharp = vec![false; m];
        for &c in &corners {
            sharp[c] = true;
        }
        sharp[m - 1] = closed && !closed_smooth;
        place_lines(&mut pts, &mut breaks, &sharp, closed, closed_smooth, &line_tol(p, straight_tol))
    } else {
        Vec::new()
    };
    let run_from = |i: usize| runs.iter().find(|r| r.0 == i).copied();
    let run_to = |i: usize| runs.iter().find(|r| r.1 == i).copied();
    let mut segs = Vec::new();
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = &pts[a..=b];
        if d.len() <= 2 || run_from(a) == Some((a, b)) || is_straight(d, straight_tol) {
            segs.push(Seg::Line(d[d.len() - 1]));
            continue;
        }
        if p.curves == CurveMode::Lines {
            simplify_lines(d, p.fit_tolerance, &mut segs);
            continue;
        }
        let seam = pts[1].sub(pts[m - 2]).norm();
        let mut t1 = if closed_smooth && a == 0 { seam } else { left_tangent(d) };
        let mut t2 = if closed_smooth && b == m - 1 { seam.scale(-1.0) } else { right_tangent(d) };
        // Curves leave and enter straight stretches along them.
        if let Some((s, _)) = run_to(a) {
            t1 = pts[a].sub(pts[s]).norm();
        }
        let next = if closed_smooth && b == m - 1 { 0 } else { b };
        if let Some((_, e)) = run_from(next) {
            t2 = pts[next].sub(pts[e]).norm();
        }
        let from = segs.len();
        fit_cubic(d, 0, d.len() - 1, t1, t2, tol2, &mut segs, 0);
        straighten(&mut segs[from..], d[0], p.fit_tolerance * 0.08);
    }
    (pts[0], segs)
}

/// Thresholds for straight stretches, in segmentation pixels.
struct LineTol {
    /// Largest distance of a point from the line.
    tol: f64,
    /// Largest sagitta of a parabola through the points: a gentle arc stays
    /// within `tol` over a short length, but bends measurably.
    sag: f64,
    min_len: f64,
    /// Stretches this close to each other or to a section end touch it.
    gap: f64,
    /// A stretch also reaches a section end through a short remainder that
    /// is this straight with it (corners left a little rounded).
    reach: f64,
}

fn line_tol(p: &Params, straight_tol: f64) -> LineTol {
    let up = p.corner_span as f64 / 4.0;
    let tol = (0.6 * p.fit_tolerance).max(0.2 * up);
    LineTol { tol, sag: 0.4 * tol, min_len: 6.0 * up, gap: 1.5 * up, reach: straight_tol }
}

/// Clean-lines mode: find the straight stretches of the sections between
/// `breaks`, move their end points onto their fitted lines and add them to
/// `breaks`. Chain ends stay put; a corner (`sharp`) moves only to where the
/// stretches on both sides of it meet. A closed loop without corners is first
/// rotated to start at a straight stretch, so the seam does not cut one in
/// two. Returns the stretches as (first, last) point indices.
fn place_lines(pts: &mut Vec<Pt>, breaks: &mut Vec<usize>, sharp: &[bool], closed: bool, closed_smooth: bool, lt: &LineTol) -> Vec<(usize, usize)> {
    let m = pts.len();
    if closed_smooth {
        let found = straight_runs(pts, lt, (false, false));
        if let Some(&(s, _, _)) = found.iter().filter(|r| r.0 > 0).max_by_key(|r| r.1 - r.0) {
            let mut v: Vec<Pt> = pts[s..m - 1].iter().chain(&pts[..s]).copied().collect();
            v.push(v[0]);
            *pts = v;
        }
    }
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a < 2 {
            continue;
        }
        let d = &pts[a..=b];
        let found = if is_straight(d, lt.reach) { fit_line(d).map(|l| (0, b - a, l)).into_iter().collect() } else { straight_runs(d, lt, (sharp[a], sharp[b])) };
        for (s, e, line) in found {
            let (s, e) = (a + s, a + e);
            let (c, dir, _) = line;
            for i in [s, e] {
                let end = (i == a || i == b) && !closed_smooth;
                if end && !sharp[i] {
                    continue;
                }
                // Two stretches meeting at one point (also across the seam
                // of a loop) meet at their intersection.
                let other = match (runs.last(), runs.first()) {
                    (Some(&(_, pe)), _) if pe == i => lines.last(),
                    (_, Some(&(0, _))) if closed && i == m - 1 => lines.first(),
                    _ => None,
                };
                let q = match other {
                    Some(&(pc, pd, _)) => match intersect(pc, pd, c, dir) {
                        Some(x) if x.dist(pts[i]) <= 2.0 * lt.gap => x,
                        _ => pts[i],
                    },
                    None if end => continue,
                    None => c.add(dir.scale(pts[i].sub(c).dot(dir))),
                };
                pts[i] = q;
                if closed && (i == 0 || i == m - 1) {
                    pts[0] = q;
                    pts[m - 1] = q;
                }
            }
            runs.push((s, e));
            lines.push(line);
        }
    }
    breaks.extend(runs.iter().flat_map(|&(s, e)| [s, e]));
    breaks.sort_unstable();
    breaks.dedup();
    runs
}

/// Straight stretches of an open polyline as (first, last, line), found
/// greedily from the start: each one is the longest run from the first point
/// where a long, unbent one starts. `corners`: whether the polyline starts
/// and ends at a corner.
fn straight_runs(d: &[Pt], lt: &LineTol, corners: (bool, bool)) -> Vec<(usize, usize, Line)> {
    let n = d.len();
    let within = |i: usize, j: usize| fit_within(&d[i..=j], lt.tol).is_some();
    let mut runs: Vec<(usize, usize, Line)> = Vec::new();
    let mut i = 0;
    // Gallop, then bisect, to the end of the longest run from i within
    // tolerance.
    let longest = |i: usize| {
        let (mut good, mut step) = (i + 1, 1);
        while good + step < n && within(i, good + step) {
            good += step;
            step *= 2;
        }
        let mut bad = (good + step).min(n);
        while bad - good > 1 {
            let mid = (good + bad) / 2;
            if within(i, mid) {
                good = mid;
            } else {
                bad = mid;
            }
        }
        good
    };
    // Whether a run reaches the section's start and end.
    let reaches = |r: &(usize, usize, Line)| {
        let near = |a: usize, b: usize| d[a].dist(d[b]) <= lt.gap || (d[a].dist(d[b]) < lt.min_len && is_straight(&d[r.0.min(a)..=r.1.max(b)], lt.reach));
        (near(0, r.0), near(r.1, n - 1))
    };
    while i + 2 < n {
        let mut run = trim_run(d, i, longest(i), lt);
        // A curved tail at the start also cut the run short: extend it again
        // from where the line really starts.
        let mut from = i;
        while let Some((s, _, _)) = run {
            if s <= from {
                break;
            }
            from = s;
            run = trim_run(d, s, longest(s), lt);
        }
        match run.filter(|r| {
            let (rs, re) = reaches(r);
            (rs && re) || sharp_ends(d, r, lt, (rs, re), corners)
        }) {
            Some(r) => {
                i = r.1.max(i + 1);
                runs.push(r);
            }
            None => i += 1,
        }
    }
    // Neighbouring runs that are together still straight are one line.
    let mut k = 1;
    while k < runs.len() {
        let (s, e) = (runs[k - 1].0, runs[k].1);
        let joined = (d[runs[k - 1].1].dist(d[runs[k].0]) <= lt.gap).then(|| fit_within(&d[s..=e], lt.tol)).flatten();
        match joined {
            Some(line) if sagitta(&d[s..=e], line) <= lt.sag => {
                runs[k - 1] = (s, e, line);
                runs.remove(k);
            }
            _ => k += 1,
        }
    }
    // A stretch ending just short of a corner or chain end reaches it.
    if let Some(&r) = runs.first() {
        if reaches(&r).0 {
            runs[0].0 = 0;
        }
    }
    if let Some(&r) = runs.last() {
        if reaches(&r).1 {
            runs.last_mut().unwrap().1 = n - 1;
        }
    }
    runs
}

/// Whether the outline turns away from a straight run sharply enough at one
/// of its ends that does not `reach` the section end. A short piece of a
/// large arc is as straight as a line, but the arc keeps bending at the same
/// rate beyond it: within one run length it turns by at most about
/// 9 tol / len (radians), as the run is the longest within tolerance. A line
/// meeting a tighter curve, another line at an angle or a corner (`corners`
/// at the section ends) within that length turns much more. A run that
/// reaches a corner does not count that corner: an arc may start at one too.
fn sharp_ends(d: &[Pt], &(s, e, (_, dir, _)): &(usize, usize, Line), lt: &LineTol, reach: (bool, bool), corners: (bool, bool)) -> bool {
    let len = d[s].dist(d[e]);
    let need = 24.0 * lt.tol / len;
    let n = d.len();
    // Chord over the next run length past `from`, stepping by `step`.
    let turn = |from: usize, step: isize, out: Pt, corner: bool| {
        let mut j = from as isize;
        while (0..n as isize).contains(&(j + step)) && d[j as usize].dist(d[from]) < len {
            j += step;
        }
        if corner && d[j as usize].dist(d[from]) < len {
            return true;
        }
        let chord = d[j as usize].sub(d[from]);
        chord.len() >= lt.gap && chord.norm().dot(out).clamp(-1.0, 1.0).acos() >= need
    };
    let fwd = if d[e].sub(d[s]).dot(dir) < 0.0 { dir.scale(-1.0) } else { dir };
    (!reach.1 && turn(e, 1, fwd, corners.1)) || (!reach.0 && turn(s, -1, fwd.scale(-1.0), corners.0))
}

/// A run within tolerance may reach a little way into the curves at either
/// end: cut those tails off, then keep the run if it is long and unbent.
fn trim_run(d: &[Pt], mut s: usize, mut e: usize, lt: &LineTol) -> Option<(usize, usize, Line)> {
    // Judge the ends against the middle half, which the tails do not tilt.
    let q = (e - s) / 4;
    let (c, dir, rms) = fit_line(&d[s + q..=e - q])?;
    let nrm = Pt::new(-dir.y, dir.x);
    // Cut where a point leaves the noise band of the line.
    let thr = (3.0 * rms).clamp(0.15 * lt.tol, 0.5 * lt.tol);
    while s < e && d[s].sub(c).dot(nrm).abs() > thr {
        s += 1;
    }
    while e > s && d[e].sub(c).dot(nrm).abs() > thr {
        e -= 1;
    }
    if d[s].dist(d[e]) < lt.min_len {
        return None;
    }
    let line = fit_line(&d[s..=e])?;
    (sagitta(&d[s..=e], line) <= lt.sag).then_some((s, e, line))
}

/// Least-squares line through `d` if every point is within `tol` of it.
fn fit_within(d: &[Pt], tol: f64) -> Option<Line> {
    let line = fit_line(d)?;
    let (c, dir, _) = line;
    let nrm = Pt::new(-dir.y, dir.x);
    d.iter().all(|p| p.sub(c).dot(nrm).abs() <= tol).then_some(line)
}

/// How far points near a line bend: the sagitta of the least-squares
/// parabola through them, in the line's frame. Noise along a straight edge
/// averages out; a gentle arc does not.
fn sagitta(d: &[Pt], (c, dir, _): Line) -> f64 {
    if d.len() < 4 {
        return 0.0;
    }
    let nrm = Pt::new(-dir.y, dir.x);
    // Normal equations of v = k2 u^2 + k1 u + k0.
    let mut s = [0.0f64; 5];
    let mut t = [0.0f64; 3];
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in d {
        let q = p.sub(c);
        let (u, v) = (q.dot(dir), q.dot(nrm));
        lo = lo.min(u);
        hi = hi.max(u);
        let mut up = 1.0;
        for k in 0..5 {
            s[k] += up;
            if k < 3 {
                t[k] += up * v;
            }
            up *= u;
        }
    }
    let det3 = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let det = det3([[s[4], s[3], s[2]], [s[3], s[2], s[1]], [s[2], s[1], s[0]]]);
    if det.abs() < 1e-12 {
        return 0.0;
    }
    let k2 = det3([[t[2], s[3], s[2]], [t[1], s[2], s[1]], [t[0], s[1], s[0]]]) / det;
    let half = (hi - lo) / 2.0;
    (k2 * half * half).abs()
}

/// Replace cubics whose control points lie on their chord by true lines:
/// crisp straight edges with no effect on curved parts (whose controls are
/// never collinear), so tangent continuity is preserved.
fn straighten(segs: &mut [Seg], start: Pt, eps: f64) {
    let mut prev = start;
    for s in segs.iter_mut() {
        if let Seg::Cubic(a, b, e) = *s {
            let ch = e.sub(prev);
            let len = ch.len();
            if len > 1e-9 {
                let off = |p: Pt| {
                    let v = p.sub(prev);
                    (ch.x * v.y - ch.y * v.x).abs() / len
                };
                let inside = |p: Pt| {
                    let t = p.sub(prev).dot(ch) / (len * len);
                    (-0.01..=1.01).contains(&t)
                };
                if off(a) <= eps && off(b) <= eps && inside(a) && inside(b) {
                    *s = Seg::Line(e);
                }
            }
        }
        prev = s.end();
    }
}

/// Least-squares line through points: (centroid, unit direction, rms residual).
fn fit_line(pts: &[Pt]) -> Option<Line> {
    if pts.len() < 2 {
        return None;
    }
    let n = pts.len() as f64;
    let c = pts.iter().fold(Pt::new(0.0, 0.0), |a, p| a.add(*p)).scale(1.0 / n);
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for p in pts {
        let d = p.sub(c);
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
    }
    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let dir = Pt::new(theta.cos(), theta.sin());
    let normal = Pt::new(-dir.y, dir.x);
    let rms = (pts.iter().map(|p| p.sub(c).dot(normal).powi(2)).sum::<f64>() / n).sqrt();
    Some((c, dir, rms))
}

fn intersect(c1: Pt, d1: Pt, c2: Pt, d2: Pt) -> Option<Pt> {
    let den = d1.x * d2.y - d1.y * d2.x;
    if den.abs() < 0.2 {
        return None; // nearly parallel arms: not a corner worth sharpening
    }
    let t = ((c2.x - c1.x) * d2.y - (c2.y - c1.y) * d2.x) / den;
    Some(c1.add(d1.scale(t)))
}

/// A fitted line: (centroid, unit direction, rms residual).
type Line = (Pt, Pt, f64);

/// Lines fitted to the two arms of corner `corners[ci_idx]`. Arms start just
/// past the corner zone (left unsnapped, see fit_polyline) and stay within
/// half the way to the neighbouring corners.
fn corner_arms(raw: &[Pt], closed: bool, corners: &[usize], ci_idx: usize, k: usize) -> Option<(Line, Line)> {
    let n = raw.len() as isize;
    let idx = |i: isize| -> Option<usize> {
        if closed {
            Some(i.rem_euclid(n) as usize)
        } else if (0..n).contains(&i) {
            Some(i as usize)
        } else {
            None
        }
    };
    // Distance (in points) from each corner to its neighbouring corners.
    let gap = |a: usize, b: usize| -> isize {
        if closed {
            ((b as isize - a as isize).rem_euclid(n)).max(1)
        } else {
            (b as isize - a as isize).abs()
        }
    };
    let c = corners[ci_idx];
    let ci = c as isize;
    let m = corners.len();
    let prev_gap = if m > 1 && (closed || ci_idx > 0) { gap(corners[(ci_idx + m - 1) % m], c) } else { ci + 1 };
    let next_gap = if m > 1 && (closed || ci_idx + 1 < m) { gap(c, corners[(ci_idx + 1) % m]) } else { n - ci };
    let near = (k / 3) as isize + 1;
    let far = (k as isize).min(prev_gap.min(next_gap) / 2).max(near + 1);
    let arm = |dir: isize, far: isize| -> Option<Line> {
        let pts: Option<Vec<Pt>> = (near..=far).map(|o| idx(ci + dir * o).map(|j| raw[j])).collect();
        fit_line(&pts?)
    };
    // A short arm gives a poor direction (sub-pixel snapping leaves a little
    // periodic noise), and at acute corners a small angle error moves the
    // intersection far. Extend each arm along its own side while it stays
    // straight; the allowed residual grows with supersampling like the noise.
    let ext_tol = 0.25 * (k as f64 / 8.0).max(1.0);
    let extend = |dir: isize, gap: isize| -> Option<Line> {
        let mut best = arm(dir, far)?;
        let limit = (gap / 2).min(8 * k as isize);
        let mut f = far * 2;
        while best.2 <= 0.45 && f <= limit {
            match arm(dir, f) {
                Some(l) if l.2 <= ext_tol => best = l,
                _ => break,
            }
            f *= 2;
        }
        Some(best)
    };
    Some((extend(-1, prev_gap)?, extend(1, next_gap)?))
}

/// Restore sharp corners rounded off by anti-aliasing or supersampling: fit a
/// line to each arm (away from the corner itself), move the corner to their
/// intersection and project the points between onto the arms. Only applied
/// when both arms are straight and the intersection is close by.
fn sharpen_corners(raw: &[Pt], closed: bool, corners: &[usize], k: usize) -> Vec<Pt> {
    let n = raw.len() as isize;
    let mut pts = raw.to_vec();
    let idx = |i: isize| -> Option<usize> {
        if closed {
            Some(i.rem_euclid(n) as usize)
        } else if (0..n).contains(&i) {
            Some(i as usize)
        } else {
            None
        }
    };
    let max_move = (k as f64 / 4.0 * 1.6).max(1.2);
    let near = (k / 3) as isize + 1;
    for (ci_idx, &c) in corners.iter().enumerate() {
        let ci = c as isize;
        let Some(((ca, da, ra), (cb, db, rb))) = corner_arms(raw, closed, corners, ci_idx, k) else { continue };
        if ra > 0.45 || rb > 0.45 {
            continue; // an arm is curved: keep the corner as detected
        }
        let Some(x) = intersect(ca, da, cb, db) else { continue };
        if x.dist(raw[c]) > max_move {
            continue;
        }
        pts[c] = x;
        // Project the chamfer points onto their arm lines.
        for o in 1..near {
            for (dir, cl, dl) in [(-1isize, ca, da), (1, cb, db)] {
                if let Some(j) = idx(ci + dir * o) {
                    let v = raw[j].sub(cl);
                    pts[j] = cl.add(dl.scale(v.dot(dl)));
                }
            }
        }
    }
    pts
}

/// Ramer-Douglas-Peucker simplification into straight segments.
fn simplify_lines(d: &[Pt], tol: f64, out: &mut Vec<Seg>) {
    fn rec(d: &[Pt], a: usize, b: usize, tol: f64, keep: &mut Vec<usize>) {
        let (pa, pb) = (d[a], d[b]);
        let ab = pb.sub(pa);
        let len = ab.len().max(1e-9);
        let mut worst = (0.0, a);
        for (i, p) in d.iter().enumerate().take(b).skip(a + 1) {
            let ap = p.sub(pa);
            let dist = (ab.x * ap.y - ab.y * ap.x).abs() / len;
            if dist > worst.0 {
                worst = (dist, i);
            }
        }
        if worst.0 > tol {
            rec(d, a, worst.1, tol, keep);
            keep.push(worst.1);
            rec(d, worst.1, b, tol, keep);
        }
    }
    let mut keep = Vec::new();
    rec(d, 0, d.len() - 1, tol, &mut keep);
    for i in keep {
        out.push(Seg::Line(d[i]));
    }
    out.push(Seg::Line(d[d.len() - 1]));
}

/// Extra angle (degrees) within which grid corners are only candidates.
const LOOSE_DEG: f64 = 15.0;

/// Corners as (index, angle in degrees measured on the grid); angles up to
/// `max_angle_deg + LOOSE_DEG` are reported.
fn detect_corners(pts: &[Pt], closed: bool, max_angle_deg: f64, k: usize) -> Vec<(usize, f64)> {
    let n = pts.len();
    if max_angle_deg <= 0.0 {
        return vec![]; // corner detection switched off
    }
    if n < 2 * k + 1 && !closed {
        return vec![];
    }
    // On small closed loops long arms would wrap around past other corners.
    let k = if closed { k.min(n / 4).max(1) } else { k };
    let cos_thr = (max_angle_deg + LOOSE_DEG).min(179.0).to_radians().cos();
    let get = |i: isize| -> Option<Pt> {
        if closed {
            Some(pts[i.rem_euclid(n as isize) as usize])
        } else if i < 0 || i >= n as isize {
            None
        } else {
            Some(pts[i as usize])
        }
    };
    // Cosine of the angle between the two arms at every vertex (higher =
    // sharper), averaged over arm lengths k/4..=k. A true corner is sharp at
    // every scale; a point in the middle of a short side only looks sharp
    // when its arms are long enough to wrap around the neighbouring corners,
    // so averaging keeps such points from tying with (and displacing) the
    // real corner.
    let mut sharp = vec![f64::NEG_INFINITY; n];
    // Span 1 sees every pixel step as a right angle; start at 2.
    let k_min = (k / 4).max(2).min(k);
    for i in 0..n {
        let ii = i as isize;
        let (mut sum, mut cnt) = (0.0, 0);
        for kk in k_min..=k {
            let kk = kk as isize;
            if let (Some(a), Some(b)) = (get(ii - kk), get(ii + kk)) {
                let (va, vb) = (a.sub(pts[i]), b.sub(pts[i]));
                if va.len() > 1e-9 && vb.len() > 1e-9 {
                    sum += va.norm().dot(vb.norm());
                    cnt += 1;
                }
            }
        }
        if cnt > 0 {
            sharp[i] = sum / cnt as f64;
        }
    }
    let mut out = Vec::new();
    for i in 0..n {
        if !closed && (i < 2 || i + 2 >= n) {
            continue;
        }
        if sharp[i] < cos_thr {
            continue; // angle wider than threshold
        }
        // Non-maximum suppression within half the arm length: a full arm
        // would let one corner of a short side suppress the other.
        let mut is_max = true;
        for o in 1..=(k / 2).max(1) as isize {
            for j in [i as isize - o, i as isize + o] {
                let jj = if closed { j.rem_euclid(n as isize) } else { j };
                if jj < 0 || jj >= n as isize {
                    continue;
                }
                let s = sharp[jj as usize];
                if s > sharp[i] || (s == sharp[i] && (jj as usize) < i) {
                    is_max = false;
                }
            }
        }
        if is_max {
            out.push((i, sharp[i].clamp(-1.0, 1.0).acos().to_degrees()));
        }
    }
    out
}

fn taubin(pts: &mut [Pt], fixed: &[bool], cyclic: bool, passes: usize) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    // In cyclic mode the last point duplicates the first; smooth n-1 points.
    let m = if cyclic { n - 1 } else { n };
    for _ in 0..passes {
        for &f in &[0.5f64, -0.53] {
            let src = pts.to_vec();
            for i in 0..m {
                if fixed[i] {
                    continue;
                }
                let (prev, next) = if cyclic {
                    (src[(i + m - 1) % m], src[(i + 1) % m])
                } else if i == 0 || i == n - 1 {
                    continue;
                } else {
                    (src[i - 1], src[i + 1])
                };
                let lap = prev.add(next).scale(0.5).sub(src[i]);
                pts[i] = src[i].add(lap.scale(f));
            }
        }
    }
}

fn is_straight(d: &[Pt], tol: f64) -> bool {
    let (a, b) = (d[0], d[d.len() - 1]);
    let ab = b.sub(a);
    let len = ab.len();
    if len < 1e-9 {
        return false;
    }
    d.iter().all(|p| {
        let ap = p.sub(a);
        (ab.x * ap.y - ab.y * ap.x).abs() / len <= tol
    })
}

fn left_tangent(d: &[Pt]) -> Pt {
    let j = 3.min(d.len() - 1);
    d[j].sub(d[0]).norm()
}

fn right_tangent(d: &[Pt]) -> Pt {
    let n = d.len();
    let j = n - 1 - 3.min(n - 1);
    d[j].sub(d[n - 1]).norm()
}

fn fit_cubic(d: &[Pt], first: usize, last: usize, t1: Pt, t2: Pt, err2: f64, out: &mut Vec<Seg>, depth: u32) {
    let n = last - first + 1;
    if n == 2 || depth > 24 {
        let dist = d[first].dist(d[last]) / 3.0;
        out.push(Seg::Cubic(d[first].add(t1.scale(dist)), d[last].add(t2.scale(dist)), d[last]));
        return;
    }
    let mut u = chord_params(d, first, last);
    let mut bez = generate_bezier(d, first, last, &u, t1, t2);
    let (mut max, mut split) = max_error(d, first, last, &bez, &u);
    if max < err2 {
        out.push(Seg::Cubic(bez[1], bez[2], bez[3]));
        return;
    }
    if max < err2 * 16.0 {
        for _ in 0..6 {
            u = reparameterize(d, first, last, &u, &bez);
            bez = generate_bezier(d, first, last, &u, t1, t2);
            let r = max_error(d, first, last, &bez, &u);
            max = r.0;
            split = r.1;
            if max < err2 {
                out.push(Seg::Cubic(bez[1], bez[2], bez[3]));
                return;
            }
        }
    }
    let split = split.clamp(first + 1, last - 1);
    let tc = d[split - 1].sub(d[split + 1]).norm();
    fit_cubic(d, first, split, t1, tc, err2, out, depth + 1);
    fit_cubic(d, split, last, tc.scale(-1.0), t2, err2, out, depth + 1);
}

fn chord_params(d: &[Pt], first: usize, last: usize) -> Vec<f64> {
    let mut u = vec![0.0; last - first + 1];
    for i in first + 1..=last {
        u[i - first] = u[i - first - 1] + d[i].dist(d[i - 1]);
    }
    let total = u[last - first];
    if total > 0.0 {
        for v in &mut u {
            *v /= total;
        }
    }
    u
}

fn generate_bezier(d: &[Pt], first: usize, last: usize, u: &[f64], t1: Pt, t2: Pt) -> [Pt; 4] {
    let (p0, p3) = (d[first], d[last]);
    let mut c = [[0.0f64; 2]; 2];
    let mut x = [0.0f64; 2];
    for (i, &t) in u.iter().enumerate() {
        let s = 1.0 - t;
        let b0 = s * s * s;
        let b1 = 3.0 * t * s * s;
        let b2 = 3.0 * t * t * s;
        let b3 = t * t * t;
        let a1 = t1.scale(b1);
        let a2 = t2.scale(b2);
        c[0][0] += a1.dot(a1);
        c[0][1] += a1.dot(a2);
        c[1][1] += a2.dot(a2);
        let tmp = d[first + i].sub(p0.scale(b0 + b1).add(p3.scale(b2 + b3)));
        x[0] += a1.dot(tmp);
        x[1] += a2.dot(tmp);
    }
    c[1][0] = c[0][1];
    let det = c[0][0] * c[1][1] - c[1][0] * c[0][1];
    let (mut al, mut ar) = if det.abs() > 1e-12 {
        ((x[0] * c[1][1] - x[1] * c[0][1]) / det, (c[0][0] * x[1] - c[1][0] * x[0]) / det)
    } else {
        (0.0, 0.0)
    };
    let seg_len = p0.dist(p3);
    let eps = 1e-6 * seg_len;
    if al < eps || ar < eps {
        al = seg_len / 3.0;
        ar = al;
    }
    [p0, p0.add(t1.scale(al)), p3.add(t2.scale(ar)), p3]
}

fn max_error(d: &[Pt], first: usize, last: usize, b: &[Pt; 4], u: &[f64]) -> (f64, usize) {
    let mut max = 0.0;
    let mut split = (last - first + 1) / 2 + first;
    for i in first + 1..last {
        let p = bezier(b[0], b[1], b[2], b[3], u[i - first]);
        let e = p.sub(d[i]);
        let dd = e.dot(e);
        if dd >= max {
            max = dd;
            split = i;
        }
    }
    (max, split)
}

fn reparameterize(d: &[Pt], first: usize, last: usize, u: &[f64], b: &[Pt; 4]) -> Vec<f64> {
    (first..=last).map(|i| newton(b, d[i], u[i - first])).collect()
}

fn newton(q: &[Pt; 4], p: Pt, u: f64) -> f64 {
    let q1 = [q[1].sub(q[0]).scale(3.0), q[2].sub(q[1]).scale(3.0), q[3].sub(q[2]).scale(3.0)];
    let q2 = [q1[1].sub(q1[0]).scale(2.0), q1[2].sub(q1[1]).scale(2.0)];
    let qu = bezier(q[0], q[1], q[2], q[3], u);
    let s = 1.0 - u;
    let q1u = q1[0].scale(s * s).add(q1[1].scale(2.0 * s * u)).add(q1[2].scale(u * u));
    let q2u = q2[0].scale(s).add(q2[1].scale(u));
    let diff = qu.sub(p);
    let num = diff.dot(q1u);
    let den = q1u.dot(q1u) + diff.dot(q2u);
    if den.abs() < 1e-12 {
        return u;
    }
    (u - num / den).clamp(0.0, 1.0)
}
