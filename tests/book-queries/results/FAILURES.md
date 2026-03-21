# SQL Query Failures

Generated: Sat Mar 21 09:52:11 EDT 2026

## 07-set-operations.sql - Query 11

**Category**: set-operation

**Query**:
```sql
(SELECT employee_id FROM employees WHERE department_id = 10
 UNION
 SELECT employee_id FROM employees WHERE department_id = 20)
INTERSECT
SELECT employee_id FROM employees WHERE salary > 60000
```

**Error**: Query doesn't start with a SQL keyword

