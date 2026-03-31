-- PostgreSQL array literal syntax
-- Tests: Array type, ARRAY[] constructor, :: casting

SELECT ARRAY[1, 2, 3, 4, 5]::int[] as numbers;
