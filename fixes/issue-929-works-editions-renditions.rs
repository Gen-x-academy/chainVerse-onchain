//! Fix for #929: distinguish works, editions, and renditions.
//!
//! ## Problem
//!
//! The repository has no model for multiple editions or digital formats
//! of one book: everything is a flat `work` with a single hash.
//!
//! ## Solution
//!
//! `contracts/library-rights` gains a three-level hierarchy with stable
//! parent-child ids:
//!
//! ```text
//! Work (abstract, parent-less)
//!   └─ Edition (parent must be a Work)
//!        └─ Rendition (parent must be an Edition)
//! ```
//!
//! - `register_edition(caller, parent_work_id, edition_id, metadata,
//!   custodian)` -- the parent must exist and be a `Work`; anything else
//!   is `InvalidParent` (editions can never hang off editions or
//!   renditions).
//! - `register_rendition(caller, parent_edition_id, rendition_id,
//!   content, metadata, custodian)` -- the parent must exist and be an
//!   `Edition`; anything else is `InvalidParent`.
//! - Kinds are fixed at registration and there is no re-parenting, so
//!   the graph is a forest: relationships **cannot cycle** and **cannot
//!   cross an invalid parent kind** by construction.
//! - `children(parent_id, cursor, limit)` -- cursor/limit-bounded
//!   queries (`1..=50` per page, out-of-range cursors rejected), so a
//!   catalog with thousands of editions/renditions can be paged
//!   deterministically without unbounded scans.
//!
//! ## ABI impact
//!
//! New `register_edition`, `register_rendition`, and `children` entry
//! points. `children` returns a `ChildrenPage { ids, next_cursor,
//! done }`.
//!
//! ## Storage impact
//!
//! New persistent keys: `Entry(id)` per child, plus `ChildCount(parent)`
//! and `ChildIndex(parent, i)` child indexes. All CATALOG-tiered.
//!
//! ## Event impact
//!
//! `EDN_NEW (edition_id, parent, version, metadata_hash)` and
//! `RND_NEW (rendition_id, parent, version, algorithm, digest)`.
//!
//! ## Privacy impact
//!
//! Only ids and coarse kind/parent facts are on-chain; edition-specific
//! cover art, descriptions, and format metadata stay off-chain and are
//! referenced only by hash.
//!
//! ## Deployment & migration impact
//!
//! Additive; no existing storage reshaped. Child indexes are new keys.
//!
//! ## Tests
//!
//! `contracts/library-rights/src/tests/registry.rs`: edition/rendition
//! round trips, multiple editions and formats, pagination across pages
//! and at the 50-per-page cap, edition-under-edition and
//! rendition-under-work rejection, missing-parent rejection, duplicate
//! child id rejection, and invalid limit/cursor rejection.

/// Illustrative core model (see `contracts/library-rights` for the
/// deployable Soroban contract).
pub struct Catalog {
    parent: std::collections::HashMap<u64, Option<u64>>, // id -> parent
}
impl Catalog {
    pub fn new() -> Self {
        Self { parent: Default::default() }
    }
    /// `kind`: 0 = Work (parentless), 1 = Edition (needs a Work parent),
    /// 2 = Rendition (needs an Edition parent).
    pub fn register(&mut self, id: u64, kind: u8, parent: Option<u64>) -> Result<(), &'static str> {
        if self.parent.contains_key(&id) {
            return Err("already registered");
        }
        match (kind, parent) {
            (0, None) => {}
            (1, Some(p)) => {
                if self.parent.get(&p) != Some(&None) {
                    return Err("invalid parent"); // parent must be a Work
                }
            }
            (2, Some(p)) => {
                if !matches!(self.parent.get(&p), Some(Some(_))) {
                    return Err("invalid parent"); // parent must be an Edition
                }
            }
            _ => return Err("invalid parent"),
        }
        self.parent.insert(id, parent);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_is_a_forest() {
        let mut c = Catalog::new();
        c.register(1, 0, None).unwrap(); // work
        c.register(2, 1, Some(1)).unwrap(); // edition of work
        c.register(3, 2, Some(2)).unwrap(); // epub rendition
        c.register(4, 2, Some(2)).unwrap(); // pdf rendition
        assert_eq!(c.register(5, 1, Some(2)), Err("invalid parent")); // edition off edition
        assert_eq!(c.register(6, 2, Some(1)), Err("invalid parent")); // rendition off work
        assert_eq!(c.register(7, 1, Some(99)), Err("invalid parent")); // missing work
    }
}
