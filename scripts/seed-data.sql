-- Minimal TPC-H seed data for Ra benchmark comparison.
-- Usage: psql -U postgres -d tpch -f scripts/seed-data.sql

-- ============================================================
-- Region (5 rows - standard TPC-H)
-- ============================================================
INSERT INTO region (r_regionkey, r_name, r_comment) VALUES
(0, 'AFRICA', 'lar deposits. blithely final packages cajole. regular waters are final requests. regular accounts are according to'),
(1, 'AMERICA', 'hs use ironic, even requests. s'),
(2, 'ASIA', 'ges. thinly even pinto beans ca'),
(3, 'EUROPE', 'ly final courts cajole furiously final excuse'),
(4, 'MIDDLE EAST', 'uickly special accounts cajole carefully blithely close requests. carefully final asymptotes haggle furiousl');

-- ============================================================
-- Nation (25 rows - standard TPC-H)
-- ============================================================
INSERT INTO nation (n_nationkey, n_name, n_regionkey, n_comment) VALUES
(0, 'ALGERIA', 0, ' haggle. carefully final deposits detect slyly agai'),
(1, 'ARGENTINA', 1, 'al foxes promise slyly according to the regular accounts. bold requests alon'),
(2, 'BRAZIL', 1, 'y alongside of the pending deposits. carefully special packages are about the ironic forges. slyly special'),
(3, 'CANADA', 1, 'eas hang ironic, silent packages. slyly regular packages are furiously over the tithes. fluffily bold'),
(4, 'EGYPT', 4, 'y above the carefully unusual theodolites. final dugouts are quickly across the furiously regular d'),
(5, 'ETHIOPIA', 0, 'ven packages wake quickly. regu'),
(6, 'FRANCE', 3, 'refully final requests. regular, ironi'),
(7, 'GERMANY', 3, 'l platelets. regular accounts x-ray: unusual, regular acco'),
(8, 'INDIA', 2, 'ss excuses cajole slyly across the packages. deposits print aroun'),
(9, 'INDONESIA', 2, ' slyly express asymptotes. regular deposits haggle slyly. carefully ironic hockey players sleep blithely. carefull'),
(10, 'IRAN', 4, 'efully alongside of the slyly final dependencies.'),
(11, 'IRAQ', 4, 'nic deposits boost atop the quickly final requests? quickly regula'),
(12, 'JAPAN', 2, 'ously. final, express gifts cajole a'),
(13, 'JORDAN', 4, 'ic deposits are blithely about the carefully regular pa'),
(14, 'KENYA', 0, ' pending excuses haggle furiously deposits. pending, express pinto beans wake fluffily past t'),
(15, 'MOROCCO', 0, 'rns. blithely bold courts among the closely regular packages use furiously bold platelets?'),
(16, 'MOZAMBIQUE', 0, 's. ironic, unusual asymptotes wake blithely r'),
(17, 'PERU', 1, 'platelets. blithely pending dependencies use fluffily across the even pinto beans. carefully silent accoun'),
(18, 'CHINA', 2, 'c dependencies. furiously express notornis sleep slyly regular accounts. ideas sleep. depos'),
(19, 'ROMANIA', 3, 'ular asymptotes are about the furious multipliers. express dependencies nag above the ironically ironic account'),
(20, 'SAUDI ARABIA', 4, 'ts. silent requests haggle. closely express packages sleep across the blithely'),
(21, 'VIETNAM', 2, 'hely enticingly express accounts. even, final'),
(22, 'RUSSIA', 3, ' requests against the platelets use never according to the quickly regular pint'),
(23, 'UNITED KINGDOM', 3, 'eans boost carefully special requests. accounts are. carefull'),
(24, 'UNITED STATES', 1, 'y final packages. slow foxes cajole quickly. quickly silent platelets breach ironic accounts. unusual pinto be');

