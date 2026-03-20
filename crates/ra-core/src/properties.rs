//! Physical properties of query plans.
//!
//! Physical properties describe characteristics of the data produced
//! by a plan node, such as sort ordering or partitioning. The optimizer
//! uses these to enforce or exploit properties when choosing join
//! strategies and avoiding redundant sorts.

use serde::{Deserialize, Serialize};

use crate::algebra::{NullOrdering, SortDirection};
use crate::expr::ColumnRef;

/// A physical property that describes how data is organized.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum PhysicalProperty {
    /// Data is sorted by the given keys.
    Ordering(Ordering),
    /// Data is partitioned across nodes by the given columns.
    Partitioning(Partitioning),
    /// Data distribution across a cluster.
    Distribution(DistributionProperty),
}

/// A sort ordering over one or more columns.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct Ordering {
    /// The ordered columns with direction.
    pub columns: Vec<OrderingColumn>,
}

/// A single column in an ordering specification.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct OrderingColumn {
    /// The column reference.
    pub column: ColumnRef,
    /// Sort direction.
    pub direction: SortDirection,
    /// How NULLs are ordered relative to non-null values.
    pub nulls: NullOrdering,
}

/// A partitioning scheme describing how data is distributed.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
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

/// How data is distributed across cluster nodes.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum DistributionProperty {
    /// All data on a single node.
    SingleNode,
    /// Full copy on every node.
    Replicated,
    /// Hash-distributed across nodes by key columns.
    HashDistributed(Vec<ColumnRef>),
    /// Range-distributed across nodes by key columns.
    RangeDistributed(Vec<ColumnRef>),
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

    /// Whether this property set has no properties.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
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
    ///
    /// For ordering properties, a provided ordering that is a
    /// super-sequence of the required ordering is accepted (the
    /// data is sorted by more keys than needed, which still
    /// satisfies the requirement).
    #[must_use]
    pub fn satisfies(&self, required: &Self) -> bool {
        required.properties.iter().all(|req| {
            match req {
                PhysicalProperty::Ordering(req_ord) => {
                    self.satisfies_ordering(req_ord)
                }
                other => self.properties.contains(other),
            }
        })
    }

    /// Check whether the provided ordering satisfies a required
    /// ordering.
    ///
    /// The required ordering is satisfied when it is a prefix of
    /// (or equal to) the provided ordering.
    #[must_use]
    pub fn satisfies_ordering(
        &self,
        required: &Ordering,
    ) -> bool {
        if let Some(provided) = self.ordering() {
            required.is_prefix_of(provided)
        } else {
            required.columns.is_empty()
        }
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

    /// Return the distribution property, if present.
    #[must_use]
    pub fn distribution(&self) -> Option<&DistributionProperty> {
        self.properties.iter().find_map(|p| {
            if let PhysicalProperty::Distribution(d) = p {
                Some(d)
            } else {
                None
            }
        })
    }

    /// Create a property set with just an ordering.
    #[must_use]
    pub fn with_ordering(ordering: Ordering) -> Self {
        let mut ps = Self::new();
        ps.add(PhysicalProperty::Ordering(ordering));
        ps
    }

    /// Create a property set with just a partitioning.
    #[must_use]
    pub fn with_partitioning(part: Partitioning) -> Self {
        let mut ps = Self::new();
        ps.add(PhysicalProperty::Partitioning(part));
        ps
    }
}

impl Ordering {
    /// Create an ordering from a list of ordering columns.
    #[must_use]
    pub fn new(columns: Vec<OrderingColumn>) -> Self {
        Self { columns }
    }

    /// Whether this ordering has no columns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
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

    /// Return the common prefix between this ordering and another.
    ///
    /// Useful for incremental sort: the prefix already sorted
    /// determines how much work is saved.
    #[must_use]
    pub fn common_prefix(&self, other: &Self) -> Self {
        let cols: Vec<OrderingColumn> = self
            .columns
            .iter()
            .zip(&other.columns)
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.clone())
            .collect();
        Self::new(cols)
    }

    /// Return column references in this ordering.
    #[must_use]
    pub fn column_refs(&self) -> Vec<&ColumnRef> {
        self.columns.iter().map(|c| &c.column).collect()
    }
}

