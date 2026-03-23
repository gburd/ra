SELECT MIN(chn.name) AS character, MIN(t.title) AS movie
FROM cast_info AS ci, char_name AS chn, company_name AS cn, company_type AS ct, movie_companies AS mc, role_type AS rt, title AS t
WHERE cn.country_code = '[ru]'
  AND rt.role = 'actor'
  AND t.production_year > 2010
  AND ci.role_id = rt.id
  AND ci.person_role_id = chn.id
  AND ci.movie_id = t.id
  AND t.id = mc.movie_id
  AND mc.company_type_id = ct.id
  AND mc.company_id = cn.id;
