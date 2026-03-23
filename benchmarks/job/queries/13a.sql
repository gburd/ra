SELECT MIN(cn.name) AS company, MIN(mi_idx.info) AS rating, MIN(t.title) AS movie
FROM company_name AS cn, company_type AS ct, info_type AS it, info_type AS it2, kind_type AS kt, movie_companies AS mc, movie_info AS mi, movie_info_idx AS mi_idx, title AS t
WHERE cn.country_code = '[de]'
  AND ct.kind = 'production companies'
  AND it.info = 'rating'
  AND it2.info = 'release dates'
  AND kt.kind = 'movie'
  AND t.kind_id = kt.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it2.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it.id
  AND t.id = mc.movie_id
  AND mc.company_type_id = ct.id
  AND mc.company_id = cn.id;
