SELECT MIN(mi_idx.info) AS rating, MIN(t.title) AS movie
FROM info_type AS it1, info_type AS it2, keyword AS k, kind_type AS kt, movie_info AS mi, movie_info_idx AS mi_idx, movie_keyword AS mk, title AS t
WHERE it1.info = 'countries'
  AND it2.info = 'rating'
  AND k.keyword = 'murder'
  AND kt.kind = 'movie'
  AND t.production_year > 2010
  AND t.kind_id = kt.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it1.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it2.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id;