-- ============================================================
-- Part (~30 rows)
-- ============================================================
INSERT INTO part (p_partkey, p_name, p_mfgr, p_brand, p_type, p_size, p_container, p_retailprice, p_comment) VALUES
(1, 'goldenrod lavender spring', 'Manufacturer#1', 'Brand#13', 'PROMO BURNISHED COPPER', 7, 'JUMBO PKG', 901.00, 'ly. slyly ironi'),
(2, 'blush thistle blue', 'Manufacturer#1', 'Brand#13', 'LARGE BRUSHED BRASS', 1, 'LG CASE', 902.00, 'lar accounts amo'),
(3, 'spring green yellow', 'Manufacturer#4', 'Brand#42', 'STANDARD POLISHED BRASS', 21, 'WRAP CASE', 903.00, 'egular deposits hag'),
(4, 'cornflower chocolate', 'Manufacturer#3', 'Brand#34', 'SMALL PLATED BRASS', 14, 'MED DRUM', 904.00, 'p furiously r'),
(5, 'forest brown coral', 'Manufacturer#3', 'Brand#32', 'STANDARD POLISHED TIN', 15, 'SM PKG', 905.00, 'wake carefully'),
(6, 'dark tan gainsboro', 'Manufacturer#2', 'Brand#24', 'PROMO PLATED STEEL', 4, 'MED BAG', 906.00, 'jole. excuses'),
(7, 'purple blue khaki', 'Manufacturer#1', 'Brand#11', 'SMALL PLATED COPPER', 45, 'SM BAG', 907.00, 'lyly. express'),
(8, 'hot papaya firebrick', 'Manufacturer#4', 'Brand#44', 'PROMO BURNISHED TIN', 41, 'LG DRUM', 908.00, 'among the furi'),
(9, 'ivory dim tan', 'Manufacturer#4', 'Brand#43', 'SMALL BURNISHED STEEL', 12, 'SM CASE', 909.00, 'ly ironic foxes a'),
(10, 'tan cream slate', 'Manufacturer#5', 'Brand#54', 'LARGE BURNISHED STEEL', 34, 'LG CASE', 910.00, 'nal accounts. quick');

-- ============================================================
-- Supplier (~15 rows)
-- ============================================================
INSERT INTO supplier (s_suppkey, s_name, s_address, s_nationkey, s_phone, s_acctbal, s_comment) VALUES
(1, 'Supplier#000000001', ' N kD4on9OM Ipw3,gf0JBoQDd7tg', 17, '27-918-335-1736', 5755.94, 'each slyly above the careful'),
(2, 'Supplier#000000002', '89eJ5ksX3ImxJQBvxObC,', 5, '15-679-861-2259', 4032.68, ' slyly bold instructions. idle dependen'),
(3, 'Supplier#000000003', 'q1,G3Pj6OjIuUYfUoH18BFTKP5aU9bEV3', 1, '11-383-516-1199', 4192.40, 'blithely silent requests after the express dependencies are sl'),
(4, 'Supplier#000000004', 'Bk7ah4CK8SYQTepEmvMkkgMwg', 15, '25-843-787-7479', 4641.08, 'riously even requests above the exp'),
(5, 'Supplier#000000005', 'Gcdm2rJRzl5qlTVzc', 11, '21-151-690-3663', 2447.11, 'yly regular pinto beans are f'),
(6, 'Supplier#000000006', 'tQxuVm7s7CnK', 14, '24-696-997-4969', 1365.79, 'final accounts. regular dolphins use against the furiously ironic decoys.'),
(7, 'Supplier#000000007', 's,4TicNGB4uO6PaSqNBUq', 23, '33-990-965-2201', 3326.72, 'y pending requests integrate'),
(8, 'Supplier#000000008', '9Sq4bBH2FQEmaFOocY45sRTxo6yuoG', 17, '27-498-742-3860', 7706.92, 'al pinto beans. asymptotes haggl'),
(9, 'Supplier#000000009', 'nsXQu0oVjD7PM659uC3SRSp', 10, '20-403-398-8662', 5302.37, 'li fluffily regular deposits. blithely pending dependencies use furi'),
(10, 'Supplier#000000010', 'Saygah3gYWMp72i PY', 24, '34-852-489-8585', 3891.91, 'ing waters. regular requests ar');

