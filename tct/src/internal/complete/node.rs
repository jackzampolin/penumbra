use serde::{Deserialize, Serialize};

use crate::{
    internal::{
        hash::CachedHash,
        height::{IsHeight, Succ},
        path::{self, AuthPath, WhichWay, Witness},
        three::{IntoElems, Three},
    },
    Complete, ForgetOwned, GetHash, Hash, Height, Insert,
};

use super::super::active;

pub mod children;
pub use children::Children;

/// A complete sparse node in a tree, storing only the witnessed subtrees.
#[derive(Clone, Derivative, Serialize, Deserialize)]
#[derivative(Debug, PartialEq(bound = "Child: PartialEq"), Eq(bound = "Child: Eq"))]
pub struct Node<Child> {
    #[derivative(PartialEq = "ignore")]
    #[serde(skip)]
    hash: CachedHash,
    children: Children<Child>,
}

impl<Child: Complete> PartialEq<active::Node<Child::Focus>> for Node<Child>
where
    Child: PartialEq + PartialEq<Child::Focus>,
{
    fn eq(&self, other: &active::Node<Child::Focus>) -> bool {
        other == self
    }
}

impl<Child: Height> Node<Child> {
    /// Set the hash of this node without checking to see whether the hash is correct.
    ///
    /// # Correctness
    ///
    /// This should only be called when the hash is already known (i.e. after construction from
    /// children with a known node hash).
    pub(in super::super) fn set_hash_unchecked(&self, hash: Hash) {
        self.hash.set_if_empty(|| hash);
    }

    pub(in super::super) fn from_siblings_and_focus_or_else_hash(
        siblings: Three<Insert<Child>>,
        focus: Insert<Child>,
    ) -> Insert<Self> {
        fn zero<T>() -> Insert<T> {
            Insert::Hash(Hash::default())
        }

        // Push the focus into the siblings, and fill any empty children with the zero hash
        Self::from_children_or_else_hash(match siblings.push(focus) {
            Err([a, b, c, d]) => [a, b, c, d],
            Ok(siblings) => match siblings.into_elems() {
                IntoElems::_3([a, b, c]) => [a, b, c, zero()],
                IntoElems::_2([a, b]) => [a, b, zero(), zero()],
                IntoElems::_1([a]) => [a, zero(), zero(), zero()],
                IntoElems::_0([]) => [zero(), zero(), zero(), zero()],
            },
        })
    }

    pub(in super::super) fn from_children_or_else_hash(
        children: [Insert<Child>; 4],
    ) -> Insert<Self> {
        match Children::try_from(children) {
            Ok(children) => Insert::Keep(Self {
                hash: CachedHash::default(),
                children,
            }),
            Err([a, b, c, d]) => {
                // If there were no witnessed children, compute a hash for this node based on the
                // node's height and the hashes of its children.
                Insert::Hash(Hash::node(<Self as Height>::Height::HEIGHT, a, b, c, d))
            }
        }
    }

    /// Get the children of this node as an array of either children or hashes.
    pub fn children(&self) -> [Insert<&Child>; 4] {
        self.children.children()
    }
}

impl<Child: Height> Height for Node<Child> {
    type Height = Succ<Child::Height>;
}

impl<Child: Complete> Complete for Node<Child> {
    type Focus = active::Node<Child::Focus>;
}

impl<Child: Height + GetHash> GetHash for Node<Child> {
    #[inline]
    fn hash(&self) -> Hash {
        self.hash.set_if_empty(|| {
            let [a, b, c, d] = self.children.children().map(|x| x.hash());
            Hash::node(<Self as Height>::Height::HEIGHT, a, b, c, d)
        })
    }

    #[inline]
    fn cached_hash(&self) -> Option<Hash> {
        self.hash.get()
    }
}

impl<Child: GetHash + Witness> Witness for Node<Child> {
    type Item = Child::Item;

    #[inline]
    fn witness(&self, index: impl Into<u64>) -> Option<(AuthPath<Self>, Self::Item)> {
        let index = index.into();

        // Which way to go down the tree from this node
        let (which_way, index) = WhichWay::at(Self::Height::HEIGHT, index);

        // Select the child we should be witnessing
        let (child, siblings) = which_way.pick(self.children());

        // Hash all the other siblings
        let siblings = siblings.map(|sibling| sibling.hash());

        // Witness the selected child
        let (child, leaf) = child.keep()?.witness(index)?;

        Some((path::Node { siblings, child }, leaf))
    }
}

impl<Child: GetHash + ForgetOwned> ForgetOwned for Node<Child> {
    #[inline]
    fn forget_owned(self, index: impl Into<u64>) -> (Insert<Self>, bool) {
        let index = index.into();

        let [a, b, c, d]: [Insert<Child>; 4] = self.children.into();

        // Which child should we be forgetting?
        let (which_way, index) = WhichWay::at(Self::Height::HEIGHT, index);

        // Recursively forget the appropriate child
        let (children, forgotten) = match which_way {
            WhichWay::Leftmost => {
                let (a, forgotten) = match a {
                    Insert::Keep(a) => a.forget_owned(index),
                    Insert::Hash(_) => (a, false),
                };
                ([a, b, c, d], forgotten)
            }
            WhichWay::Left => {
                let (b, forgotten) = match b {
                    Insert::Keep(b) => b.forget_owned(index),
                    Insert::Hash(_) => (b, false),
                };
                ([a, b, c, d], forgotten)
            }
            WhichWay::Right => {
                let (c, forgotten) = match c {
                    Insert::Keep(c) => c.forget_owned(index),
                    Insert::Hash(_) => (c, false),
                };
                ([a, b, c, d], forgotten)
            }
            WhichWay::Rightmost => {
                let (d, forgotten) = match d {
                    Insert::Keep(d) => d.forget_owned(index),
                    Insert::Hash(_) => (d, false),
                };
                ([a, b, c, d], forgotten)
            }
        };

        // Reconstruct the node from the children, or else (if all the children are hashes) hash
        // those hashes into a single node hash
        let reconstructed = Self::from_children_or_else_hash(children);

        // If the node was reconstructed, we know that its hash should not have changed, so carry
        // over the old cached hash, if any existed, to prevent recomputation
        let reconstructed = reconstructed.map(|node| {
            if let Some(hash) = self.hash.get() {
                node.set_hash_unchecked(hash);
            }
            node
        });

        (reconstructed, forgotten)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn check_node_size() {
        static_assertions::assert_eq_size!(Node<()>, [u8; 56]);
    }
}
