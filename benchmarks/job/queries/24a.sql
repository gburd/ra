SELECT MIN(chn.name) AS character, MIN(n.name) AS actress, MIN(t.title) AS movie
FROM aka_name AS an, cast_info AS ci, char_name AS chn, company_name AS cn, info_type AS it, keyword AS k, movie_companies AS mc, movie_info AS mi, movie_keyword AS mk, name AS n, role_type AS rt, title AS t
WHERE cn.country_code = '[us]'
  AND it.info = 'release dates'
  AND k.keyword = 'hero'
  AND rt.role = 'actress'
  AND n.gender = 'f'
  AND t.production_year > 2010
  AND n.id = an.person_id
  AND n.id = ci.person_id
  AND ci.role_id = rt.id
  AND ci.person_role_id = chn.id
  AND ci.movie_id = t.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id;
