-- Join Order Benchmark (JOB) IMDB Schema
-- 21 tables from the Internet Movie Database (May 2013 snapshot)

-- Drop existing tables
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

-- Table 1: aka_name (actor aliases)
CREATE TABLE aka_name (
    id INTEGER PRIMARY KEY,
    person_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    imdb_index VARCHAR(3),
    name_pcode_cf VARCHAR(11),
    name_pcode_nf VARCHAR(11),
    surname_pcode VARCHAR(11),
    md5sum VARCHAR(65)
);

-- Table 2: aka_title (movie aliases)
CREATE TABLE aka_title (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    imdb_index VARCHAR(4),
    kind_id INTEGER NOT NULL,
    production_year INTEGER,
    phonetic_code VARCHAR(5),
    episode_of_id INTEGER,
    season_nr INTEGER,
    episode_nr INTEGER,
    note TEXT,
    md5sum VARCHAR(65)
);

-- Table 3: cast_info (actors in movies)
CREATE TABLE cast_info (
    id INTEGER PRIMARY KEY,
    person_id INTEGER NOT NULL,
    movie_id INTEGER NOT NULL,
    person_role_id INTEGER,
    note TEXT,
    nr_order INTEGER,
    role_id INTEGER NOT NULL
);

-- Table 4: char_name (character names)
CREATE TABLE char_name (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    imdb_index VARCHAR(2),
    imdb_id INTEGER,
    name_pcode_nf VARCHAR(5),
    surname_pcode VARCHAR(5),
    md5sum VARCHAR(65)
);

-- Table 5: comp_cast_type (complete cast types)
CREATE TABLE comp_cast_type (
    id INTEGER PRIMARY KEY,
    kind VARCHAR(32) NOT NULL
);

-- Table 6: company_name (production companies)
CREATE TABLE company_name (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    country_code VARCHAR(6),
    imdb_id INTEGER,
    name_pcode_nf VARCHAR(5),
    name_pcode_sf VARCHAR(5),
    md5sum VARCHAR(65)
);

-- Table 7: company_type (types of companies)
CREATE TABLE company_type (
    id INTEGER PRIMARY KEY,
    kind VARCHAR(32) NOT NULL
);

-- Table 8: complete_cast (complete cast info)
CREATE TABLE complete_cast (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER,
    subject_id INTEGER NOT NULL,
    status_id INTEGER NOT NULL
);

-- Table 9: info_type (types of movie/person information)
CREATE TABLE info_type (
    id INTEGER PRIMARY KEY,
    info VARCHAR(32) NOT NULL
);

-- Table 10: keyword (movie keywords)
CREATE TABLE keyword (
    id INTEGER PRIMARY KEY,
    keyword TEXT NOT NULL,
    phonetic_code VARCHAR(5)
);

-- Table 11: kind_type (types of titles: movie, TV series, etc.)
CREATE TABLE kind_type (
    id INTEGER PRIMARY KEY,
    kind VARCHAR(15) NOT NULL
);

-- Table 12: link_type (types of movie links: sequel, remake, etc.)
CREATE TABLE link_type (
    id INTEGER PRIMARY KEY,
    link VARCHAR(32) NOT NULL
);

-- Table 13: movie_companies (companies involved in movies)
CREATE TABLE movie_companies (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL,
    company_id INTEGER NOT NULL,
    company_type_id INTEGER NOT NULL,
    note TEXT
);

-- Table 14: movie_info (general movie information)
CREATE TABLE movie_info (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL,
    info_type_id INTEGER NOT NULL,
    info TEXT NOT NULL,
    note TEXT
);

-- Table 15: movie_info_idx (indexed movie information like ratings)
CREATE TABLE movie_info_idx (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL,
    info_type_id INTEGER NOT NULL,
    info TEXT NOT NULL,
    note TEXT
);

-- Table 16: movie_keyword (keywords associated with movies)
CREATE TABLE movie_keyword (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL,
    keyword_id INTEGER NOT NULL
);

