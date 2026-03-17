//! Physical properties of query plans.
//!
//! Physical properties describe characteristics of the data produced
//! by a plan node, such as sort ordering or partitioning. The optimizer
//! uses these to enforce or exploit properties when choosing join
//! strategies and avoiding redundant sorts.

use serde::{Deserialize, Serialize};

use crate::algebra::SortDirection;
use crate::expr::ColumnRef;

/// A physical property that describes how data is organized.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalProperty {
    /// Data is sorted by the given keys.
    Ordering(Ordering),
    /// Data is partitioned across nodes by the given columns.
    Partitioning(Partitioning),
}

/// A sort ordering over one or more columns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ordering {
    /// The ordered columns with direction.
    pub columns: Vec<OrderingColumn>,
}

/// A single column in an ordering specification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderingColumn {
    /// The column reference.
    pub column: ColumnRef,
    /// Sort direction.
    pub direction: SortDirection,
}

/// A partitioning scheme describing how data is distributed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Partitioning {
    /// Data is on a single partition (not distributed).
    Single,
    /// Data is hash-partitioned by the given columns.
    Hash(Vec<ColumnRef>),
    /// Data is range-partitioned by the given columns.
    Range(Vec<ColumnRef>),
    /// Data is broadcast to all partitions.
    Broadcast,
    /// Data is round-robin distributed.
    RoundRobin,
}

/// A set of physical properties satisfied by a plan node.
#[derive(
    Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize,
)]
pub struct PropertySet {
    /// The properties in this set.
    pub properties: Vec<PhysicalProperty>,
}

impl PropertySet {
    /// Create an empty property set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a property to the set.
    pub fn add(&mut self, prop: PhysicalProperty) {
        if !self.properties.contains(&prop) {
            self.properties.push(prop);
        }
    }

    /// Check if this set contains a specific property.
    #[must_use]
    pub fn contains(&self, prop: &PhysicalProperty) -> bool {
        self.properties.contains(prop)
    }

    /// Check if this property set satisfies all the required
    /// properties.
    #[must_use]
    pub fn satisfies(&self, required: &Self) -> bool {
        required
            .properties
            .iter()
            .all(|req| self.properties.contains(req))
    }

    /// Return the ordering property, if present.
    #[must_use]
    pub fn ordering(&self) -> Option<&Ordering> {
        self.properties.iter().find_map(|p| {
            if let PhysicalProperty::Ordering(o) = p {
                Some(o)
            } else {
                None
            }
        })
    }

    /// Return the partitioning property, if present.
    #[must_use]
    pub fn partitioning(&self) -> Option<&Partitioning> {
        self.properties.iter().find_map(|p| {
            if let PhysicalProperty::Partitioning(p) = p {
                Some(p)
            } else {
                None
            }
        })
    }
}

impl Ordering {
    /// Create an ordering from a list of (column, direction) pairs.
    #[must_use]
    pub fn new(columns: Vec<OrderingColumn>) -> Self {
        Self { columns }
    }

    /// Check if this ordering is a prefix of `other`.
    ///
    /// An ordering `[a ASC]` is a prefix of `[a ASC, b DESC]`.
    #[must_use]
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        if self.columns.len() > other.columns.len() {
            return false;
        }
        self.columns
            .iter()
            .zip(&other.columns)
            .all(|(a, b)| a == b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::SortDirection;
    use crate::expr::ColumnRef;

    fn ordering_col(
        name: &str,
        dir: SortDirection,
    ) -> OrderingColumn {
        OrderingColumn {
            column: ColumnRef::new(name),
            direction: dir,
        }
    }

    #[test]
    fn property_set_add_dedup() {
        let mut ps = PropertySet::new();
        let prop = PhysicalProperty::Partitioning(Partitioning::Single);
        ps.add(prop.clone());
        ps.add(prop);
        assert_eq!(ps.properties.len(), 1);
    }

    #[test]
    fn property_set_contains() {
        let mut ps = PropertySet::new();
        let prop = PhysicalProperty::Partitioning(Partitioning::Single);
        assert!(!ps.contains(&prop));
        ps.add(prop.clone());
        assert!(ps.contains(&prop));
    }

    #[test]
    fn property_set_satisfies() {
        let mut provided = PropertySet::new();
        provided
            .add(PhysicalProperty::Partitioning(Partitioning::Single));
        provided.add(PhysicalProperty::Ordering(Ordering::new(vec![
            ordering_col("id", SortDirection::Asc),
        ])));

        let mut required = PropertySet::new();
        required
            .add(PhysicalProperty::Partitioning(Partitioning::Single));
        assert!(provided.satisfies(&required));

        let mut extra_required = PropertySet::new();
        extra_required
            .add(PhysicalProperty::Partitioning(Partitioning::Broadcast));
        assert!(!provided.satisfies(&extra_required));
    }

    #[test]
    fn property_set_ordering_accessor() {
        let mut ps = PropertySet::new();
        assert!(ps.ordering().is_none());
        ps.add(PhysicalProperty::Ordering(Ordering::new(vec![
            ordering_col("x", SortDirection::Desc),
        ])));
        let o = ps.ordering().expect("ordering should be present");
        assert_eq!(o.columns.len(), 1);
        assert_eq!(o.columns[0].direction, SortDirection::Desc);
    }

    #[test]
    fn property_set_partitioning_accessor() {
        let mut ps = PropertySet::new();
        assert!(ps.partitioning().is_none());
        ps.add(PhysicalProperty::Partitioning(Partitioning::Hash(
            vec![ColumnRef::new("id")],
        )));
        assert!(ps.partitioning().is_some());
    }

    #[test]
    fn ordering_prefix() {
        let short = Ordering::new(vec![
            ordering_col("a", SortDirection::Asc),
        ]);
        let long = Ordering::new(vec![
            ordering_col("a", SortDirection::Asc),
            ordering_col("b", SortDirection::Desc),
        ]);
        assert!(short.is_prefix_of(&long));
        assert!(!long.is_prefix_of(&short));
        assert!(short.is_prefix_of(&short));
    }

    #[test]
    fn ordering_prefix_different_direction() {
        let a = Ordering::new(vec![
            ordering_col("x", SortDirection::Asc),
        ]);
        let b = Ordering::new(vec![
            ordering_col("x", SortDirection::Desc),
        ]);
        assert!(!a.is_prefix_of(&b));
    }

    #[test]
    fn serialize_roundtrip() {
        let mut ps = PropertySet::new();
        ps.add(PhysicalProperty::Ordering(Ordering::new(vec![
            ordering_col("id", SortDirection::Asc),
        ])));
        ps.add(PhysicalProperty::Partitioning(Partitioning::Hash(
            vec![ColumnRef::new("id")],
        )));

        let json = serde_json::to_string(&ps)
            .expect("serialization should succeed");
        let deserialized: PropertySet = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(ps, deserialized);
    }
}
