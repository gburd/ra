SELECT MIN(kt.kind) AS kind, MIN(t.title) AS movie
FROM comp_cast_type AS cct1, company_name AS cn, company_type AS ct, complete_cast AS cc, info_type AS it, keyword AS k, kind_type AS kt, movie_companies AS mc, movie_info AS mi, movie_keyword AS mk, title AS t
WHERE cct1.kind = 'complete+verified'
  AND cn.country_code = '[us]'
  AND it.info = 'release dates'
  AND kt.kind = 'movie'
  AND t.production_year > 2000
  AND t.kind_id = kt.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id
  AND mc.company_type_id = ct.id
  AND t.id = cc.movie_id
  AND cc.status_id = cct1.id;