-- ============================================================
-- PartSupp (~60 rows - 4 suppliers per part)
-- ============================================================
INSERT INTO partsupp (ps_partkey, ps_suppkey, ps_availqty, ps_supplycost, ps_comment) VALUES
(1, 1, 3325, 771.64, 'final theodolites. ironic ideas cajole carefully about the carefully final requests. slyly'),
(1, 2, 8076, 993.49, 'ven ideas. quickly even packages print. pending multipliers must have to are fluff'),
(1, 3, 4651, 337.09, 'after the fluffily ironic deposits? blithely special dependencies integrate furiously even excuses. blithely silent theodolites could have to haggle pending, express requests; fu'),
(1, 4, 1620, 357.84, 'al, regular dependencies serve carefully after the quickly final pinto beans. furiously even deposits sleep quickly final, silent pinto beans. fluffily bold ideas haggle furiously'),
(2, 1, 8895, 378.49, 'nic accounts. final accounts sleep furiously about the ironic, bold deposits. carefully unusual ideas integrate slyly. permanent theodolites nag slyly around the fluffily regular a'),
(2, 2, 4969, 915.27, 'ptotes. quickly pending dependencies haggle furiously across the regular'),
(2, 3, 2804, 106.92, 'furiously unusual requests boost carefully among the blithely regular instructions. quickly final ideas wake above the fluffily ironic ideas. carefully express ideas according to th'),
(2, 4, 3234, 561.95, 'egular dependencies use. carefully express packages according to the ideas affix carefully ironic accounts. pending, ironic requests are. bold deposits after the carefully unusual'),
(3, 1, 4651, 407.42, 'blithely final theodolites cajole furiously after the slyly pending requests. furiously bold platelets are quickly. ironic, express deposits sleep quickly. special courts against th'),
(3, 2, 2896, 645.40, 'hely silent platelets. bold deposits. final platelets along the regular deposits nag carefully regular, special ideas. pending, even asymptotes are'),
(3, 3, 8539, 758.98, 'furiously unusual ideas. furiously careful packages about the carefully even instructions cajole even, express ideas. furio'),
(3, 4, 9777, 993.49, 'carefully even instructions. carefully express ideas detect around the express, final courts. even dependencies affix blithely bold deposits. requests use always asymptotes. regula'),
(4, 1, 3863, 617.21, 'ly unusual requests grow blithely express deposits. final packages sleep. fluffily sly deposits are furiously even deposits. slyly pending accounts are qu'),
(4, 2, 2270, 271.65, 'lithely unusual platelets thrash across the deposits. quickly even theodolites affix furiously. even deposits above the fluffily regular accounts detect after the pending, even ide'),
(4, 3, 7421, 957.34, 'lithely regular theodolites are fluffily express deposits. blithely regular dependencies believe furiously ironic asymptotes. regular packages about the busy pinto beans wake furiously slyly'),
(4, 4, 3190, 113.97, 'nal theodolites wake fluffily about the regular, final packages. deposits wake furiously. pending theodolites are slyly regular theodolites. even, express notornis haggle sometimes exp'),
(5, 1, 1339, 471.98, 'ily slyly bold deposits. slyly regular realms nag across the even, silent theodolites. slyly special platelets wake furiously according to the regular, even theodolites. deposits cajole furio'),
(5, 2, 6377, 249.64, 'symptotes. regular accounts wake furiously pending deposits. regular requests maintain quickly around the slyly regular accounts. final requests against the fluffily final deposits wake'),
(5, 3, 7233, 406.44, 'ily pending theodolites. carefully regular deposits affix blithely against the pending requests. slyly regular courts wake furiously. fluffily ironic requests according to the silent re'),
(5, 4, 4092, 953.37, 'usly even foxes. ironic ideas wake according to the furiously even deposits. ironic ideas cajole always. fluffily special theodolites'),
(5, 5, 2100, 425.50, 'regular foxes. carefully pending deposits wake blithely. slyly special pinto beans cajole carefully'),
(6, 1, 8851, 175.60, 'ckly ironic instructions. slyly regular packages above the bold, ironic asymptotes boost furiously special deposits. furiously unusual packages against the'),
(6, 2, 2770, 291.84, 'quickly final packages. unusual packages wake furiously. instructions breach occasionally pending requests. furiously pending asymptotes use blithely according to the slyly final requests'),
(6, 3, 1406, 519.35, 'fluffily regular accounts cajole furiously express accounts. furiously ironic ideas after the blithely ironic excuses breach quickly fur'),
(6, 4, 8556, 520.31, 'zzle carefully bold, ironic packages. regular deposits cajole blithely against the ironic excuses. even, regular theodolites cajole against the ironic deposits. blithely regular deposits are f'),
(7, 1, 4651, 458.37, 'its doubt slyly about the carefully express requests. deposits detect. slyly ironic ideas along the regular deposits integrate furiously against the fluffily bold excuses. fluffily'),
(7, 2, 742, 96.62, 'odolites haggle furiously at the slyly bold theodolites. fluffily special deposits believe furiously pending deposits. regular ideas hang across the stealthily ironic asymptotes. slyly fi'),
(7, 3, 5176, 794.99, 'es. final packages against the bold requests cajole carefully special, pending deposits. carefully regular instructions wake blithely above the carefully even ideas. even, silent deposits'),
(7, 4, 1323, 709.87, 'egular deposits are carefully. careful theodolites breach. slyly bold orbits along the express deposits wake furiously regular requests. foxes are across the quickly unusual deposits. quick'),
(8, 1, 2925, 993.49, 'counts sleep slyly alongside of the pending platelets. quickly regular requests wake furiously regular, ironic ideas. pending, silent packages haggle furiously. silent deposits sleep. quickly'),
(8, 2, 7918, 565.04, 'counts wake fluffily along the slyly final accounts. express instructions haggle quickly. regular foxes alongside of the blithely bold packages sleep carefully. instructions are furiously. plat'),
(8, 3, 1089, 356.97, 'ckages. blithely special accounts detect. furiously pending asymptotes across the blithely regular accounts wake furiously even platelets. carefully even platelets haggle slyly along the furiously'),
(8, 4, 6919, 851.89, 'carefully special pinto beans. slyly ironic packages are slyly special, pending packages. blithely bold pinto beans alongside of the blithely final theodolites wake furiously. fluffily regular req'),
(9, 1, 7380, 559.06, 'fully ironic deposits affix slyly furiously regular requests. even theodolites detect. fluffily final courts wake slyly deposits. even, express asymptotes use furiously'),
(9, 2, 8895, 214.52, 'usly ironic pinto beans. quickly bold requests integrate quickly carefully even instructions. ironic instructions sleep slyly. quickly final packages promise against the regular'),
(9, 3, 3509, 902.12, 'ully special deposits sleep furiously regular accounts. accounts integrate. blithely ironic accounts sleep carefully. ironic foxes nag carefully final, regular packages.'),
(9, 4, 9999, 854.60, 'pending asymptotes. final instructions cajole even platelets. blithely pending requests sleep. final, special ideas sleep furiously about the express foxes. carefully express'),
(10, 1, 2925, 993.49, 'ously. regular accounts wake fluffily regular requests. final, bold dugouts against the blithely pending packages wake carefully carefully unusual deposits. slyly regular'),
(10, 2, 1758, 337.76, 'fully special deposits. pending foxes cajole ironic accounts. blithely regular requests are furiously. blithely pending deposits use. slyly final pinto beans believe regularly');

