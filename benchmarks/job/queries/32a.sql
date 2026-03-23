SELECT MIN(lt.link) AS link_type, MIN(t1.title) AS movie1, MIN(t2.title) AS movie2
FROM keyword AS k, link_type AS lt, movie_keyword AS mk, movie_link AS ml, title AS t1, title AS t2
WHERE k.keyword = '10,000-mile-club'
  AND k.id = mk.keyword_id
  AND mk.movie_id = t1.id
  AND t1.id = ml.movie_id
  AND ml.linked_movie_id = t2.id
  AND ml.link_type_id = lt.id;
