SELECT MIN(mi.info) AS release_info, MIN(t.title) AS movie
FROM aka_title AS at, company_name AS cn, company_type AS ct, info_type AS it, keyword AS k, movie_companies AS mc, movie_info AS mi, movie_keyword AS mk, title AS t
WHERE cn.country_code = '[us]'
  AND it.info = 'release dates'
  AND t.production_year > 2005
  AND t.id = at.movie_id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id
  AND mc.company_type_id = ct.id;