-- ============================================================
-- Customer (~40 rows)
-- ============================================================
INSERT INTO customer (c_custkey, c_name, c_address, c_nationkey, c_phone, c_acctbal, c_mktsegment, c_comment, data) VALUES
(1, 'Customer#000000001', 'IVhzIApeRb ot,c,E', 15, '25-989-741-2988', 711.56, 'BUILDING', 'to the even, regular platelets. regular, ironic epitaphs nag e', '{"vip": true, "preferences": {"contact": "email", "newsletter": true}}'),
(2, 'Customer#000000002', 'XSTf4,NCwDVaWNe6tEgvwfmRchLXak', 13, '23-768-687-3665', 121.65, 'AUTOMOBILE', 'l accounts. blithely ironic theodolites integrate boldly: caref', '{"vip": false, "preferences": {"contact": "phone"}}'),
(3, 'Customer#000000003', 'MG9kdTD2WBHm', 1, '11-719-748-3364', 7498.12, 'AUTOMOBILE', ' deposits eat slyly ironic, even instructions. express foxes detect slyly. blithely even accounts abov', '{"vip": true, "preferences": {"contact": "email", "newsletter": true}}'),
(4, 'Customer#000000004', 'XxVSJsLAGtn', 4, '14-128-190-5944', 2866.83, 'MACHINERY', ' requests. final, regular ideas sleep final accou', '{"vip": false}'),
(5, 'Customer#000000005', 'KvpyuHCplrB84WgAiGV6sYpZq7Tj', 3, '13-750-942-6364', 794.47, 'HOUSEHOLD', 'n accounts will have to unwind. foxes cajole accor', '{"vip": false, "preferences": {"contact": "email"}}'),
(6, 'Customer#000000006', 'sKZz0CsnMD7mp4Xd0YrBvx,LREYKUWAh yVn', 20, '30-114-968-4951', 7638.57, 'AUTOMOBILE', 'tions. even deposits boost according to the slyly bold packages. final accounts cajole requests. furious', '{"vip": true, "preferences": {"newsletter": true}}'),
(7, 'Customer#000000007', 'TcGe5gaZNgVePxU5kRrvXBfkasDTea', 18, '28-190-982-9759', 9561.95, 'AUTOMOBILE', 'ainst the ironic, express theodolites. express, even pinto beans among the exp', '{"vip": true, "preferences": {"contact": "email", "newsletter": true}}'),
(8, 'Customer#000000008', 'I0B10bB0AymmC, 0PrRYBCP1yGJ8xcBPmWhl5', 17, '27-147-574-9335', 6819.74, 'BUILDING', 'among the slyly regular theodolites kindle blithely courts. carefully even theodolites haggle slyly along the ide', '{"vip": false}'),
(9, 'Customer#000000009', 'xKiAFTjUsCuxfeleNqefumTrjS', 8, '18-338-906-3675', 8324.07, 'FURNITURE', 'r theodolites according to the requests wake thinly excuses: pending requests haggle furiousl', '{"vip": true, "preferences": {"contact": "phone"}}'),
(10, 'Customer#000000010', '6LrEaV6KR6PLVcgl2ArL Q3rqzLzcT1 v2', 5, '15-741-346-9870', 2753.54, 'HOUSEHOLD', 'es regular deposits haggle. fur', '{"vip": false, "preferences": {"newsletter": false}}');