impl OrderingColumn {
    /// Create a new ordering column with default NULL ordering
    /// (NULLS LAST for ASC, NULLS FIRST for DESC, matching
    /// `PostgreSQL` defaults).
    #[must_use]
    pub fn new(
        column: ColumnRef,
        direction: SortDirection,
    ) -> Self {
        let nulls = match direction {
            SortDirection::Asc => NullOrdering::Last,
            SortDirection::Desc => NullOrdering::First,
        };
        Self {
            column,
            direction,
            nulls,
        }
    }

    /// Create an ordering column with explicit NULL ordering.
    #[must_use]
    pub fn with_nulls(
        column: ColumnRef,
        direction: SortDirection,
        nulls: NullOrdering,
    ) -> Self {
        Self {
            column,
            direction,
            nulls,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::{NullOrdering, SortDirection};
    use crate::expr::ColumnRef;

    fn ordering_col(
        name: &str,
        dir: SortDirection,
    ) -> OrderingColumn {
        OrderingColumn::new(ColumnRef::new(name), dir)
    }

    #[test]
    fn ordering_column_default_nulls() {
        let asc = OrderingColumn::new(
            ColumnRef::new("x"),
            SortDirection::Asc,
        );
        assert_eq!(asc.nulls, NullOrdering::Last);

        let desc = OrderingColumn::new(
            ColumnRef::new("x"),
            SortDirection::Desc,
        );
        assert_eq!(desc.nulls, NullOrdering::First);
    }

    #[test]
    fn ordering_column_explicit_nulls() {
        let col = OrderingColumn::with_nulls(
            ColumnRef::new("x"),
            SortDirection::Asc,
            NullOrdering::First,
        );
        assert_eq!(col.nulls, NullOrdering::First);
    }

    #[test]
    fn property_set_add_dedup() {
        let mut ps = PropertySet::new();
        let prop =
            PhysicalProperty::Partitioning(Partitioning::Single);
        ps.add(prop.clone());
        ps.add(prop);
        assert_eq!(ps.properties.len(), 1);
    }

    #[test]
    fn property_set_is_empty() {
        let ps = PropertySet::new();
        assert!(ps.is_empty());
        let ps = PropertySet::with_partitioning(
            Partitioning::Single,
        );
        assert!(!ps.is_empty());
    }

    #[test]
    fn property_set_contains() {
        let mut ps = PropertySet::new();
        let prop =
            PhysicalProperty::Partitioning(Partitioning::Single);
        assert!(!ps.contains(&prop));
        ps.add(prop.clone());
        assert!(ps.contains(&prop));
    }

    #[test]
    fn property_set_satisfies() {
        let mut provided = PropertySet::new();
        provided.add(PhysicalProperty::Partitioning(
            Partitioning::Single,
        ));
        provided.add(PhysicalProperty::Ordering(Ordering::new(
            vec![ordering_col("id", SortDirection::Asc)],
        )));

        let mut required = PropertySet::new();
        required.add(PhysicalProperty::Partitioning(
            Partitioning::Single,
        ));
        assert!(provided.satisfies(&required));

        let mut extra_required = PropertySet::new();
        extra_required.add(PhysicalProperty::Partitioning(
            Partitioning::Broadcast,
        ));
        assert!(!provided.satisfies(&extra_required));
    }

    #[test]
    fn property_set_satisfies_ordering_prefix() {
        let mut provided = PropertySet::new();
        provided.add(PhysicalProperty::Ordering(Ordering::new(
            vec![
                ordering_col("a", SortDirection::Asc),
                ordering_col("b", SortDirection::Desc),
            ],
        )));

        // Requiring [a ASC] is satisfied by [a ASC, b DESC]
        let required = PropertySet::with_ordering(Ordering::new(
            vec![ordering_col("a", SortDirection::Asc)],
        ));
        assert!(provided.satisfies(&required));

        // Requiring [a ASC, b DESC] is satisfied exactly
        let required_full =
            PropertySet::with_ordering(Ordering::new(vec![
                ordering_col("a", SortDirection::Asc),
                ordering_col("b", SortDirection::Desc),
            ]));
        assert!(provided.satisfies(&required_full));

        // Requiring [a ASC, b ASC] is NOT satisfied
        let required_wrong =
            PropertySet::with_ordering(Ordering::new(vec![
                ordering_col("a", SortDirection::Asc),
                ordering_col("b", SortDirection::Asc),
            ]));
        assert!(!provided.satisfies(&required_wrong));
    }

    #[test]
    fn property_set_ordering_accessor() {
        let mut ps = PropertySet::new();
        assert!(ps.ordering().is_none());
        ps.add(PhysicalProperty::Ordering(Ordering::new(vec![
            ordering_col("x", SortDirection::Desc),
        ])));
        let o = ps.ordering().expect("ordering present");
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
    fn property_set_distribution_accessor() {
        let mut ps = PropertySet::new();
        assert!(ps.distribution().is_none());
        ps.add(PhysicalProperty::Distribution(
            DistributionProperty::Replicated,
        ));
        assert_eq!(
            ps.distribution(),
            Some(&DistributionProperty::Replicated)
        );
    }

    #[test]
    fn ordering_prefix() {
        let short = Ordering::new(vec![ordering_col(
            "a",
            SortDirection::Asc,
        )]);
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
        let a = Ordering::new(vec![ordering_col(
            "x",
            SortDirection::Asc,
        )]);
        let b = Ordering::new(vec![ordering_col(
            "x",
            SortDirection::Desc,
        )]);
        assert!(!a.is_prefix_of(&b));
    }

    #[test]
    fn ordering_common_prefix() {
        let a = Ordering::new(vec![
            ordering_col("x", SortDirection::Asc),
            ordering_col("y", SortDirection::Asc),
            ordering_col("z", SortDirection::Desc),
        ]);
        let b = Ordering::new(vec![
            ordering_col("x", SortDirection::Asc),
            ordering_col("y", SortDirection::Asc),
            ordering_col("w", SortDirection::Asc),
        ]);
        let prefix = a.common_prefix(&b);
        assert_eq!(prefix.columns.len(), 2);
        assert_eq!(prefix.columns[0].column.column, "x");
        assert_eq!(prefix.columns[1].column.column, "y");
    }

    #[test]
    fn ordering_common_prefix_empty() {
        let a = Ordering::new(vec![ordering_col(
            "x",
            SortDirection::Asc,
        )]);
        let b = Ordering::new(vec![ordering_col(
            "y",
            SortDirection::Asc,
        )]);
        let prefix = a.common_prefix(&b);
        assert!(prefix.is_empty());
    }

    #[test]
    fn ordering_is_empty() {
        let empty = Ordering::new(vec![]);
        assert!(empty.is_empty());
        let non_empty = Ordering::new(vec![ordering_col(
            "a",
            SortDirection::Asc,
        )]);
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn ordering_column_refs() {
        let ord = Ordering::new(vec![
            ordering_col("a", SortDirection::Asc),
            ordering_col("b", SortDirection::Desc),
        ]);
        let refs = ord.column_refs();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].column, "a");
        assert_eq!(refs[1].column, "b");
    }

    #[test]
    fn with_ordering_constructor() {
        let ps = PropertySet::with_ordering(Ordering::new(vec![
            ordering_col("id", SortDirection::Asc),
        ]));
        assert!(ps.ordering().is_some());
        assert!(ps.partitioning().is_none());
    }

    #[test]
    fn with_partitioning_constructor() {
        let ps = PropertySet::with_partitioning(
            Partitioning::Hash(vec![ColumnRef::new("id")]),
        );
        assert!(ps.partitioning().is_some());
        assert!(ps.ordering().is_none());
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
        ps.add(PhysicalProperty::Distribution(
            DistributionProperty::HashDistributed(vec![
                ColumnRef::new("id"),
            ]),
        ));

        let json = serde_json::to_string(&ps)
            .expect("serialization should succeed");
        let deserialized: PropertySet = serde_json::from_str(&json)
            .expect("deserialization should succeed");
        assert_eq!(ps, deserialized);
    }

    #[test]
    fn distribution_property_variants() {
        let single = DistributionProperty::SingleNode;
        let repl = DistributionProperty::Replicated;
        let hash = DistributionProperty::HashDistributed(vec![
            ColumnRef::new("k"),
        ]);
        let range = DistributionProperty::RangeDistributed(vec![
            ColumnRef::new("k"),
        ]);
        assert_ne!(single, repl);
        assert_ne!(hash, range);
    }
}
