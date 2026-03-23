SELECT MIN(k.keyword) AS keyword, MIN(n.name) AS actor_name, MIN(t.title) AS movie_title
FROM cast_info AS ci, keyword AS k, movie_keyword AS mk, name AS n, title AS t
WHERE k.keyword = 'superhero'
  AND t.production_year > 2014
  AND t.id = mk.movie_id
  AND mk.keyword_id = k.id
  AND t.id = ci.movie_id
  AND ci.person_id = n.id;