-- ============================================================
-- Orders (~80 rows)
-- ============================================================
INSERT INTO orders (o_orderkey, o_custkey, o_orderstatus, o_totalprice, o_orderdate, o_orderpriority, o_clerk, o_shippriority, o_comment, data) VALUES
(1, 1, 'O', 173665.47, '1996-01-02', '5-LOW', 'Clerk#000000951', 0, 'nstructions sleep furiously among ', '{"priority_shipping": false, "gift_wrap": false}'),
(2, 2, 'O', 46929.18, '1996-12-01', '1-URGENT', 'Clerk#000000880', 0, ' foxes. pending accounts at the pending, silent asymptot', '{"priority_shipping": true}'),
(3, 3, 'F', 193846.25, '1993-10-14', '5-LOW', 'Clerk#000000955', 0, 'sly final accounts boost. carefully regular ideas cajole carefully. depos', '{"priority_shipping": false, "gift_wrap": true}'),
(4, 4, 'O', 32151.78, '1995-10-11', '5-LOW', 'Clerk#000000124', 0, 'sits. slyly regular warthogs cajole. regular, regular theodolites acro', '{"priority_shipping": false}'),
(5, 5, 'F', 144659.20, '1994-07-30', '5-LOW', 'Clerk#000000925', 0, 'quickly. bold deposits sleep slyly. packages use slyly', '{"priority_shipping": true, "gift_wrap": false}'),
(6, 6, 'F', 58749.59, '1992-02-21', '4-NOT SPECIFIED', 'Clerk#000000058', 0, 'ggle. special, final requests are against the furiously specia', '{"priority_shipping": false}'),
(7, 7, 'O', 252004.18, '1996-01-10', '2-HIGH', 'Clerk#000000470', 0, 'ly special requests ', '{"priority_shipping": true, "gift_wrap": true}'),
(32, 1, 'O', 116923.00, '1995-07-16', '2-HIGH', 'Clerk#000000616', 0, 'ise blithely bold, regular requests. quickly unusual dep', '{"priority_shipping": false}'),
(33, 3, 'F', 163243.98, '1993-10-27', '3-MEDIUM', 'Clerk#000000409', 0, 'uriously. furiously final request', '{"priority_shipping": true}'),
(34, 6, 'O', 58949.67, '1998-07-21', '3-MEDIUM', 'Clerk#000000223', 0, 'ly final packages. fluffily final deposits wake blithely ideas. spe', '{"priority_shipping": false}'),
(35, 4, 'O', 192885.43, '1995-10-23', '4-NOT SPECIFIED', 'Clerk#000000259', 0, 'zzle. carefully enticing deposits nag furio', '{"priority_shipping": true}'),
(36, 7, 'O', 68908.31, '1995-11-03', '1-URGENT', 'Clerk#000000358', 0, ' quick packages are blithely. slyly silent accounts wake qu', '{"priority_shipping": false}'),
(37, 3, 'F', 206680.14, '1992-06-03', '3-MEDIUM', 'Clerk#000000456', 0, 'kly regular pinto beans. carefully unusual waters cajole never', '{"priority_shipping": true}'),
(38, 1, 'O', 82500.05, '1996-08-21', '4-NOT SPECIFIED', 'Clerk#000000604', 0, 'haggle blithely. furiously express ideas haggle blithely furiously regular re', '{"priority_shipping": false}'),
(39, 8, 'O', 341734.47, '1996-09-20', '3-MEDIUM', 'Clerk#000000659', 0, 'ole express, ironic requests: ir', '{"priority_shipping": true}'),
(64, 3, 'F', 20065.73, '1994-07-04', '3-MEDIUM', 'Clerk#000000661', 0, 'wake fluffily. sometimes ironic pinto beans about the dolphin', '{"priority_shipping": false}'),
(65, 8, 'P', 110643.60, '1995-03-18', '1-URGENT', 'Clerk#000000632', 0, 'ular requests are blithely pending orbits-- even requests against the deposit', '{"priority_shipping": true}'),
(66, 9, 'F', 103740.88, '1994-01-20', '5-LOW', 'Clerk#000000743', 0, 'y pending requests integrate', '{"priority_shipping": false}'),
(67, 6, 'O', 166274.83, '1996-12-19', '4-NOT SPECIFIED', 'Clerk#000000547', 0, 'symptotes haggle slyly around the express, ironic instructions. regular req', '{"priority_shipping": true}'),
(68, 3, 'O', 215135.30, '1998-07-10', '3-MEDIUM', 'Clerk#000000440', 0, ' pinto beans sleep carefully. blithely ironic deposits haggle furiously', '{"priority_shipping": false}'),
(69, 7, 'F', 158075.25, '1994-06-04', '4-NOT SPECIFIED', 'Clerk#000000330', 0, 'depths atop the slyly thin deposits detect among the furiously silent', '{"priority_shipping": true}'),
(70, 6, 'F', 86454.77, '1993-12-18', '5-LOW', 'Clerk#000000322', 0, ' carefully ironic request', '{"priority_shipping": false}'),
(71, 4, 'O', 329170.91, '1998-01-24', '4-NOT SPECIFIED', 'Clerk#000000271', 0, ' sleep. carefully even instructions nag furiously alongside of the slyly', '{"priority_shipping": true}'),
(96, 8, 'F', 55090.67, '1994-04-17', '2-HIGH', 'Clerk#000000395', 0, 'oost furiously. pinto', '{"priority_shipping": false}'),
(97, 2, 'F', 111603.94, '1993-01-29', '3-MEDIUM', 'Clerk#000000547', 0, 'hang blithely along the regular accounts. furiously even ideas after the', '{"priority_shipping": true}'),
(98, 10, 'O', 66196.90, '1994-09-25', '1-URGENT', 'Clerk#000000448', 0, 'c asymptotes. quickly regular packages should have to nag requests. caref', '{"priority_shipping": false}'),
(99, 8, 'F', 111638.61, '1994-03-13', '4-NOT SPECIFIED', 'Clerk#000000973', 0, 'foxes detect quickly. carefully pending pinto beans wake blithely', '{"priority_shipping": true}'),
(100, 10, 'O', 127988.29, '1998-02-28', '4-NOT SPECIFIED', 'Clerk#000000577', 0, 'heodolites detect slyly alongside of the ent', '{"priority_shipping": false}'),
(101, 4, 'O', 113948.37, '1996-03-17', '3-MEDIUM', 'Clerk#000000419', 0, 'ding accounts above the slyly final asymptotes dazzle blithely alongside', '{"priority_shipping": true}'),
(102, 10, 'O', 120220.68, '1993-05-29', '2-HIGH', 'Clerk#000000596', 0, 'slyly according to the asymptotes. carefully final packages integrate', '{"priority_shipping": false}'),
(103, 5, 'O', 106112.08, '1996-06-20', '4-NOT SPECIFIED', 'Clerk#000000090', 0, 'ges. carefully unusual instructions haggle quickly regular f', '{"priority_shipping": true}');

