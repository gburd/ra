SELECT MIN(chn.name) AS character, MIN(mi_idx.info) AS rating, MIN(n.name) AS actor, MIN(t.title) AS movie
FROM cast_info AS ci, char_name AS chn, comp_cast_type AS cct1, comp_cast_type AS cct2, complete_cast AS cc, info_type AS it, keyword AS k, kind_type AS kt, movie_info_idx AS mi_idx, movie_keyword AS mk, name AS n, title AS t
WHERE cct1.kind = 'cast'
  AND cct2.kind = 'complete'
  AND it.info = 'rating'
  AND k.keyword = 'superhero'
  AND kt.kind = 'movie'
  AND t.production_year > 2005
  AND t.kind_id = kt.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = ci.movie_id
  AND ci.person_id = n.id
  AND ci.person_role_id = chn.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it.id
  AND t.id = cc.movie_id
  AND cc.subject_id = cct1.id
  AND cc.status_id = cct2.id;
