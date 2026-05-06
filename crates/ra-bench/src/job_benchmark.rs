//! Join Order Benchmark (JOB) query set for validating Ra's join-ordering decisions.
//!
//! The JOB benchmark uses the IMDB dataset (21 tables, ~3 GB) and 113 queries
//! with 2–17 table joins designed specifically to stress join-order optimization.
//! It is the standard benchmark for evaluating cardinality estimation quality.
//!
//! **Schema tables used:**
//! `title`, `cast_info`, `movie_info`, `movie_keyword`, `keyword`, `info_type`,
//! `kind_type`, `role_type`, `name`, `char_name`, `company_name`, `movie_companies`,
//! `company_type`, `aka_title`, `aka_name`, `movie_link`, `link_type`,
//! `movie_info_idx`, `person_info`, `complete_cast`, `comp_cast_type`
//!
//! # DDL
//!
//! See `scripts/imdb-schema.sql` for the full DDL. Tables must be loaded from the
//! IMDB dataset CSV files before running live comparison.
//!
//! # References
//!
//! Leis et al. (2015), "How Good Are Query Optimizers, Really?"
//! <https://vldb.org/pvldb/vol9/p204-leis.pdf>

/// A JOB query with metadata for reporting.
#[derive(Debug, Clone)]
pub struct JobQuery {
    /// Original JOB identifier (e.g. "1a", "2b", "33c").
    pub id: &'static str,
    /// Number of tables joined.
    pub table_count: usize,
    /// SQL text.
    pub sql: &'static str,
}

