-- Minimal TPC-H schema for the Ra differential result oracle (RA-STEERING §5, Gate 1).
-- Standard TPC-H tables; a handful of referentially-consistent rows so that the
-- corpus queries (and Ra's re-emitted optimized SQL) both execute and return
-- non-trivial results to compare. Not TPC-H scale data — just enough to expose
-- optimizer wrong-answer defects via row-multiset divergence.

DROP TABLE IF EXISTS lineitem, orders, customer, part, supplier, partsupp, nation, region CASCADE;

CREATE TABLE region (
    r_regionkey  INTEGER PRIMARY KEY,
    r_name       CHAR(25) NOT NULL,
    r_comment    VARCHAR(152)
);

CREATE TABLE nation (
    n_nationkey  INTEGER PRIMARY KEY,
    n_name       CHAR(25) NOT NULL,
    n_regionkey  INTEGER NOT NULL,
    n_comment    VARCHAR(152)
);

CREATE TABLE customer (
    c_custkey    INTEGER PRIMARY KEY,
    c_name       VARCHAR(25) NOT NULL,
    c_address    VARCHAR(40),
    c_nationkey  INTEGER NOT NULL,
    c_phone      CHAR(15),
    c_acctbal    NUMERIC(15,2),
    c_mktsegment CHAR(10),
    c_comment    VARCHAR(117)
);

CREATE TABLE supplier (
    s_suppkey    INTEGER PRIMARY KEY,
    s_name       CHAR(25) NOT NULL,
    s_address    VARCHAR(40),
    s_nationkey  INTEGER NOT NULL,
    s_phone      CHAR(15),
    s_acctbal    NUMERIC(15,2),
    s_comment    VARCHAR(101)
);

CREATE TABLE part (
    p_partkey     INTEGER PRIMARY KEY,
    p_name        VARCHAR(55) NOT NULL,
    p_mfgr        CHAR(25),
    p_brand       CHAR(10),
    p_type        VARCHAR(25),
    p_size        INTEGER,
    p_container   CHAR(10),
    p_retailprice NUMERIC(15,2),
    p_comment     VARCHAR(23)
);

CREATE TABLE partsupp (
    ps_partkey    INTEGER NOT NULL,
    ps_suppkey    INTEGER NOT NULL,
    ps_availqty   INTEGER,
    ps_supplycost NUMERIC(15,2),
    ps_comment    VARCHAR(199),
    PRIMARY KEY (ps_partkey, ps_suppkey)
);

CREATE TABLE orders (
    o_orderkey      INTEGER PRIMARY KEY,
    o_custkey       INTEGER NOT NULL,
    o_orderstatus   CHAR(1),
    o_totalprice    NUMERIC(15,2),
    o_orderdate     DATE,
    o_orderpriority CHAR(15),
    o_clerk         CHAR(15),
    o_shippriority  INTEGER,
    o_comment       VARCHAR(79)
);

CREATE TABLE lineitem (
    l_orderkey      INTEGER NOT NULL,
    l_partkey       INTEGER NOT NULL,
    l_suppkey       INTEGER NOT NULL,
    l_linenumber    INTEGER NOT NULL,
    l_quantity      NUMERIC(15,2),
    l_extendedprice NUMERIC(15,2),
    l_discount      NUMERIC(15,2),
    l_tax           NUMERIC(15,2),
    l_returnflag    CHAR(1),
    l_linestatus    CHAR(1),
    l_shipdate      DATE,
    l_commitdate    DATE,
    l_receiptdate   DATE,
    l_shipinstruct  CHAR(25),
    l_shipmode      CHAR(10),
    l_comment       VARCHAR(44),
    PRIMARY KEY (l_orderkey, l_linenumber)
);

-- ---- seed data (small, referentially consistent) ----
INSERT INTO region VALUES
 (0,'AFRICA',''),(1,'AMERICA',''),(2,'ASIA',''),(3,'EUROPE',''),(4,'MIDDLE EAST','');

INSERT INTO nation VALUES
 (0,'ALGERIA',0,''),(1,'ARGENTINA',1,''),(2,'BRAZIL',1,''),(3,'CANADA',1,''),
 (4,'EGYPT',4,''),(5,'ETHIOPIA',0,''),(6,'FRANCE',3,''),(7,'GERMANY',3,''),
 (8,'INDIA',2,''),(9,'INDONESIA',2,'');

INSERT INTO customer VALUES
 (1,'Customer#1','addr1',5,'25-1',711.56,'BUILDING','c1'),
 (2,'Customer#2','addr2',6,'25-2',121.65,'AUTOMOBILE','c2'),
 (3,'Customer#3','addr3',5,'25-3',7498.12,'AUTOMOBILE','c3'),
 (4,'Customer#4','addr4',1,'25-4',2866.83,'MACHINERY','c4'),
 (5,'Customer#5','addr5',3,'25-5',794.47,'HOUSEHOLD','c5'),
 (6,'Customer#6','addr6',5,'25-6',7638.57,'AUTOMOBILE','c6'),
 (7,'Customer#7','addr7',8,'25-7',9561.95,'AUTOMOBILE','c7');

INSERT INTO supplier VALUES
 (1,'Supplier#1','saddr1',5,'27-1',5755.94,'s1'),
 (2,'Supplier#2','saddr2',6,'27-2',4032.68,'s2'),
 (3,'Supplier#3','saddr3',1,'27-3',4192.40,'s3'),
 (4,'Supplier#4','saddr4',3,'27-4',4641.08,'s4');

INSERT INTO part VALUES
 (1,'part one','Mfgr#1','Brand#13','SMALL PLATED COPPER',7,'JUMBO PKG',901.00,'p1'),
 (2,'part two','Mfgr#1','Brand#13','LARGE BRUSHED BRASS',1,'LG CASE',902.00,'p2'),
 (3,'part three','Mfgr#4','Brand#42','STANDARD POLISHED TIN',21,'WRAP CASE',903.00,'p3'),
 (4,'part four','Mfgr#3','Brand#34','SMALL PLATED STEEL',14,'MED DRUM',904.00,'p4');

INSERT INTO partsupp VALUES
 (1,1,3325,771.64,'ps1'),(1,2,8895,378.49,'ps2'),
 (2,3,4069,920.92,'ps3'),(3,4,3956,4.27,'ps4'),
 (4,1,4069,920.92,'ps5');

INSERT INTO orders VALUES
 (1,1,'O',173665.47,'1996-01-02','5-LOW','Clerk#1',0,'o1'),
 (2,3,'O',46929.18,'1996-12-01','1-URGENT','Clerk#2',0,'o2'),
 (3,3,'F',193846.25,'1993-10-14','5-LOW','Clerk#3',0,'o3'),
 (4,5,'O',32151.78,'1995-10-11','5-LOW','Clerk#4',0,'o4'),
 (5,1,'F',144659.20,'1994-07-30','5-LOW','Clerk#5',0,'o5'),
 (6,6,'F',58749.59,'1998-02-21','4-NOT SPECIFIED','Clerk#6',0,'o6'),
 (7,7,'O',252004.18,'1998-01-10','2-HIGH','Clerk#7',0,'o7');

INSERT INTO lineitem VALUES
 (1,1,1,1,17,24710.35,0.04,0.02,'N','O','1996-03-13','1996-02-12','1996-03-22','x','TRUCK','l1'),
 (1,2,2,2,36,45983.16,0.09,0.06,'N','O','1996-04-12','1996-02-28','1996-04-20','x','MAIL','l2'),
 (2,3,3,1,38,44694.46,0.00,0.05,'N','O','1997-01-28','1997-01-14','1997-02-02','x','RAIL','l3'),
 (3,1,1,1,45,54058.05,0.06,0.00,'R','F','1994-02-02','1994-01-04','1994-02-23','x','AIR','l4'),
 (3,2,2,2,49,46796.47,0.10,0.00,'R','F','1993-11-09','1993-12-20','1993-11-24','x','RAIL','l5'),
 (4,4,1,1,30,30690.90,0.03,0.08,'N','O','1996-01-10','1995-12-14','1996-01-18','x','REG AIR','l6'),
 (5,1,1,1,15,18150.15,0.02,0.04,'R','F','1994-10-31','1994-08-31','1994-11-20','x','FOB','l7'),
 (6,3,3,1,37,44694.46,0.08,0.03,'N','F','1998-03-13','1998-02-12','1998-03-22','x','SHIP','l8'),
 (7,4,1,1,12,12000.00,0.07,0.03,'N','O','1998-02-10','1998-01-14','1998-02-20','x','TRUCK','l9');

ANALYZE;
