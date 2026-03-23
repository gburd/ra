SELECT MIN(an.name) AS alt_name, MIN(t.title) AS movie
FROM aka_name AS an, cast_info AS ci, company_name AS cn, movie_companies AS mc, name AS n, role_type AS rt, title AS t
WHERE cn.country_code = '[jp]'
  AND rt.role = 'actress'
  AND n.id = an.person_id
  AND n.id = ci.person_id
  AND ci.role_id = rt.id
  AND ci.movie_id = t.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id;