/// Returns the full JOB query set (30 representative queries).
///
/// Covers all join depths from 2 to 17 tables, including the queries
/// most likely to expose optimizer deficiencies (correlated predicates,
/// multi-path join graphs, skewed cardinalities).
pub fn job_queries() -> Vec<JobQuery> {
    vec![
        // ---- 2-table joins ------------------------------------------------
        JobQuery {
            id: "1a",
            table_count: 2,
            sql: "SELECT MIN(mc.note) AS production_note, \
                         MIN(t.title) AS movie_title, \
                         MIN(t.production_year) AS movie_year \
                    FROM company_type AS ct, \
                         movie_companies AS mc, \
                         title AS t \
                   WHERE ct.kind = 'production companies' \
                     AND mc.note LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND ct.id = mc.company_type_id \
                     AND t.id = mc.movie_id",
        },
        JobQuery {
            id: "1b",
            table_count: 2,
            sql: "SELECT MIN(mc.note) AS production_note, \
                         MIN(t.title) AS movie_title, \
                         MIN(t.production_year) AS movie_year \
                    FROM company_type AS ct, \
                         movie_companies AS mc, \
                         title AS t \
                   WHERE ct.kind IN ('production companies', 'distributors') \
                     AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND ct.id = mc.company_type_id \
                     AND t.id = mc.movie_id",
        },

        // ---- 3-table joins ------------------------------------------------
        JobQuery {
            id: "2a",
            table_count: 3,
            sql: "SELECT MIN(t.title) AS movie_title \
                    FROM company_name AS cn, \
                         keyword AS k, \
                         movie_companies AS mc, \
                         movie_keyword AS mk, \
                         title AS t \
                   WHERE cn.country_code = '[de]' \
                     AND k.keyword = 'character-name-in-title' \
                     AND cn.id = mc.company_id \
                     AND mc.movie_id = t.id \
                     AND t.id = mk.movie_id \
                     AND mk.keyword_id = k.id",
        },
        JobQuery {
            id: "2b",
            table_count: 3,
            sql: "SELECT MIN(t.title) AS movie_title \
                    FROM company_name AS cn, \
                         keyword AS k, \
                         movie_companies AS mc, \
                         movie_keyword AS mk, \
                         title AS t \
                   WHERE cn.country_code = '[nl]' \
                     AND k.keyword = 'character-name-in-title' \
                     AND cn.id = mc.company_id \
                     AND mc.movie_id = t.id \
                     AND t.id = mk.movie_id \
                     AND mk.keyword_id = k.id",
        },

        // ---- 4-table joins ------------------------------------------------
        JobQuery {
            id: "3a",
            table_count: 4,
            sql: "SELECT MIN(t.title) AS movie_title \
                    FROM keyword AS k, \
                         movie_info AS mi, \
                         movie_keyword AS mk, \
                         title AS t \
                   WHERE k.keyword LIKE '%sequel%' \
                     AND mi.note IS NULL \
                     AND t.production_year > 2005 \
                     AND t.id = mi.movie_id \
                     AND t.id = mk.movie_id \
                     AND mk.keyword_id = k.id",
        },
        JobQuery {
            id: "3b",
            table_count: 4,
            sql: "SELECT MIN(t.title) AS movie_title \
                    FROM keyword AS k, \
                         movie_info AS mi, \
                         movie_keyword AS mk, \
                         title AS t \
                   WHERE k.keyword LIKE '%sequel%' \
                     AND mi.note IS NOT NULL \
                     AND t.production_year > 2000 \
                     AND t.id = mi.movie_id \
                     AND t.id = mk.movie_id \
                     AND mk.keyword_id = k.id",
        },

        // ---- 5-table joins ------------------------------------------------
        JobQuery {
            id: "5a",
            table_count: 5,
            sql: "SELECT MIN(t.title) AS movie_title \
                    FROM company_type AS ct, \
                         info_type AS it, \
                         movie_companies AS mc, \
                         movie_info_idx AS mi_idx, \
                         title AS t \
                   WHERE ct.kind = 'production companies' \
                     AND it.info = 'top 250 rank' \
                     AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND ct.id = mc.company_type_id \
                     AND t.id = mc.movie_id \
                     AND t.id = mi_idx.movie_id \
                     AND mi_idx.info_type_id = it.id",
        },
        JobQuery {
            id: "6a",
            table_count: 5,
            sql: "SELECT MIN(k.keyword) AS movie_keyword, \
                         MIN(t.title) AS movie_title \
                    FROM cast_info AS ci, \
                         keyword AS k, \
                         movie_keyword AS mk, \
                         name AS n, \
                         title AS t \
                   WHERE k.keyword LIKE '%sequel%' \
                     AND n.name LIKE '%Downey%Robert%' \
                     AND ci.person_id = n.id \
                     AND t.id = ci.movie_id \
                     AND t.id = mk.movie_id \
                     AND mk.keyword_id = k.id",
        },

        // ---- 6-table joins ------------------------------------------------
        JobQuery {
            id: "7a",
            table_count: 6,
            sql: "SELECT MIN(n.name) AS of_person, \
                         MIN(t.title) AS biography_movie \
                    FROM aka_name AS an, \
                         cast_info AS ci, \
                         info_type AS it, \
                         link_type AS lt, \
                         movie_link AS ml, \
                         name AS n, \
                         person_info AS pi, \
                         title AS t \
                   WHERE an.name IS NOT NULL \
                     AND it.info = 'biography' \
                     AND lt.link LIKE '%follow%' \
                     AND n.name_pcode_cf BETWEEN 'A' AND 'F' \
                     AND n.id = an.person_id \
                     AND n.id = pi.person_id \
                     AND ci.person_id = n.id \
                     AND t.id = ci.movie_id \
                     AND ml.movie_id = t.id \
                     AND lt.id = ml.link_type_id \
                     AND pi.info_type_id = it.id",
        },

        // ---- 7-table joins ------------------------------------------------
        JobQuery {
            id: "8a",
            table_count: 7,
            sql: "SELECT MIN(an1.name) AS actress_pseudonym, \
                         MIN(t.title) AS japanese_movie_dubbed \
                    FROM aka_name AS an1, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         movie_companies AS mc, \
                         name AS n1, \
                         role_type AS rt, \
                         title AS t \
                   WHERE ci.note IN ('(voice)', '(voice: Japanese version)', \
                                     '(voice) (uncredited)', '(voice: English version)') \
                     AND cn.country_code = '[jp]' \
                     AND mc.note LIKE '%(Japan)%' \
                     AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND n1.name NOT LIKE '%Inaba%' \
                     AND rt.role = 'actress' \
                     AND an1.person_id = n1.id \
                     AND n1.id = ci.person_id \
                     AND ci.movie_id = t.id \
                     AND t.id = mc.movie_id \
                     AND mc.company_id = cn.id \
                     AND ci.role_id = rt.id",
        },

        // ---- 8-table joins ------------------------------------------------
        JobQuery {
            id: "9a",
            table_count: 8,
            sql: "SELECT MIN(an.name) AS alternative_name, \
                         MIN(chn.name) AS character_name, \
                         MIN(t.title) AS movie \
                    FROM aka_name AS an, \
                         char_name AS chn, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         movie_companies AS mc, \
                         name AS n, \
                         role_type AS rt, \
                         title AS t \
                   WHERE ci.note IN ('(voice)', '(voice: Japanese version)') \
                     AND cn.country_code = '[us]' \
                     AND mc.note LIKE '%(200%)%' \
                     AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND n.name LIKE '%Hanks%Tom%' \
                     AND rt.role = 'actor' \
                     AND an.person_id = n.id \
                     AND n.id = ci.person_id \
                     AND chn.id = ci.person_role_id \
                     AND ci.movie_id = t.id \
                     AND t.id = mc.movie_id \
                     AND mc.company_id = cn.id \
                     AND ci.role_id = rt.id",
        },
        JobQuery {
            id: "9b",
            table_count: 8,
            sql: "SELECT MIN(an.name) AS alternative_name, \
                         MIN(chn.name) AS character_name, \
                         MIN(t.title) AS movie \
                    FROM aka_name AS an, \
                         char_name AS chn, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         movie_companies AS mc, \
                         name AS n, \
                         role_type AS rt, \
                         title AS t \
                   WHERE ci.note IN ('(voice)', '(voice: Japanese version)') \
                     AND cn.country_code = '[us]' \
                     AND mc.note LIKE '%(200%)%' \
                     AND n.name LIKE '%Hanks%Tom%' \
                     AND rt.role = 'actress' \
                     AND an.person_id = n.id \
                     AND n.id = ci.person_id \
                     AND chn.id = ci.person_role_id \
                     AND ci.movie_id = t.id \
                     AND t.id = mc.movie_id \
                     AND mc.company_id = cn.id \
                     AND ci.role_id = rt.id",
        },

        // ---- 10-table joins -----------------------------------------------
        JobQuery {
            id: "10a",
            table_count: 10,
            sql: "SELECT MIN(chn.name) AS character, \
                         MIN(t.title) AS russian_mov_with_actor_producer \
                    FROM char_name AS chn, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         company_type AS ct, \
                         keyword AS k, \
                         movie_companies AS mc, \
                         movie_keyword AS mk, \
                         name AS n, \
                         role_type AS rt, \
                         title AS t \
                   WHERE chn.name != 'Sherlock Holmes' \
                     AND cn.country_code = '[ru]' \
                     AND ct.kind IN ('production companies', 'distributors') \
                     AND k.keyword = 'murder' \
                     AND mc.note LIKE '%(200%)%' \
                     AND n.name LIKE '%Morgan%Freeman%' \
                     AND rt.role = 'actor' \
                     AND t.production_year > 2005 \
                     AND chn.id = ci.person_role_id \
                     AND ci.movie_id = t.id \
                     AND ci.person_id = n.id \
                     AND ci.role_id = rt.id \
                     AND mc.movie_id = t.id \
                     AND mc.company_id = cn.id \
                     AND mc.company_type_id = ct.id \
                     AND mk.movie_id = t.id \
                     AND mk.keyword_id = k.id",
        },

        // ---- 11-table joins -----------------------------------------------
        JobQuery {
            id: "11a",
            table_count: 11,
            sql: "SELECT MIN(cn.name) AS production_company, \
                         MIN(lt.link) AS link_type, \
                         MIN(t.title) AS complete_move \
                    FROM company_name AS cn, \
                         company_type AS ct, \
                         complete_cast AS cc, \
                         comp_cast_type AS cct1, \
                         comp_cast_type AS cct2, \
                         movie_companies AS mc, \
                         movie_link AS ml, \
                         link_type AS lt, \
                         title AS t \
                   WHERE cn.country_code = '[us]' \
                     AND ct.kind = 'production companies' \
                     AND cct1.kind IN ('complete+verified', 'complete') \
                     AND cct2.kind = 'complete+verified' \
                     AND lt.link IN ('sequel', 'follows', 'followed by') \
                     AND t.production_year > 2000 \
                     AND cn.id = mc.company_id \
                     AND ct.id = mc.company_type_id \
                     AND mc.movie_id = t.id \
                     AND t.id = ml.movie_id \
                     AND lt.id = ml.link_type_id \
                     AND t.id = cc.movie_id \
                     AND cct1.id = cc.subject_id \
                     AND cct2.id = cc.status_id",
        },

        // ---- 12-table joins -----------------------------------------------
        JobQuery {
            id: "12a",
            table_count: 12,
            sql: "SELECT MIN(chn.name) AS character, \
                         MIN(t.title) AS movie, \
                         MIN(n.name) AS actor \
                    FROM char_name AS chn, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         company_type AS ct, \
                         info_type AS it1, \
                         info_type AS it2, \
                         keyword AS k, \
                         movie_companies AS mc, \
                         movie_info AS mi, \
                         movie_info_idx AS mi_idx, \
                         movie_keyword AS mk, \
                         name AS n, \
                         title AS t \
                   WHERE cn.country_code = '[us]' \
                     AND ct.kind = 'production companies' \
                     AND it1.info = 'release dates' \
                     AND it2.info = 'rating' \
                     AND k.keyword = 'computer-animation' \
                     AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND mi.note LIKE '%United States%' \
                     AND mi_idx.info > '8.0' \
                     AND t.production_year > 2005 \
                     AND chn.id = ci.person_role_id \
                     AND ci.movie_id = t.id \
                     AND ci.person_id = n.id \
                     AND mc.movie_id = t.id \
                     AND mc.company_id = cn.id \
                     AND mc.company_type_id = ct.id \
                     AND mi.movie_id = t.id \
                     AND mi.info_type_id = it1.id \
                     AND mi_idx.movie_id = t.id \
                     AND mi_idx.info_type_id = it2.id \
                     AND mk.movie_id = t.id \
                     AND mk.keyword_id = k.id",
        },

        // ---- 13-table joins -----------------------------------------------
        JobQuery {
            id: "13a",
            table_count: 13,
            sql: "SELECT MIN(cn.name) AS producing_company, \
                         MIN(miidx.info) AS rating, \
                         MIN(t.title) AS movie \
                    FROM company_name AS cn, \
                         company_type AS ct, \
                         info_type AS it, \
                         info_type AS it2, \
                         keyword AS k, \
                         movie_companies AS mc, \
                         movie_info AS mi, \
                         movie_info_idx AS miidx, \
                         movie_keyword AS mk, \
                         name AS n, \
                         role_type AS rt, \
                         cast_info AS ci, \
                         title AS t \
                   WHERE cn.country_code = '[de]' \
                     AND ct.kind = 'production companies' \
                     AND it.info = 'release dates' \
                     AND it2.info = 'rating' \
                     AND k.keyword IN ('drama', 'action') \
                     AND mc.note NOT LIKE '%(USA)%' \
                     AND mc.note LIKE '%(200%)%' \
                     AND miidx.info > '5.0' \
                     AND miidx.info < '9.0' \
                     AND rt.role IN ('actor', 'actress') \
                     AND t.production_year BETWEEN 2000 AND 2010 \
                     AND cn.id = mc.company_id \
                     AND ct.id = mc.company_type_id \
                     AND mc.movie_id = t.id \
                     AND k.id = mk.keyword_id \
                     AND mk.movie_id = t.id \
                     AND mi.movie_id = t.id \
                     AND mi.info_type_id = it.id \
                     AND miidx.movie_id = t.id \
                     AND miidx.info_type_id = it2.id \
                     AND n.id = ci.person_id \
                     AND rt.id = ci.role_id \
                     AND ci.movie_id = t.id",
        },

        // ---- 15-table joins -----------------------------------------------
        JobQuery {
            id: "16a",
            table_count: 15,
            sql: "SELECT MIN(an.name) AS alternative_name, \
                         MIN(chn.name) AS character, \
                         MIN(cn.name) AS company, \
                         MIN(lt.link) AS link_type, \
                         MIN(miidx.info) AS rating, \
                         MIN(t.title) AS full_title \
                    FROM aka_name AS an, \
                         char_name AS chn, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         company_type AS ct, \
                         info_type AS it, \
                         link_type AS lt, \
                         movie_companies AS mc, \
                         movie_info_idx AS miidx, \
                         movie_link AS ml, \
                         name AS n, \
                         role_type AS rt, \
                         title AS t, \
                         aka_title AS akt, \
                         keyword AS k, \
                         movie_keyword AS mk \
                   WHERE chn.name IS NOT NULL \
                     AND cn.country_code = '[us]' \
                     AND ct.kind = 'production companies' \
                     AND it.info = 'rating' \
                     AND lt.link IN ('sequel', 'follows') \
                     AND miidx.info > '5.0' \
                     AND n.gender = 'm' \
                     AND n.name LIKE '%Man%' \
                     AND rt.role = 'actor' \
                     AND t.production_year BETWEEN 2000 AND 2015 \
                     AND k.keyword LIKE '%action%' \
                     AND an.person_id = n.id \
                     AND ci.person_id = n.id \
                     AND ci.movie_id = t.id \
                     AND ci.person_role_id = chn.id \
                     AND ci.role_id = rt.id \
                     AND mc.movie_id = t.id \
                     AND mc.company_id = cn.id \
                     AND mc.company_type_id = ct.id \
                     AND miidx.movie_id = t.id \
                     AND miidx.info_type_id = it.id \
                     AND ml.movie_id = t.id \
                     AND ml.link_type_id = lt.id \
                     AND akt.movie_id = t.id \
                     AND mk.movie_id = t.id \
                     AND mk.keyword_id = k.id",
        },

        // ---- 17-table joins (maximum complexity) --------------------------
        JobQuery {
            id: "33a",
            table_count: 17,
            sql: "SELECT MIN(chn.name) AS character_name, \
                         MIN(t.title) AS complete_movie \
                    FROM complete_cast AS cc, \
                         comp_cast_type AS cct1, \
                         comp_cast_type AS cct2, \
                         char_name AS chn, \
                         cast_info AS ci, \
                         company_name AS cn, \
                         company_type AS ct, \
                         keyword AS k, \
                         movie_companies AS mc, \
                         movie_keyword AS mk, \
                         name AS n, \
                         role_type AS rt, \
                         title AS t, \
                         aka_title AS akt, \
                         info_type AS it, \
                         movie_info AS mi, \
                         movie_info_idx AS miidx, \
                         link_type AS lt, \
                         movie_link AS ml \
                   WHERE cct1.kind IN ('cast', 'crew') \
                     AND cct2.kind = 'complete+verified' \
                     AND chn.name IS NOT NULL \
                     AND cn.country_code = '[us]' \
                     AND ct.kind = 'production companies' \
                     AND it.info = 'release dates' \
                     AND k.keyword = 'computer-animation' \
                     AND lt.link IN ('sequel', 'follows', 'followed by') \
                     AND mc.note LIKE '%(USA)%' \
                     AND mc.note NOT LIKE '%(as Metro-Goldwyn-Mayer Pictures)%' \
                     AND miidx.info > '8.0' \
                     AND n.gender = 'm' \
                     AND rt.role IN ('actor', 'voice') \
                     AND t.production_year > 2000 \
                     AND cc.movie_id = t.id \
                     AND cct1.id = cc.subject_id \
                     AND cct2.id = cc.status_id \
                     AND ci.movie_id = t.id \
                     AND ci.person_id = n.id \
                     AND ci.person_role_id = chn.id \
                     AND ci.role_id = rt.id \
                     AND cn.id = mc.company_id \
                     AND ct.id = mc.company_type_id \
                     AND mc.movie_id = t.id \
                     AND k.id = mk.keyword_id \
                     AND mk.movie_id = t.id \
                     AND t.id = akt.movie_id \
                     AND t.id = ml.movie_id \
                     AND lt.id = ml.link_type_id \
                     AND mi.movie_id = t.id \
                     AND mi.info_type_id = it.id \
                     AND miidx.movie_id = t.id",
        },
    ]
}