-- Table 17: movie_link (links between movies)
CREATE TABLE movie_link (
    id INTEGER PRIMARY KEY,
    movie_id INTEGER NOT NULL,
    linked_movie_id INTEGER NOT NULL,
    link_type_id INTEGER NOT NULL
);

-- Table 18: name (people: actors, directors, etc.)
CREATE TABLE name (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    imdb_index VARCHAR(9),
    imdb_id INTEGER,
    gender VARCHAR(1),
    name_pcode_cf VARCHAR(5),
    name_pcode_nf VARCHAR(5),
    surname_pcode VARCHAR(5),
    md5sum VARCHAR(65)
);

-- Table 19: person_info (information about people)
CREATE TABLE person_info (
    id INTEGER PRIMARY KEY,
    person_id INTEGER NOT NULL,
    info_type_id INTEGER NOT NULL,
    info TEXT NOT NULL,
    note TEXT
);

-- Table 20: role_type (types of roles: actor, director, etc.)
CREATE TABLE role_type (
    id INTEGER PRIMARY KEY,
    role VARCHAR(32) NOT NULL
);

-- Table 21: title (movies, TV shows, episodes)
CREATE TABLE title (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    imdb_index VARCHAR(5),
    kind_id INTEGER NOT NULL,
    production_year INTEGER,
    imdb_id INTEGER,
    phonetic_code VARCHAR(5),
    episode_of_id INTEGER,
    season_nr INTEGER,
    episode_nr INTEGER,
    series_years VARCHAR(49),
    md5sum VARCHAR(65)
);

-- Indexes for join columns (critical for JOB performance)
CREATE INDEX idx_aka_name_person ON aka_name(person_id);
CREATE INDEX idx_aka_title_movie ON aka_title(movie_id);
CREATE INDEX idx_aka_title_kind ON aka_title(kind_id);
CREATE INDEX idx_cast_info_person ON cast_info(person_id);
CREATE INDEX idx_cast_info_movie ON cast_info(movie_id);
CREATE INDEX idx_cast_info_role ON cast_info(role_id);
CREATE INDEX idx_cast_info_person_role ON cast_info(person_role_id);
CREATE INDEX idx_complete_cast_movie ON complete_cast(movie_id);
CREATE INDEX idx_complete_cast_subject ON complete_cast(subject_id);
CREATE INDEX idx_complete_cast_status ON complete_cast(status_id);
CREATE INDEX idx_movie_companies_movie ON movie_companies(movie_id);
CREATE INDEX idx_movie_companies_company ON movie_companies(company_id);
CREATE INDEX idx_movie_companies_type ON movie_companies(company_type_id);
CREATE INDEX idx_movie_info_movie ON movie_info(movie_id);
CREATE INDEX idx_movie_info_type ON movie_info(info_type_id);
CREATE INDEX idx_movie_info_idx_movie ON movie_info_idx(movie_id);
CREATE INDEX idx_movie_info_idx_type ON movie_info_idx(info_type_id);
CREATE INDEX idx_movie_keyword_movie ON movie_keyword(movie_id);
CREATE INDEX idx_movie_keyword_keyword ON movie_keyword(keyword_id);
CREATE INDEX idx_movie_link_movie ON movie_link(movie_id);
CREATE INDEX idx_movie_link_linked ON movie_link(linked_movie_id);
CREATE INDEX idx_movie_link_type ON movie_link(link_type_id);
CREATE INDEX idx_person_info_person ON person_info(person_id);
CREATE INDEX idx_person_info_type ON person_info(info_type_id);
CREATE INDEX idx_title_kind ON title(kind_id);
CREATE INDEX idx_title_episode ON title(episode_of_id);

-- Additional indexes for common filters
CREATE INDEX idx_company_name_country ON company_name(country_code);
CREATE INDEX idx_keyword_keyword ON keyword(keyword);
CREATE INDEX idx_kind_type_kind ON kind_type(kind);
CREATE INDEX idx_title_production_year ON title(production_year);
CREATE INDEX idx_name_gender ON name(gender);
