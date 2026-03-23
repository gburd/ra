-- Join Order Benchmark (JOB) IMDB Schema
-- 21 tables from the Internet Movie Database (May 2013 snapshot)
--
-- Source: https://github.com/gregrahn/join-order-benchmark
-- Paper: "How Good Are Query Optimizers, Really?" (PVLDB Vol 9, No 3, 2015)

DROP TABLE IF EXISTS aka_name CASCADE;
DROP TABLE IF EXISTS aka_title CASCADE;
DROP TABLE IF EXISTS cast_info CASCADE;
DROP TABLE IF EXISTS char_name CASCADE;
DROP TABLE IF EXISTS comp_cast_type CASCADE;
DROP TABLE IF EXISTS company_name CASCADE;
DROP TABLE IF EXISTS company_type CASCADE;
DROP TABLE IF EXISTS complete_cast CASCADE;
DROP TABLE IF EXISTS info_type CASCADE;
DROP TABLE IF EXISTS keyword CASCADE;
DROP TABLE IF EXISTS kind_type CASCADE;
DROP TABLE IF EXISTS link_type CASCADE;
DROP TABLE IF EXISTS movie_companies CASCADE;
DROP TABLE IF EXISTS movie_info CASCADE;
DROP TABLE IF EXISTS movie_info_idx CASCADE;
DROP TABLE IF EXISTS movie_keyword CASCADE;
DROP TABLE IF EXISTS movie_link CASCADE;
DROP TABLE IF EXISTS name CASCADE;
DROP TABLE IF EXISTS person_info CASCADE;
DROP TABLE IF EXISTS role_type CASCADE;
DROP TABLE IF EXISTS title CASCADE;

CREATE TABLE aka_name (
    id integer NOT NULL PRIMARY KEY,
    person_id integer NOT NULL,
    name text NOT NULL,
    imdb_index character varying(12),
    name_pcode_cf character varying(5),
    name_pcode_nf character varying(5),
    surname_pcode character varying(5),
    md5sum character varying(32)
);

CREATE TABLE aka_title (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    title text NOT NULL,
    imdb_index character varying(12),
    kind_id integer NOT NULL,
    production_year integer,
    phonetic_code character varying(5),
    episode_of_id integer,
    season_nr integer,
    episode_nr integer,
    note text,
    md5sum character varying(32)
);

CREATE TABLE cast_info (
    id integer NOT NULL PRIMARY KEY,
    person_id integer NOT NULL,
    movie_id integer NOT NULL,
    person_role_id integer,
    note text,
    nr_order integer,
    role_id integer NOT NULL
);

CREATE TABLE char_name (
    id integer NOT NULL PRIMARY KEY,
    name text NOT NULL,
    imdb_index character varying(12),
    imdb_id integer,
    name_pcode_nf character varying(5),
    surname_pcode character varying(5),
    md5sum character varying(32)
);

CREATE TABLE comp_cast_type (
    id integer NOT NULL PRIMARY KEY,
    kind character varying(32) NOT NULL
);

CREATE TABLE company_name (
    id integer NOT NULL PRIMARY KEY,
    name text NOT NULL,
    country_code character varying(255),
    imdb_id integer,
    name_pcode_nf character varying(5),
    name_pcode_sf character varying(5),
    md5sum character varying(32)
);

CREATE TABLE company_type (
    id integer NOT NULL PRIMARY KEY,
    kind character varying(32) NOT NULL
);

CREATE TABLE complete_cast (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer,
    subject_id integer NOT NULL,
    status_id integer NOT NULL
);

CREATE TABLE info_type (
    id integer NOT NULL PRIMARY KEY,
    info character varying(32) NOT NULL
);

CREATE TABLE keyword (
    id integer NOT NULL PRIMARY KEY,
    keyword text NOT NULL,
    phonetic_code character varying(5)
);

CREATE TABLE kind_type (
    id integer NOT NULL PRIMARY KEY,
    kind character varying(15) NOT NULL
);

CREATE TABLE link_type (
    id integer NOT NULL PRIMARY KEY,
    link character varying(32) NOT NULL
);

CREATE TABLE movie_companies (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    company_id integer NOT NULL,
    company_type_id integer NOT NULL,
    note text
);

CREATE TABLE movie_info (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    info_type_id integer NOT NULL,
    info text NOT NULL,
    note text
);

CREATE TABLE movie_info_idx (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    info_type_id integer NOT NULL,
    info text NOT NULL,
    note text
);

CREATE TABLE movie_keyword (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    keyword_id integer NOT NULL
);

CREATE TABLE movie_link (
    id integer NOT NULL PRIMARY KEY,
    movie_id integer NOT NULL,
    linked_movie_id integer NOT NULL,
    link_type_id integer NOT NULL
);

CREATE TABLE name (
    id integer NOT NULL PRIMARY KEY,
    name text NOT NULL,
    imdb_index character varying(12),
    imdb_id integer,
    gender character varying(1),
    name_pcode_cf character varying(5),
    name_pcode_nf character varying(5),
    surname_pcode character varying(5),
    md5sum character varying(32)
);

CREATE TABLE person_info (
    id integer NOT NULL PRIMARY KEY,
    person_id integer NOT NULL,
    info_type_id integer NOT NULL,
    info text NOT NULL,
    note text
);

CREATE TABLE role_type (
    id integer NOT NULL PRIMARY KEY,
    role character varying(32) NOT NULL
);

CREATE TABLE title (
    id integer NOT NULL PRIMARY KEY,
    title text NOT NULL,
    imdb_index character varying(12),
    kind_id integer NOT NULL,
    production_year integer,
    imdb_id integer,
    phonetic_code character varying(5),
    episode_of_id integer,
    season_nr integer,
    episode_nr integer,
    series_years character varying(49),
    md5sum character varying(32)
);

-- Foreign key indexes (from fkindexes.sql in the JOB repository)
CREATE INDEX company_id_movie_companies ON movie_companies(company_id);
CREATE INDEX company_type_id_movie_companies ON movie_companies(company_type_id);
CREATE INDEX info_type_id_movie_info_idx ON movie_info_idx(info_type_id);
CREATE INDEX info_type_id_movie_info ON movie_info(info_type_id);
CREATE INDEX info_type_id_person_info ON person_info(info_type_id);
CREATE INDEX keyword_id_movie_keyword ON movie_keyword(keyword_id);
CREATE INDEX kind_id_aka_title ON aka_title(kind_id);
CREATE INDEX kind_id_title ON title(kind_id);
CREATE INDEX linked_movie_id_movie_link ON movie_link(linked_movie_id);
CREATE INDEX link_type_id_movie_link ON movie_link(link_type_id);
CREATE INDEX movie_id_aka_title ON aka_title(movie_id);
CREATE INDEX movie_id_cast_info ON cast_info(movie_id);
CREATE INDEX movie_id_complete_cast ON complete_cast(movie_id);
CREATE INDEX movie_id_movie_companies ON movie_companies(movie_id);
CREATE INDEX movie_id_movie_info_idx ON movie_info_idx(movie_id);
CREATE INDEX movie_id_movie_keyword ON movie_keyword(movie_id);
CREATE INDEX movie_id_movie_link ON movie_link(movie_id);
CREATE INDEX movie_id_movie_info ON movie_info(movie_id);
CREATE INDEX person_id_aka_name ON aka_name(person_id);
CREATE INDEX person_id_cast_info ON cast_info(person_id);
CREATE INDEX person_id_person_info ON person_info(person_id);
CREATE INDEX person_role_id_cast_info ON cast_info(person_role_id);
CREATE INDEX role_id_cast_info ON cast_info(role_id);
