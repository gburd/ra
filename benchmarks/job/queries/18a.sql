SELECT MIN(mi.info) AS budget, MIN(mi_idx.info) AS votes, MIN(t.title) AS movie
FROM cast_info AS ci, info_type AS it1, info_type AS it2, movie_info AS mi, movie_info_idx AS mi_idx, name AS n, title AS t
WHERE it1.info = 'budget'
  AND it2.info = 'votes'
  AND n.gender = 'm'
  AND n.id = ci.person_id
  AND ci.movie_id = t.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it1.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it2.id;
