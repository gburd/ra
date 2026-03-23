SELECT MIN(t.title) AS movie_title
FROM company_type AS ct, info_type AS it, movie_companies AS mc, movie_info AS mi, title AS t
WHERE ct.kind = 'production companies'
  AND t.production_year > 2005
  AND t.id = mi.movie_id
  AND t.id = mc.movie_id
  AND mc.company_type_id = ct.id
  AND mi.info_type_id = it.id;
