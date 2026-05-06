-- IMDB Schema for the Join Order Benchmark (JOB)
--
-- Source: Leis et al. (2015), "How Good Are Query Optimizers, Really?"
-- https://vldb.org/pvldb/vol9/p204-leis.pdf
--
-- Usage:
--   psql -d bench -f scripts/imdb-schema.sql
--
-- Load data (after downloading IMDB CSV dumps):
--   for f in title cast_info movie_info movie_keyword ...; do
--     psql -d bench -c "\copy ${f} FROM '${f}.csv' CSV"
--   done
--
-- Approximate row counts (full dataset):
--   title          2,528,312   name         4,167,491   cast_info    36,244,344
--   movie_info    14,835,720   movie_keyword 4,523,930   keyword        134,170
--   company_name     234,997   movie_companies 2,609,129  info_type          113
--   char_name      3,140,339   role_type           12    kind_type             7
--   link_type           18    company_type          4    movie_link       29,997
--   aka_title       361,472   aka_name        901,343   movie_info_idx 1,380,035
--   person_info   2,963,664   complete_cast   135,086   comp_cast_type         4

-- Drop existing tables (safe re-run)
DROP TABLE IF EXISTS complete_cast   CASCADE;
DROP TABLE IF EXISTS person_info     CASCADE;
DROP TABLE IF EXISTS movie_link      CASCADE;
DROP TABLE IF EXISTS aka_name        CASCADE;
DROP TABLE IF EXISTS movie_info_idx  CASCADE;
DROP TABLE IF EXISTS movie_info      CASCADE;
DROP TABLE IF EXISTS movie_keyword   CASCADE;
DROP TABLE IF EXISTS movie_companies CASCADE;
DROP TABLE IF EXISTS cast_info       CASCADE;
DROP TABLE IF EXISTS aka_title       CASCADE;
DROP TABLE IF EXISTS title           CASCADE;
DROP TABLE IF EXISTS char_name       CASCADE;
DROP TABLE IF EXISTS name            CASCADE;
DROP TABLE IF EXISTS keyword         CASCADE;
DROP TABLE IF EXISTS company_name    CASCADE;
DROP TABLE IF EXISTS info_type       CASCADE;
DROP TABLE IF EXISTS kind_type       CASCADE;
DROP TABLE IF EXISTS role_type       CASCADE;
DROP TABLE IF EXISTS link_type       CASCADE;
DROP TABLE IF EXISTS company_type    CASCADE;
DROP TABLE IF EXISTS comp_cast_type  CASCADE;

-- ============================================================
-- Lookup / dimension tables
-- ============================================================

CREATE TABLE kind_type (
    id    SERIAL PRIMARY KEY,
    kind  VARCHAR(15)
);

CREATE TABLE role_type (
    id    SERIAL PRIMARY KEY,
    role  VARCHAR(32)
);

CREATE TABLE info_type (
    id    SERIAL PRIMARY KEY,
    info  VARCHAR(32)
);

CREATE TABLE link_type (
    id    SERIAL PRIMARY KEY,
    link  VARCHAR(32)
);

CREATE TABLE company_type (
    id    SERIAL PRIMARY KEY,
    kind  VARCHAR(32)
);

CREATE TABLE comp_cast_type (
    id    SERIAL PRIMARY KEY,
    kind  VARCHAR(32)
);

-- ============================================================
-- Core entity tables
-- ============================================================

CREATE TABLE title (
    id               SERIAL PRIMARY KEY,
    title            TEXT         NOT NULL,
    imdb_index       VARCHAR(12),
    kind_id          INT REFERENCES kind_type(id),
    production_year  INT,
    imdb_id          INT,
    phonetic_code    VARCHAR(5),
    episode_of_id    INT,
    season_nr        INT,
    episode_nr       INT,
    series_years     VARCHAR(49),
    md5sum           VARCHAR(32)
);

CREATE TABLE name (
    id              SERIAL PRIMARY KEY,
    name            TEXT    NOT NULL,
    imdb_index      VARCHAR(12),
    imdb_id         INT,
    gender          VARCHAR(1),
    name_pcode_cf   VARCHAR(5),
    name_pcode_nf   VARCHAR(5),
    surname_pcode   VARCHAR(5),
    md5sum          VARCHAR(32)
);

CREATE TABLE char_name (
    id              SERIAL PRIMARY KEY,
    name            TEXT    NOT NULL,
    imdb_index      VARCHAR(12),
    imdb_id         INT,
    name_pcode_nf   VARCHAR(5),
    surname_pcode   VARCHAR(5),
    md5sum          VARCHAR(32)
);

CREATE TABLE keyword (
    id             SERIAL PRIMARY KEY,
    keyword        TEXT    NOT NULL,
    phonetic_code  VARCHAR(5)
);

CREATE TABLE company_name (
    id           SERIAL PRIMARY KEY,
    name         TEXT    NOT NULL,
    country_code VARCHAR(6),
    imdb_id      INT,
    name_pcode_nf VARCHAR(5),
    name_pcode_sf VARCHAR(5),
    md5sum        VARCHAR(32)
);

-- ============================================================
-- Relationship tables
-- ============================================================

