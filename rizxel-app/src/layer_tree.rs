//! Parent/child structure of the layer list, derived from the `parent` ids.
//!
//! Layers stay one flat list, bottom to top. A group's descendants sit
//! directly below it, so every subtree is the contiguous index range that
//! ends at its root (in the Layers panel, the rows right under the group
//! row). [`LayerTree`] computes depths, children and subtree ranges and
//! checks that rule; [`canonical_order`] restores it for files written
//! without it.

use std::collections::HashMap;
use std::ops::Range;

use rizxel_io::MAX_GROUP_DEPTH;
use rizxel_math::LayerId;

use crate::document::Layer;

/// The parts of a layer the tree is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub id: LayerId,
    pub parent: Option<LayerId>,
    pub group: bool,
}

impl Link {
    pub fn of(layer: &Layer) -> Self {
        Self {
            id: layer.id,
            parent: layer.props.parent,
            group: layer.kind().is_group(),
        }
    }
}

/// Derived tree of a layer list (see the module docs).
#[derive(Debug, Clone)]
pub struct LayerTree {
    parent: Vec<Option<usize>>,
    depth: Vec<usize>,
    /// First index of each layer's subtree (the layer itself for leaves).
    start: Vec<usize>,
    /// Number of descendants.
    descendants: Vec<usize>,
    valid: bool,
}

impl LayerTree {
    pub fn new(layers: &[Layer]) -> Self {
        Self::from_links(&layers.iter().map(Link::of).collect::<Vec<_>>())
    }

    pub fn from_links(links: &[Link]) -> Self {
        let n = links.len();
        let index: HashMap<LayerId, usize> =
            links.iter().enumerate().map(|(i, l)| (l.id, i)).collect();
        let mut valid = index.len() == n;
        let parent: Vec<Option<usize>> = links
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let p = l.parent?;
                match index.get(&p) {
                    Some(&pi) if pi != i && links[pi].group => Some(pi),
                    _ => {
                        valid = false;
                        None
                    }
                }
            })
            .collect();
        let mut depth = vec![0; n];
        let mut start: Vec<usize> = (0..n).collect();
        let mut end = vec![0; n];
        let mut descendants = vec![0; n];
        for i in 0..n {
            // Depth-capped walk, so a cycle ends it too.
            let mut p = parent[i];
            let mut d = 0;
            while let Some(a) = p {
                d += 1;
                if d > MAX_GROUP_DEPTH {
                    valid = false;
                    break;
                }
                start[a] = start[a].min(i);
                end[a] = end[a].max(i);
                descendants[a] += 1;
                p = parent[a];
            }
            depth[i] = d.min(MAX_GROUP_DEPTH);
        }
        // Contiguity: the descendants fill exactly the range below the root.
        for a in 0..n {
            if descendants[a] > 0 && (end[a] >= a || a - start[a] != descendants[a]) {
                valid = false;
            }
        }
        Self {
            parent,
            depth,
            start,
            descendants,
            valid,
        }
    }

    /// Parents exist and are groups, no cycles, depth within
    /// [`MAX_GROUP_DEPTH`] and every subtree contiguous.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    /// Index of the group containing layer `i`.
    pub fn parent(&self, i: usize) -> Option<usize> {
        self.parent.get(i).copied().flatten()
    }

    /// Nesting depth: 0 for top-level layers.
    pub fn depth(&self, i: usize) -> usize {
        self.depth.get(i).copied().unwrap_or(0)
    }

    /// Indices of layer `i` and its descendants (it is the last one).
    pub fn subtree(&self, i: usize) -> Range<usize> {
        match self.start.get(i) {
            Some(&s) => s..i + 1,
            None => i..i,
        }
    }

    pub fn has_children(&self, i: usize) -> bool {
        self.descendants.get(i).is_some_and(|&n| n > 0)
    }

    /// Direct children of layer `i`, bottom to top.
    pub fn children(&self, i: usize) -> Vec<usize> {
        (0..self.len())
            .filter(|&c| self.parent[c] == Some(i))
            .collect()
    }

    /// True when `a` is a strict ancestor of `b`.
    pub fn is_ancestor(&self, a: usize, b: usize) -> bool {
        let mut p = self.parent(b);
        for _ in 0..=MAX_GROUP_DEPTH {
            match p {
                Some(x) if x == a => return true,
                Some(x) => p = self.parent(x),
                None => return false,
            }
        }
        false
    }

    /// Ancestors of layer `i`, nearest first.
    pub fn ancestors(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.parent(i), |&a| self.parent(a)).take(MAX_GROUP_DEPTH)
    }

    /// Rows of the Layers panel, top to bottom: every layer except those
    /// inside a group for which `collapsed` is true.
    pub fn visible_rows(&self, collapsed: impl Fn(usize) -> bool) -> Vec<usize> {
        (0..self.len())
            .rev()
            .filter(|&i| !self.ancestors(i).any(&collapsed))
            .collect()
    }

    /// The row that shows layer `i` in the Layers panel: `i`, or its
    /// outermost ancestor for which `collapsed` is true.
    pub fn shown_row(&self, i: usize, collapsed: impl Fn(usize) -> bool) -> usize {
        self.ancestors(i)
            .filter(|&a| collapsed(a))
            .last()
            .unwrap_or(i)
    }

    /// Deepest group containing every layer of `indices` (`None`: the top
    /// level, or `indices` is empty).
    pub fn common_parent(&self, indices: &[usize]) -> Option<usize> {
        let (&first, rest) = indices.split_first()?;
        self.ancestors(first)
            .find(|&a| rest.iter().all(|&i| self.is_ancestor(a, i)))
    }

    /// Deepest depth inside the subtree of `i`, relative to `i` (0 when it
    /// has no children).
    pub fn subtree_height(&self, i: usize) -> usize {
        let base = self.depth(i);
        self.subtree(i)
            .map(|j| self.depth(j) - base)
            .max()
            .unwrap_or(0)
    }
}

