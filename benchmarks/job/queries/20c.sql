SELECT MIN(t.title) AS movie
FROM cast_info AS ci, char_name AS chn, comp_cast_type AS cct1, comp_cast_type AS cct2, complete_cast AS cc, keyword AS k, kind_type AS kt, movie_keyword AS mk, name AS n, title AS t
WHERE cct1.kind = 'cast'
  AND cct2.kind = 'complete'
  AND k.keyword = 'superhero'
  AND kt.kind = 'movie'
  AND t.production_year > 2000
  AND t.kind_id = kt.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = ci.movie_id
  AND ci.person_id = n.id
  AND ci.person_role_id = chn.id
  AND t.id = cc.movie_id
  AND cc.subject_id = cct1.id
  AND cc.status_id = cct2.id;