CREATE TABLE aka_title (
    id               SERIAL PRIMARY KEY,
    movie_id         INT REFERENCES title(id),
    title            TEXT,
    imdb_index       VARCHAR(12),
    kind_id          INT REFERENCES kind_type(id),
    production_year  INT,
    phonetic_code    VARCHAR(5),
    episode_of_id    INT,
    season_nr        INT,
    episode_nr       INT,
    note             TEXT,
    md5sum           VARCHAR(32)
);

CREATE TABLE aka_name (
    id            SERIAL PRIMARY KEY,
    person_id     INT REFERENCES name(id),
    name          TEXT,
    imdb_index    VARCHAR(12),
    name_pcode_cf VARCHAR(5),
    name_pcode_nf VARCHAR(5),
    surname_pcode VARCHAR(5),
    md5sum        VARCHAR(32)
);

CREATE TABLE cast_info (
    id             SERIAL PRIMARY KEY,
    person_id      INT REFERENCES name(id),
    movie_id       INT REFERENCES title(id),
    person_role_id INT REFERENCES char_name(id),
    note           TEXT,
    nr_order       INT,
    role_id        INT REFERENCES role_type(id)
);

CREATE TABLE movie_companies (
    id              SERIAL PRIMARY KEY,
    movie_id        INT REFERENCES title(id),
    company_id      INT REFERENCES company_name(id),
    company_type_id INT REFERENCES company_type(id),
    note            TEXT
);

CREATE TABLE movie_info (
    id           SERIAL PRIMARY KEY,
    movie_id     INT REFERENCES title(id),
    info_type_id INT REFERENCES info_type(id),
    info         TEXT,
    note         VARCHAR(255)
);

CREATE TABLE movie_info_idx (
    id           SERIAL PRIMARY KEY,
    movie_id     INT REFERENCES title(id),
    info_type_id INT REFERENCES info_type(id),
    info         TEXT,
    note         VARCHAR(255)
);

CREATE TABLE movie_keyword (
    id         SERIAL PRIMARY KEY,
    movie_id   INT REFERENCES title(id),
    keyword_id INT REFERENCES keyword(id)
);

CREATE TABLE movie_link (
    id              SERIAL PRIMARY KEY,
    movie_id        INT REFERENCES title(id),
    linked_movie_id INT REFERENCES title(id),
    link_type_id    INT REFERENCES link_type(id)
);

CREATE TABLE person_info (
    id           SERIAL PRIMARY KEY,
    person_id    INT REFERENCES name(id),
    info_type_id INT REFERENCES info_type(id),
    info         TEXT,
    note         VARCHAR(255)
);

CREATE TABLE complete_cast (
    id         SERIAL PRIMARY KEY,
    movie_id   INT REFERENCES title(id),
    subject_id INT REFERENCES comp_cast_type(id),
    status_id  INT REFERENCES comp_cast_type(id)
);

-- ============================================================
-- Indexes (mirrors what JOB paper uses)
-- ============================================================

CREATE INDEX idx_aka_title_movie_id         ON aka_title(movie_id);
CREATE INDEX idx_aka_name_person_id         ON aka_name(person_id);
CREATE INDEX idx_cast_info_movie_id         ON cast_info(movie_id);
CREATE INDEX idx_cast_info_person_id        ON cast_info(person_id);
CREATE INDEX idx_cast_info_person_role_id   ON cast_info(person_role_id);
CREATE INDEX idx_cast_info_role_id          ON cast_info(role_id);
CREATE INDEX idx_movie_companies_movie_id   ON movie_companies(movie_id);
CREATE INDEX idx_movie_companies_company_id ON movie_companies(company_id);
CREATE INDEX idx_movie_info_movie_id        ON movie_info(movie_id);
CREATE INDEX idx_movie_info_info_type_id    ON movie_info(info_type_id);
CREATE INDEX idx_movie_info_idx_movie_id    ON movie_info_idx(movie_id);
CREATE INDEX idx_movie_keyword_movie_id     ON movie_keyword(movie_id);
CREATE INDEX idx_movie_keyword_keyword_id   ON movie_keyword(keyword_id);
CREATE INDEX idx_movie_link_movie_id        ON movie_link(movie_id);
CREATE INDEX idx_person_info_person_id      ON person_info(person_id);
CREATE INDEX idx_complete_cast_movie_id     ON complete_cast(movie_id);
CREATE INDEX idx_title_kind_id              ON title(kind_id);
CREATE INDEX idx_title_production_year      ON title(production_year);
CREATE INDEX idx_name_gender                ON name(gender);
CREATE INDEX idx_company_name_country_code  ON company_name(country_code);

-- ============================================================
-- ANALYZE to collect statistics for the planner
-- ============================================================

ANALYZE title;
ANALYZE name;
ANALYZE char_name;
ANALYZE keyword;
ANALYZE company_name;
ANALYZE cast_info;
ANALYZE movie_companies;
ANALYZE movie_info;
ANALYZE movie_info_idx;
ANALYZE movie_keyword;
ANALYZE movie_link;
ANALYZE person_info;
ANALYZE aka_title;
ANALYZE aka_name;
ANALYZE complete_cast;
