SELECT MIN(n.name) AS actor, MIN(t.title) AS movie
FROM aka_name AS an, cast_info AS ci, info_type AS it, link_type AS lt, movie_link AS ml, name AS n, person_info AS pi, title AS t
WHERE it.info = 'mini biography'
  AND lt.link = 'features'
  AND t.production_year BETWEEN 1980 AND 1995
  AND n.id = an.person_id
  AND n.id = pi.person_id
  AND pi.info_type_id = it.id
  AND n.id = ci.person_id
  AND ci.movie_id = t.id
  AND t.id = ml.linked_movie_id
  AND ml.link_type_id = lt.id;