/// Order of `links` that satisfies the contiguity rule while keeping the
/// draw order of siblings: each group is followed by nothing of its own
/// (its children come right before it). Returns old indices in their new
/// order. Links with a missing or cyclic parent are kept as top level.
pub fn canonical_order(links: &[Link]) -> Vec<usize> {
    let tree = LayerTree::from_links(links);
    let n = links.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for i in 0..n {
        match tree.parent(i) {
            Some(p) => children[p].push(i),
            None => roots.push(i),
        }
    }
    let mut out = Vec::with_capacity(n);
    let mut placed = vec![false; n];
    fn emit(i: usize, children: &[Vec<usize>], placed: &mut [bool], out: &mut Vec<usize>) {
        if std::mem::replace(&mut placed[i], true) {
            return;
        }
        for &c in &children[i] {
            emit(c, children, placed, out);
        }
        out.push(i);
    }
    for r in roots {
        emit(r, &children, &mut placed, &mut out);
    }
    // Members of a cycle are never reached from a root.
    out.extend((0..n).filter(|&i| !placed[i]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: LayerId, parent: Option<LayerId>) -> Link {
        Link {
            id,
            parent,
            group: false,
        }
    }

    fn group(id: LayerId, parent: Option<LayerId>) -> Link {
        Link {
            id,
            parent,
            group: true,
        }
    }

    /// Bottom to top: a, [b, [c] G2] G1, d.
    fn sample() -> Vec<Link> {
        vec![
            leaf(1, None),
            leaf(2, Some(10)),
            leaf(3, Some(11)),
            group(11, Some(10)),
            group(10, None),
            leaf(4, None),
        ]
    }

    #[test]
    fn depths_children_and_ranges() {
        let t = LayerTree::from_links(&sample());
        assert!(t.is_valid());
        assert_eq!(
            (0..6).map(|i| t.depth(i)).collect::<Vec<_>>(),
            [0, 1, 2, 1, 0, 0]
        );
        assert_eq!(t.children(4), [1, 3]);
        assert_eq!(t.children(3), [2]);
        assert_eq!(t.subtree(4), 1..5);
        assert_eq!(t.subtree(3), 2..4);
        assert_eq!(t.subtree(0), 0..1);
        assert!(t.is_ancestor(4, 2) && t.is_ancestor(3, 2) && !t.is_ancestor(2, 3));
        assert!(!t.is_ancestor(4, 4));
        assert_eq!(t.ancestors(2).collect::<Vec<_>>(), [3, 4]);
        assert_eq!(t.subtree_height(4), 2);
        assert_eq!(t.subtree_height(5), 0);
        assert_eq!(t.parent(1), Some(4));
    }

    #[test]
    fn broken_structures_are_invalid() {
        // A child above its group, and a child split from its siblings.
        let mut above = sample();
        above.swap(1, 4);
        assert!(!LayerTree::from_links(&above).is_valid());
        let mut split = sample();
        split.swap(0, 1);
        assert!(!LayerTree::from_links(&split).is_valid());
        // Parent that is not a group, missing parent, cycle.
        assert!(!LayerTree::from_links(&[leaf(1, None), leaf(2, Some(1))]).is_valid());
        assert!(!LayerTree::from_links(&[leaf(1, Some(9))]).is_valid());
        assert!(!LayerTree::from_links(&[group(1, Some(2)), group(2, Some(1))]).is_valid());
    }

    #[test]
    fn depth_cap() {
        // Chain of groups, deepest first: depth == MAX is fine, deeper is not.
        let chain = |n: usize| -> Vec<Link> {
            (0..n)
                .map(|i| {
                    let id = (n - i) as LayerId;
                    group(id, (id > 1).then_some(id - 1))
                })
                .collect()
        };
        assert!(LayerTree::from_links(&chain(MAX_GROUP_DEPTH + 1)).is_valid());
        assert!(!LayerTree::from_links(&chain(MAX_GROUP_DEPTH + 2)).is_valid());
    }

    #[test]
    fn canonical_order_fixes_files_and_keeps_good_lists() {
        let good = sample();
        assert_eq!(canonical_order(&good), (0..6).collect::<Vec<_>>());
        // Goxel lists groups before their children.
        let gox = vec![
            group(10, None),
            leaf(1, Some(10)),
            leaf(2, None),
            leaf(3, Some(10)),
        ];
        let order = canonical_order(&gox);
        assert_eq!(order, [1, 3, 0, 2]);
        let fixed: Vec<Link> = order.iter().map(|&i| gox[i]).collect();
        assert!(LayerTree::from_links(&fixed).is_valid());
        // Cycles keep every layer.
        let cyc = [group(1, Some(2)), group(2, Some(1)), leaf(3, None)];
        assert_eq!(canonical_order(&cyc).len(), 3);
    }
}
