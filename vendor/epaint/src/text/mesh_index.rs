//! Derived bounds for contiguous triangle blocks. No glyphs or text are retained.
use crate::{Mesh, Rect, Vec2};
use std::{
    ops::Range,
    sync::{Arc, OnceLock},
};

// Keep triangle order unchanged. A leaf normally spans 128 glyph quads, but also
// handles backgrounds, selection rectangles and decoration triangles.
const LEAF_INDICES: usize = 256 * 3;
#[derive(Clone, Default)]
pub struct TextMeshCache(OnceLock<Option<Arc<TextMeshIndex>>>);
impl std::fmt::Debug for TextMeshCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TextMeshCache")
    }
}
impl PartialEq for TextMeshCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for TextMeshCache {}
impl TextMeshCache {
    /// Reset after changing mesh positions, indices or topology. Colors/UVs may
    /// change without invalidation because the index stores only bounds/ranges.
    pub fn invalidate(&mut self) {
        *self = Self::default();
    }
    /// Approximate initialized metadata, without building the index.
    pub fn bytes(&self) -> usize {
        self.0.get().and_then(Option::as_ref).map_or(0, |index| {
            std::mem::size_of::<TextMeshIndex>()
                + index.bounds.capacity() * std::mem::size_of::<Rect>()
                + index.leaves.capacity() * std::mem::size_of::<MeshLeaf>()
        })
    }
    pub(crate) fn get(&self, mesh: &Mesh) -> Option<&TextMeshIndex> {
        if mesh.indices.len() < 4 * LEAF_INDICES {
            return None;
        }
        self.0
            .get_or_init(|| TextMeshIndex::new(mesh).map(Arc::new))
            .as_deref()
    }
}
pub(crate) struct MeshLeaf {
    pub indices: Range<usize>,
    pub vertices: Range<usize>,
}
pub(crate) struct TextMeshIndex {
    bounds: Vec<Rect>,
    leaves: Vec<MeshLeaf>,
    base: usize,
}
impl TextMeshIndex {
    fn new(mesh: &Mesh) -> Option<Self> {
        if !mesh.indices.len().is_multiple_of(3) {
            return None;
        }
        let count = mesh.indices.len().div_ceil(LEAF_INDICES);
        let base = count.checked_next_power_of_two()?;
        let mut bounds = vec![Rect::NOTHING; base.checked_mul(2)?];
        let mut leaves = Vec::with_capacity(count);
        for (i, indices) in mesh.indices.chunks(LEAF_INDICES).enumerate() {
            let (mut min, mut max) = (usize::MAX, 0);
            let mut rect = Rect::NOTHING;
            for &index in indices {
                let index = index as usize;
                let pos = mesh.vertices.get(index)?.pos;
                if !pos.x.is_finite() || !pos.y.is_finite() {
                    return None;
                }
                rect.extend_with(pos);
                min = min.min(index);
                max = max.max(index);
            }
            bounds[base + i] = rect;
            leaves.push(MeshLeaf {
                indices: i * LEAF_INDICES..i * LEAF_INDICES + indices.len(),
                vertices: min..max + 1,
            });
        }
        for i in (1..base).rev() {
            bounds[i] = bounds[2 * i].union(bounds[2 * i + 1]);
        }
        Some(Self {
            bounds,
            leaves,
            base,
        })
    }
    pub(crate) fn visit(&self, clip: Rect, offset: Vec2, mut visitor: impl FnMut(&MeshLeaf)) {
        self.visit_node(1, clip, offset, &mut visitor);
    }
    fn visit_node(
        &self,
        node: usize,
        clip: Rect,
        offset: Vec2,
        visitor: &mut impl FnMut(&MeshLeaf),
    ) {
        // Use the same addition as emitted vertices. Inverting a large text
        // origin into local clip coordinates can round a clip edge inward.
        if !clip.intersects(self.bounds[node].translate(offset)) {
            return;
        }
        if node >= self.base {
            if let Some(leaf) = self.leaves.get(node - self.base) {
                visitor(leaf);
            }
        } else {
            self.visit_node(2 * node, clip, offset, visitor);
            self.visit_node(2 * node + 1, clip, offset, visitor);
        }
    }
}
