SELECT MIN(mi.info) AS genres, MIN(mi_idx.info) AS votes, MIN(n.name) AS actor, MIN(t.title) AS movie
FROM cast_info AS ci, company_name AS cn, info_type AS it1, info_type AS it2, keyword AS k, movie_companies AS mc, movie_info AS mi, movie_info_idx AS mi_idx, movie_keyword AS mk, name AS n, title AS t
WHERE cn.name = 'Lionsgate'
  AND it1.info = 'genres'
  AND it2.info = 'votes'
  AND k.keyword = 'murder'
  AND n.gender = 'm'
  AND n.id = ci.person_id
  AND ci.movie_id = t.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it1.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it2.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id;
