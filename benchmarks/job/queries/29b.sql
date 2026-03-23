SELECT MIN(chn.name) AS character, MIN(n.name) AS actress, MIN(t.title) AS movie
FROM aka_name AS an, cast_info AS ci, char_name AS chn, comp_cast_type AS cct1, comp_cast_type AS cct2, company_name AS cn, complete_cast AS cc, info_type AS it, info_type AS it3, keyword AS k, movie_companies AS mc, movie_info AS mi, movie_keyword AS mk, name AS n, person_info AS pi, role_type AS rt, title AS t
WHERE cct1.kind = 'cast'
  AND cct2.kind = 'complete+verified'
  AND cn.country_code = '[us]'
  AND it.info = 'release dates'
  AND it3.info = 'trivia'
  AND k.keyword = 'computer-animation'
  AND rt.role = 'actress'
  AND n.gender = 'f'
  AND t.production_year BETWEEN 2000 AND 2005
  AND n.id = an.person_id
  AND n.id = ci.person_id
  AND ci.role_id = rt.id
  AND ci.person_role_id = chn.id
  AND n.id = pi.person_id
  AND pi.info_type_id = it3.id
  AND ci.movie_id = t.id
  AND t.id = mi.movie_id
  AND mi.info_type_id = it.id
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = mc.movie_id
  AND mc.company_id = cn.id
  AND t.id = cc.movie_id
  AND cc.subject_id = cct1.id
  AND cc.status_id = cct2.id;
