SELECT MIN(cn.name) AS company, MIN(lt.link) AS link_type, MIN(t.title) AS movie
FROM company_name AS cn, company_type AS ct, keyword AS k, link_type AS lt, movie_companies AS mc, movie_keyword AS mk, movie_link AS ml, title AS t
WHERE cn.country_code <> '[pl]'
  AND ct.kind = 'production companies'
  AND k.keyword = 'sequel'
  AND lt.link = 'follows'
  AND t.production_year BETWEEN 1950 AND 2000
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_type_id = ct.id
  AND mc.company_id = cn.id
  AND t.id = ml.movie_id
  AND ml.link_type_id = lt.id;
