# Rule: "Join Pushdown: Hash Partition Join"

**Category:** federated/pushdown
**File:** `rules/federated/join-pushdown-remote-hash-partition.rra`

## Metadata

- **ID:** `federated-join-pushdown-hash-partition`
- **Version:** "1.0.0"
- **Databases:** postgresql, snowflake, bigquery, spark
- **Tags:** federated, pushdown, join, hash-partition, distributed
- **Authors:** "ra-optimizer"


# Join Pushdown: Hash Partition Join

## Description

When both tables are large and on different remotes, use hash
partitioning on the join key. Each remote filters to its partition
of the hash space, reducing the data transferred to only matching
hash partitions.

## Relational Algebra

```algebra
Join[cond](RemoteScan[t1, ep1], RemoteScan[t2, ep2])
=>
Union[all](
  for each partition p:
    Join[cond](
      RemoteScan[t1, ep1, pushdown_filter=(hash(key) % N = p)],
      RemoteScan[t2, ep2, pushdown_filter=(hash(key) % N = p)]))

Preconditions:
  - Both tables on different endpoints
  - Both tables large (> 1M rows)
```

## Before

```
(Join :type INNER :condition (= t1.key t2.key)
  :left (RemoteScan "table1" "site-a.com")
  :right (RemoteScan "table2" "site-b.com"))
```

## After

```
(Union :all true
  :children [
    (Join :type INNER :condition (= t1.key t2.key)
      :left (RemoteScan "table1" "site-a.com"
        :pushdown_filter (= (% (hash key) 4) 0))
      :right (RemoteScan "table2" "site-b.com"
        :pushdown_filter (= (% (hash key) 4) 0)))
    ...])
```

## Test Cases

### Test 1: Hash partition join

#### Input
```
(Join :type INNER :condition (= t1.key t2.key)
  :left (RemoteScan "table1" "site-a.com")
  :right (RemoteScan "table2" "site-b.com"))
```

#### Expected
```
(HashPartitionJoin :partitions 4
  :left (RemoteScan "table1" "site-a.com")
  :right (RemoteScan "table2" "site-b.com"))
```
