SELECT MIN(mc.note) AS production_note,
       MIN(t.title) AS movie_title,
       MIN(t.production_year) AS movie_year
FROM company_type AS ct,
     info_type AS it,
     movie_companies AS mc,
     movie_info_idx AS mi_idx,
     title AS t
WHERE ct.kind = 'production companies'
  AND it.info = 'top 250 rank'
  AND mc.company_type_id = ct.id
  AND mc.movie_id = t.id
  AND t.id = mi_idx.movie_id
  AND mi_idx.info_type_id = it.id;
