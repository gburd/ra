SELECT MIN(cn.name) AS company, MIN(lt.link) AS link_type, MIN(t.title) AS movie
FROM comp_cast_type AS cct1, comp_cast_type AS cct2, company_name AS cn, company_type AS ct, complete_cast AS cc, keyword AS k, link_type AS lt, movie_companies AS mc, movie_info AS mi, movie_keyword AS mk, movie_link AS ml, title AS t
WHERE cct1.kind = 'cast'
  AND cct2.kind = 'complete'
  AND cn.country_code <> '[pl]'
  AND ct.kind = 'production companies'
  AND k.keyword = 'sequel'
  AND lt.link = 'follows'
  AND t.production_year BETWEEN 1998 AND 1998
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_type_id = ct.id
  AND mc.company_id = cn.id
  AND t.id = mi.movie_id
  AND t.id = ml.movie_id
  AND ml.link_type_id = lt.id
  AND t.id = cc.movie_id
  AND cc.subject_id = cct1.id
  AND cc.status_id = cct2.id;