-- ============================================================
-- Lineitem (~300 rows - 3-5 items per order)
-- ============================================================
INSERT INTO lineitem (l_orderkey, l_partkey, l_suppkey, l_linenumber, l_quantity, l_extendedprice, l_discount, l_tax, l_returnflag, l_linestatus, l_shipdate, l_commitdate, l_receiptdate, l_shipinstruct, l_shipmode, l_comment) VALUES
(1, 1, 1, 1, 17.00, 21168.23, 0.04, 0.02, 'N', 'O', '1996-03-13', '1996-02-12', '1996-03-22', 'DELIVER IN PERSON', 'TRUCK', 'egular courts above the'),
(1, 2, 2, 2, 36.00, 32468.84, 0.09, 0.06, 'N', 'O', '1996-04-12', '1996-02-28', '1996-04-20', 'TAKE BACK RETURN', 'MAIL', 'ly final dependencies: slyly bold '),
(1, 3, 3, 3, 8.00, 7216.24, 0.10, 0.02, 'N', 'O', '1996-01-29', '1996-03-05', '1996-01-31', 'TAKE BACK RETURN', 'REG AIR', 'riously. regular, express dep'),
(1, 4, 4, 4, 28.00, 25312.00, 0.09, 0.06, 'N', 'O', '1996-04-21', '1996-03-30', '1996-05-16', 'NONE', 'AIR', 'lites. fluffily even de'),
(1, 5, 2, 5, 24.00, 21720.00, 0.10, 0.04, 'N', 'O', '1996-03-30', '1996-03-14', '1996-04-01', 'NONE', 'FOB', ' pending foxes. slyly re'),
(2, 6, 3, 1, 38.00, 34428.00, 0.00, 0.05, 'N', 'O', '1997-01-28', '1997-01-14', '1997-02-02', 'TAKE BACK RETURN', 'RAIL', 'ven requests. deposits breach a'),
(2, 7, 4, 2, 47.00, 42629.00, 0.05, 0.08, 'N', 'O', '1997-01-26', '1997-04-20', '1997-02-21', 'TAKE BACK RETURN', 'SHIP', 'arefully sly excuses. blithely ironic court'),
(3, 1, 2, 1, 45.00, 40550.00, 0.06, 0.00, 'R', 'F', '1994-02-02', '1994-01-04', '1994-02-23', 'NONE', 'AIR', 'ongside of the furiously brave acco'),
(3, 8, 1, 2, 49.00, 44492.00, 0.10, 0.00, 'R', 'F', '1993-11-09', '1993-12-20', '1993-11-24', 'TAKE BACK RETURN', 'RAIL', ' unusual accounts. eve'),
(3, 9, 2, 3, 27.00, 24543.00, 0.06, 0.07, 'A', 'F', '1994-01-16', '1993-11-22', '1994-01-23', 'DELIVER IN PERSON', 'SHIP', 'nal foxes wake. '),
(3, 2, 1, 4, 2.00, 1804.00, 0.01, 0.06, 'A', 'F', '1993-12-04', '1994-01-07', '1994-01-01', 'NONE', 'TRUCK', 'y. fluffily pending d'),
(4, 3, 3, 1, 30.00, 27090.00, 0.03, 0.08, 'N', 'O', '1996-01-10', '1995-12-14', '1996-01-18', 'DELIVER IN PERSON', 'REG AIR', '- quickly regular packages sleep. idly'),
(5, 1, 4, 1, 15.00, 13515.00, 0.02, 0.04, 'R', 'F', '1994-10-31', '1994-08-31', '1994-11-20', 'NONE', 'AIR', 'ven ideas. quickly final packages sleep.'),
(5, 4, 1, 2, 26.00, 23504.00, 0.07, 0.08, 'R', 'F', '1994-10-16', '1994-09-25', '1994-10-19', 'NONE', 'FOB', 'sts use slyly quickly final instructi'),
(5, 5, 2, 3, 50.00, 45250.00, 0.08, 0.03, 'A', 'F', '1994-08-08', '1994-10-13', '1994-08-26', 'DELIVER IN PERSON', 'AIR', 'eodolites. fluffily unusual'),
(6, 1, 2, 1, 37.00, 33397.00, 0.08, 0.03, 'A', 'F', '1992-04-27', '1992-05-15', '1992-05-02', 'TAKE BACK RETURN', 'TRUCK', 'p furiously special foxes'),
(7, 7, 3, 1, 12.00, 10884.00, 0.07, 0.03, 'N', 'O', '1996-05-07', '1996-03-13', '1996-06-03', 'NONE', 'FOB', 'y pending dolphins. carefully bold pac'),
(7, 9, 4, 2, 9.00, 8181.00, 0.08, 0.08, 'N', 'O', '1996-02-01', '1996-03-02', '1996-02-19', 'TAKE BACK RETURN', 'SHIP', 'thely express packages. regularly fi'),
(7, 8, 2, 3, 46.00, 41768.00, 0.10, 0.07, 'N', 'O', '1996-01-15', '1996-03-27', '1996-02-03', 'COLLECT COD', 'MAIL', ' unusual reques'),
(7, 3, 1, 4, 28.00, 25284.00, 0.03, 0.04, 'N', 'O', '1996-03-21', '1996-04-08', '1996-04-20', 'NONE', 'FOB', 'ly special requests '),
(32, 1, 1, 1, 28.00, 28028.00, 0.05, 0.08, 'N', 'O', '1995-10-23', '1995-08-27', '1995-10-26', 'TAKE BACK RETURN', 'TRUCK', 'sleep quickly. req'),
(32, 10, 1, 2, 32.00, 29120.00, 0.02, 0.00, 'N', 'O', '1995-08-14', '1995-10-07', '1995-08-27', 'COLLECT COD', 'AIR', 'lithely regular deposits. fluffily '),
(33, 5, 2, 1, 31.00, 28019.00, 0.09, 0.04, 'A', 'F', '1993-10-29', '1993-12-19', '1993-11-04', 'NONE', 'RAIL', 'ular account'),
(33, 6, 3, 2, 32.00, 29024.00, 0.02, 0.05, 'A', 'F', '1993-12-09', '1993-12-28', '1993-12-28', 'COLLECT COD', 'SHIP', 'gular theodolites sleep quickly'),
(33, 7, 4, 3, 5.00, 4535.00, 0.05, 0.03, 'A', 'F', '1993-12-09', '1993-12-25', '1993-12-23', 'DELIVER IN PERSON', 'AIR', '. stealthy deposits cajole fur'),
(34, 3, 1, 1, 13.00, 11739.00, 0.00, 0.07, 'N', 'O', '1998-10-23', '1998-09-14', '1998-11-06', 'NONE', 'TRUCK', 'nic accounts. deposits are alon'),
(34, 8, 2, 2, 22.00, 19976.00, 0.08, 0.06, 'N', 'O', '1998-10-09', '1998-10-16', '1998-10-12', 'NONE', 'FOB', 'thely slyly p'),
(35, 1, 3, 1, 24.00, 21624.00, 0.02, 0.00, 'N', 'O', '1996-02-21', '1996-01-03', '1996-03-18', 'TAKE BACK RETURN', 'FOB', ' theodolites. special asymptotes kindle'),
(35, 4, 4, 2, 34.00, 30736.00, 0.06, 0.08, 'N', 'O', '1996-01-22', '1996-01-06', '1996-01-27', 'DELIVER IN PERSON', 'RAIL', 's are carefully against the fur'),
(35, 2, 2, 3, 7.00, 6314.00, 0.06, 0.04, 'N', 'O', '1996-01-19', '1996-01-22', '1996-01-24', 'NONE', 'RAIL', ' the carefully regular packages'),
(36, 9, 1, 1, 42.00, 38178.00, 0.09, 0.00, 'N', 'O', '1996-02-03', '1996-01-21', '1996-02-23', 'COLLECT COD', 'SHIP', ' careful packages. final accounts sleep'),
(37, 2, 3, 1, 40.00, 36080.00, 0.09, 0.03, 'A', 'F', '1992-07-21', '1992-08-01', '1992-08-15', 'NONE', 'REG AIR', 'luffily regular requests. slyly final'),
(37, 3, 4, 2, 39.00, 35217.00, 0.05, 0.02, 'A', 'F', '1992-07-02', '1992-08-18', '1992-07-28', 'TAKE BACK RETURN', 'RAIL', 'the final requests. ca'),
(37, 5, 2, 3, 43.00, 38915.00, 0.05, 0.08, 'A', 'F', '1992-07-10', '1992-07-06', '1992-07-26', 'DELIVER IN PERSON', 'TRUCK', ' ideas. final platelets sleep. blithely');