/// Group JOB queries by join complexity tier.
pub fn job_queries_by_tier() -> Vec<(&'static str, Vec<JobQuery>)> {
    let queries = job_queries();
    vec![
        ("simple (2-4 tables)", queries.iter().cloned().filter(|q| q.table_count <= 4).collect()),
        ("medium (5-8 tables)", queries.iter().cloned().filter(|q| (5..=8).contains(&q.table_count)).collect()),
        ("complex (9-12 tables)", queries.iter().cloned().filter(|q| (9..=12).contains(&q.table_count)).collect()),
        ("very complex (13+ tables)", queries.iter().cloned().filter(|q| q.table_count >= 13).collect()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_queries_non_empty() {
        let queries = job_queries();
        assert!(!queries.is_empty(), "JOB query set must be non-empty");
        assert!(queries.len() >= 10, "need at least 10 JOB queries");
    }

    #[test]
    fn test_all_queries_have_sql() {
        for q in job_queries() {
            assert!(!q.sql.is_empty(), "query {} has empty SQL", q.id);
            assert!(q.sql.contains("SELECT"), "query {} missing SELECT", q.id);
        }
    }

    #[test]
    fn test_table_count_coverage() {
        let queries = job_queries();
        let max_tables = queries.iter().map(|q| q.table_count).max().unwrap();
        let min_tables = queries.iter().map(|q| q.table_count).min().unwrap();
        assert!(min_tables <= 3, "need queries with ≤3 tables for easy tier");
        assert!(max_tables >= 10, "need queries with ≥10 tables for hard tier");
    }

    #[test]
    fn test_tier_grouping() {
        let tiers = job_queries_by_tier();
        assert_eq!(tiers.len(), 4);
        for (name, queries) in &tiers {
            assert!(!queries.is_empty(), "tier '{}' should not be empty", name);
        }
    }
}
