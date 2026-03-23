SELECT MIN(mi_idx.info) AS rating, MIN(t.title) AS movie_title
FROM info_type AS it, keyword AS k, movie_info_idx AS mi_idx, movie_keyword AS mk, title AS t
WHERE it.info = 'rating'
  AND k.keyword = 'sequel'
  AND t.production_year > 1990
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it.id;
