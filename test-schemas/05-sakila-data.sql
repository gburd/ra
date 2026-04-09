-- Sakila DVD Rental Schema Data Generation
-- Generates 1000 films, 500 actors, 5000 film_actor relations, 2000 inventory items, 10000 rentals

\echo 'Generating Sakila test data...'

-- Generate 1,000 films
INSERT INTO film (
  title, description, release_year, language_id,
  rental_duration, rental_rate, length, replacement_cost,
  rating, special_features
)
SELECT
  'Film Title ' || i,
  'Description for film ' || i,
  1950 + (random() * 70)::int,
  1 + (random() * 5)::int,
  3 + (random() * 7)::int,
  (0.99 + random() * 5)::decimal(4,2),
  60 + (random() * 120)::int,
  (9.99 + random() * 20)::decimal(5,2),
  (ARRAY['G', 'PG', 'PG-13', 'R', 'NC-17'])[1 + (random() * 4)::int],
  (ARRAY['Trailers', 'Commentaries', 'Deleted Scenes', 'Behind the Scenes'])[1 + (random() * 3)::int]
FROM generate_series(1, 1000) AS i;

\echo 'Generated 1,000 films'

-- Generate 500 actors
INSERT INTO actor (first_name, last_name, last_update)
SELECT
  (ARRAY['John', 'Jane', 'Michael', 'Sarah', 'David', 'Emma', 'Robert', 'Lisa', 'James', 'Mary'])[1 + (random() * 9)::int],
  (ARRAY['Smith', 'Johnson', 'Williams', 'Brown', 'Jones', 'Garcia', 'Miller', 'Davis', 'Rodriguez', 'Martinez'])[1 + (random() * 9)::int],
  CURRENT_TIMESTAMP
FROM generate_series(1, 500) AS i;

\echo 'Generated 500 actors'

-- Generate 5,000 film_actor relations
INSERT INTO film_actor (actor_id, film_id, last_update)
SELECT DISTINCT ON (actor_id, film_id)
  1 + (random() * 499)::int,
  1 + (random() * 999)::int,
  CURRENT_TIMESTAMP
FROM generate_series(1, 5000) AS i;

\echo 'Generated 5,000 film_actor relations'

-- Generate 2,000 inventory items
INSERT INTO inventory (film_id, store_id, last_update)
SELECT
  1 + (random() * 999)::int,
  1 + (random() * 1)::int,
  CURRENT_TIMESTAMP
FROM generate_series(1, 2000) AS i;

\echo 'Generated 2,000 inventory items'

-- Generate 10,000 rentals
INSERT INTO rental (
  rental_date, inventory_id, customer_id,
  return_date, staff_id, last_update
)
SELECT
  CURRENT_TIMESTAMP - (random() * 365 || ' days')::interval,
  1 + (random() * 1999)::int,
  1 + (random() * 599)::int,
  CURRENT_TIMESTAMP - (random() * 350 || ' days')::interval,
  1 + (random() * 1)::int,
  CURRENT_TIMESTAMP
FROM generate_series(1, 10000) AS i;

\echo 'Generated 10,000 rentals'

-- Create indexes
CREATE INDEX IF NOT EXISTS idx_film_language_id ON film(language_id);
CREATE INDEX IF NOT EXISTS idx_film_rating ON film(rating);
CREATE INDEX IF NOT EXISTS idx_film_actor_actor_id ON film_actor(actor_id);
CREATE INDEX IF NOT EXISTS idx_film_actor_film_id ON film_actor(film_id);
CREATE INDEX IF NOT EXISTS idx_inventory_film_id ON inventory(film_id);
CREATE INDEX IF NOT EXISTS idx_rental_inventory_id ON rental(inventory_id);
CREATE INDEX IF NOT EXISTS idx_rental_customer_id ON rental(customer_id);
CREATE INDEX IF NOT EXISTS idx_rental_date ON rental(rental_date);

\echo 'Sakila test data generation complete'
