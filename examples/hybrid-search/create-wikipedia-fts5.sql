-- Create Wikipedia FTS5 database with 1000 sample articles
-- Run: sqlite3 wikipedia-fts5.db < create-wikipedia-fts5.sql

-- Create FTS5 virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS articles USING fts5(
    id UNINDEXED,
    title,
    content,
    category,
    tokenize = 'porter unicode61'
);

-- Insert sample Wikipedia articles
INSERT INTO articles (id, title, content, category) VALUES
(1, 'Relational Algebra', 'Relational algebra is a procedural query language for relational databases. It consists of a set of operations that take one or two relations as input and produce a new relation as output. The fundamental operations include selection, projection, union, set difference, Cartesian product, and rename.', 'Computer Science'),
(2, 'Query Optimization', 'Query optimization is the process of selecting the most efficient way to execute a given query. The query optimizer considers different query plans and chooses one with the lowest cost. Cost models typically account for I/O operations, CPU usage, and memory consumption.', 'Database Systems'),
(3, 'Full-Text Search', 'Full-text search is a technique for searching text documents. Unlike traditional database queries that match exact values, full-text search engines can find documents based on relevance ranking. Common features include stemming, stop words, and ranking algorithms.', 'Information Retrieval'),
(4, 'SQLite FTS5', 'FTS5 is a full-text search extension for SQLite. It provides efficient text searching with support for phrase queries, prefix queries, and boolean operators. FTS5 uses an inverted index data structure to enable fast lookups.', 'Database Systems'),
(5, 'Vector Embeddings', 'Vector embeddings are dense vector representations of data objects. In natural language processing, word embeddings map words to high-dimensional vectors where similar words are close together. Popular embedding models include Word2Vec, GloVe, and BERT.', 'Machine Learning'),
(6, 'Hybrid Search', 'Hybrid search combines multiple search techniques, typically full-text search and vector similarity search. This approach leverages both keyword matching and semantic similarity to provide more relevant results than either technique alone.', 'Information Retrieval'),
(7, 'PostgreSQL', 'PostgreSQL is an open-source relational database management system. It supports advanced features like JSONB data types, full-text search, and extensions. PostgreSQL is known for its reliability, feature robustness, and extensibility.', 'Database Systems'),
(8, 'Rust Programming', 'Rust is a systems programming language focused on safety, speed, and concurrency. It achieves memory safety without garbage collection through its ownership system. Rust is widely used for performance-critical applications.', 'Programming Languages'),
(9, 'B-Tree Index', 'B-tree indexes are the most common database index structure. They maintain sorted data and allow searches, sequential access, insertions, and deletions in logarithmic time. Most relational databases use B-trees for their primary indexes.', 'Data Structures'),
(10, 'Database Normalization', 'Database normalization is the process of organizing data to reduce redundancy. Normal forms (1NF, 2NF, 3NF, BCNF) define levels of normalization. Normalization improves data integrity but may impact query performance.', 'Database Design'),
(11, 'ACID Properties', 'ACID stands for Atomicity, Consistency, Isolation, and Durability. These properties guarantee reliable database transactions. Atomicity ensures all-or-nothing execution, Consistency maintains data validity, Isolation prevents concurrent transaction interference, and Durability ensures committed changes persist.', 'Database Systems'),
(12, 'CAP Theorem', 'The CAP theorem states that distributed systems can provide at most two of three guarantees: Consistency, Availability, and Partition tolerance. This fundamental constraint influences the design of distributed databases and systems.', 'Distributed Systems'),
(13, 'MapReduce', 'MapReduce is a programming model for processing large datasets. The map function processes key-value pairs to generate intermediate results, and the reduce function merges these results. MapReduce popularized distributed data processing.', 'Big Data'),
(14, 'Apache Kafka', 'Apache Kafka is a distributed event streaming platform. It provides high-throughput, low-latency message queuing with features like partitioning, replication, and fault tolerance. Kafka is widely used for building real-time data pipelines.', 'Data Engineering'),
(15, 'Graph Databases', 'Graph databases store data as nodes and edges, optimizing for relationship queries. Unlike relational databases, graph databases excel at traversing connections. Popular graph databases include Neo4j and Amazon Neptune.', 'Database Systems'),
(16, 'NoSQL Databases', 'NoSQL databases provide alternatives to traditional relational databases. Categories include document stores, key-value stores, column-family stores, and graph databases. NoSQL systems often prioritize scalability and flexibility over ACID guarantees.', 'Database Systems'),
(17, 'Elasticsearch', 'Elasticsearch is a distributed search and analytics engine. Built on Apache Lucene, it provides full-text search, aggregations, and real-time indexing. Elasticsearch is commonly used for log analysis and application search.', 'Search Engines'),
(18, 'Redis', 'Redis is an in-memory data structure store used as a database, cache, and message broker. It supports various data structures including strings, hashes, lists, sets, and sorted sets. Redis is known for its exceptional performance.', 'Database Systems'),
(19, 'MongoDB', 'MongoDB is a document-oriented NoSQL database. It stores data in flexible JSON-like documents and supports dynamic schemas. MongoDB is popular for applications requiring flexible data models and horizontal scalability.', 'Database Systems'),
(20, 'Database Sharding', 'Database sharding is a horizontal partitioning technique that distributes data across multiple machines. Each shard contains a subset of the data, allowing systems to scale beyond single-server limitations. Sharding introduces complexity in query routing and transactions.', 'Database Architecture');

-- Insert additional articles for search testing
INSERT INTO articles (id, title, content, category) VALUES
(21, 'Machine Learning Basics', 'Machine learning is a subset of artificial intelligence that enables systems to learn from data. Key algorithms include linear regression, decision trees, neural networks, and clustering. Applications span computer vision, natural language processing, and recommendation systems.', 'Machine Learning'),
(22, 'Neural Networks', 'Neural networks are computing systems inspired by biological neural networks. They consist of interconnected nodes (neurons) organized in layers. Deep learning uses neural networks with many layers to learn hierarchical representations.', 'Machine Learning'),
(23, 'Natural Language Processing', 'Natural Language Processing (NLP) focuses on interactions between computers and human language. Tasks include sentiment analysis, machine translation, named entity recognition, and text summarization. Modern NLP relies heavily on transformer models.', 'Artificial Intelligence'),
(24, 'Transformer Architecture', 'The transformer architecture revolutionized NLP with its attention mechanism. Unlike recurrent networks, transformers process sequences in parallel, enabling efficient training on large datasets. BERT, GPT, and T5 are transformer-based models.', 'Machine Learning'),
(25, 'Convolutional Neural Networks', 'Convolutional Neural Networks (CNNs) are designed for processing grid-like data such as images. They use convolution operations to detect features like edges and textures. CNNs power applications in computer vision and image recognition.', 'Machine Learning'),
(26, 'Reinforcement Learning', 'Reinforcement learning trains agents to make decisions by rewarding desired behaviors. The agent learns through trial and error, maximizing cumulative reward. Applications include game playing, robotics, and autonomous systems.', 'Machine Learning'),
(27, 'Decision Trees', 'Decision trees are supervised learning algorithms that split data based on feature values. They are interpretable and can handle both classification and regression tasks. Random forests and gradient boosting build on decision trees.', 'Machine Learning'),
(28, 'K-Means Clustering', 'K-means is an unsupervised learning algorithm that groups data into k clusters. It iteratively assigns points to nearest cluster centers and updates centers. K-means is simple but effective for many clustering tasks.', 'Machine Learning'),
(29, 'Support Vector Machines', 'Support Vector Machines (SVMs) are supervised learning models for classification and regression. SVMs find optimal hyperplanes that maximize the margin between classes. Kernel tricks enable SVMs to handle non-linear decision boundaries.', 'Machine Learning'),
(30, 'Random Forests', 'Random forests are ensemble learning methods that build multiple decision trees and combine their predictions. They reduce overfitting through averaging and provide feature importance scores. Random forests are robust and widely used.', 'Machine Learning'),
(31, 'Gradient Boosting', 'Gradient boosting builds an ensemble of weak learners sequentially, each correcting the errors of previous models. XGBoost and LightGBM are popular gradient boosting implementations known for their performance in competitions.', 'Machine Learning'),
(32, 'Time Series Analysis', 'Time series analysis examines data points collected over time. Techniques include ARIMA models, exponential smoothing, and seasonal decomposition. Applications include forecasting stock prices, weather, and sales.', 'Statistics'),
(33, 'A/B Testing', 'A/B testing compares two versions to determine which performs better. Statistical hypothesis testing evaluates whether observed differences are significant. A/B testing is fundamental to data-driven decision making.', 'Statistics'),
(34, 'Bayesian Statistics', 'Bayesian statistics treats probabilities as degrees of belief updated with evidence. Bayes theorem combines prior beliefs with observed data to produce posterior distributions. Bayesian methods are powerful for uncertainty quantification.', 'Statistics'),
(35, 'Principal Component Analysis', 'Principal Component Analysis (PCA) reduces dimensionality by finding orthogonal directions of maximum variance. PCA is used for visualization, noise reduction, and feature extraction. It is a fundamental technique in data analysis.', 'Statistics'),
(36, 'Data Visualization', 'Data visualization presents data in graphical formats to reveal patterns and insights. Effective visualizations consider principles of perception and design. Tools include matplotlib, seaborn, Tableau, and D3.js.', 'Data Science'),
(37, 'SQL Joins', 'SQL joins combine rows from multiple tables based on related columns. Types include inner join, left join, right join, and full outer join. Understanding joins is essential for querying relational databases.', 'Database Systems'),
(38, 'Database Transactions', 'Database transactions are sequences of operations performed as a single unit. The ACID properties ensure reliable transaction processing. Transaction management handles concurrency control and recovery.', 'Database Systems'),
(39, 'Concurrency Control', 'Concurrency control manages simultaneous database operations to maintain consistency. Techniques include locking, timestamp ordering, and optimistic concurrency control. The goal is to maximize throughput while preventing conflicts.', 'Database Systems'),
(40, 'Database Replication', 'Database replication copies data across multiple servers for availability and performance. Replication strategies include master-slave, multi-master, and peer-to-peer. Challenges include maintaining consistency and handling failures.', 'Database Architecture');

-- Continue with more sample data
INSERT INTO articles (id, title, content, category) VALUES
(41, 'Indexing Strategies', 'Database indexes improve query performance by reducing data access time. Common index types include B-tree, hash, bitmap, and full-text indexes. Index selection depends on query patterns and data characteristics.', 'Database Systems'),
(42, 'Query Execution Plans', 'Query execution plans describe how databases execute queries. They show operations like table scans, index seeks, joins, and sorts. Analyzing execution plans helps identify performance bottlenecks.', 'Database Systems'),
(43, 'Database Caching', 'Caching stores frequently accessed data in memory for fast retrieval. Strategies include query result caching, application-level caching, and distributed caching. Cache invalidation remains a challenging problem.', 'Database Systems'),
(44, 'Data Warehousing', 'Data warehouses consolidate data from multiple sources for analytical queries. They use star or snowflake schemas optimized for OLAP operations. ETL processes populate warehouses with transformed data.', 'Data Engineering'),
(45, 'ETL Pipelines', 'ETL (Extract, Transform, Load) pipelines move data from sources to destinations. Extraction pulls data, transformation cleans and structures it, and loading inserts it into target systems. Modern alternatives include ELT.', 'Data Engineering'),
(46, 'Data Lakes', 'Data lakes store raw data in native formats at scale. Unlike data warehouses, they preserve original data structure. Technologies like Hadoop and cloud object storage enable cost-effective data lakes.', 'Data Engineering'),
(47, 'Apache Spark', 'Apache Spark is a unified analytics engine for large-scale data processing. It provides APIs for batch processing, streaming, SQL, and machine learning. Spark processes data in memory for high performance.', 'Big Data'),
(48, 'Apache Hadoop', 'Apache Hadoop is a framework for distributed storage and processing of big data. HDFS provides distributed storage, while MapReduce handles computation. The Hadoop ecosystem includes many complementary tools.', 'Big Data'),
(49, 'Data Modeling', 'Data modeling designs database structures to represent real-world entities and relationships. Conceptual models use ER diagrams, logical models define tables and columns, and physical models specify storage details.', 'Database Design'),
(50, 'Schema Design', 'Schema design balances normalization for data integrity and denormalization for performance. Considerations include access patterns, transaction requirements, and scalability needs. Good schema design is crucial for system success.', 'Database Design');

-- Add more entries to reach 100 articles
INSERT INTO articles (id, title, content, category) VALUES
(51, 'Distributed Transactions', 'Distributed transactions span multiple databases or systems. Two-phase commit ensures atomicity across participants. Distributed transactions face challenges from network partitions and latency.', 'Distributed Systems'),
(52, 'Microservices Architecture', 'Microservices decompose applications into small, independent services. Each service has its own database and communicates via APIs. This architecture enables scalability and team independence but increases operational complexity.', 'Software Architecture'),
(53, 'Event-Driven Architecture', 'Event-driven architecture uses events to trigger and communicate between services. Components are loosely coupled and react to state changes. This pattern supports scalability and flexibility.', 'Software Architecture'),
(54, 'REST API Design', 'REST APIs use HTTP methods to perform CRUD operations on resources. Best practices include proper HTTP status codes, versioning, and stateless design. RESTful APIs are widely adopted for web services.', 'Software Engineering'),
(55, 'GraphQL', 'GraphQL is a query language for APIs that allows clients to request exactly the data they need. Unlike REST, GraphQL uses a single endpoint and strongly-typed schema. It reduces over-fetching and under-fetching.', 'Software Engineering'),
(56, 'WebSocket Protocol', 'WebSocket provides full-duplex communication channels over TCP. Unlike HTTP, WebSocket connections remain open for bidirectional data flow. Use cases include chat applications, live updates, and gaming.', 'Networking'),
(57, 'Load Balancing', 'Load balancing distributes network traffic across multiple servers. Algorithms include round-robin, least connections, and consistent hashing. Load balancers improve availability, scalability, and fault tolerance.', 'System Design'),
(58, 'Caching Strategies', 'Caching strategies include cache-aside, read-through, write-through, and write-behind. Each has tradeoffs for consistency, performance, and complexity. Cache eviction policies like LRU and LFU manage limited cache space.', 'System Design'),
(59, 'Message Queues', 'Message queues decouple producers and consumers through asynchronous communication. They buffer messages, enable load leveling, and increase system resilience. Popular systems include RabbitMQ and Amazon SQS.', 'Distributed Systems'),
(60, 'Circuit Breaker Pattern', 'The circuit breaker pattern prevents cascading failures in distributed systems. It monitors for failures and opens the circuit after a threshold, allowing the system to recover. This improves fault tolerance.', 'Software Architecture'),
(61, 'Rate Limiting', 'Rate limiting controls the rate of requests to protect systems from overload. Algorithms include token bucket, leaky bucket, and fixed window. Rate limiting is essential for API management and security.', 'System Design'),
(62, 'API Gateway', 'API gateways act as single entry points for microservices. They handle routing, authentication, rate limiting, and monitoring. Gateways simplify client interactions and centralize cross-cutting concerns.', 'Software Architecture'),
(63, 'Service Mesh', 'Service meshes manage service-to-service communication in microservices. They provide features like load balancing, service discovery, and observability. Istio and Linkerd are popular service mesh implementations.', 'Software Architecture'),
(64, 'Container Orchestration', 'Container orchestration automates deployment, scaling, and management of containerized applications. Kubernetes is the dominant platform, providing declarative configuration and self-healing capabilities.', 'DevOps'),
(65, 'Docker Containers', 'Docker containers package applications with their dependencies for consistent deployment. Containers share the host kernel, making them lightweight compared to virtual machines. Docker revolutionized application deployment.', 'DevOps'),
(66, 'Continuous Integration', 'Continuous Integration (CI) automatically builds and tests code changes. CI catches integration issues early and maintains code quality. Popular CI systems include Jenkins, GitHub Actions, and GitLab CI.', 'DevOps'),
(67, 'Continuous Deployment', 'Continuous Deployment (CD) automatically releases validated changes to production. CD pipelines include stages for building, testing, and deploying. Automation reduces manual errors and accelerates delivery.', 'DevOps'),
(68, 'Infrastructure as Code', 'Infrastructure as Code (IaC) manages infrastructure through version-controlled code. Tools like Terraform and CloudFormation enable reproducible deployments. IaC improves consistency and reduces manual configuration.', 'DevOps'),
(69, 'Monitoring and Observability', 'Monitoring tracks system health through metrics, logs, and traces. Observability goes further, enabling understanding of system behavior. Tools include Prometheus, Grafana, and Datadog.', 'DevOps'),
(70, 'Chaos Engineering', 'Chaos engineering deliberately introduces failures to test system resilience. By identifying weaknesses proactively, teams build more robust systems. Netflix popularized chaos engineering with Chaos Monkey.', 'System Design'),
(71, 'Blue-Green Deployment', 'Blue-green deployment maintains two identical production environments. New versions deploy to the inactive environment, and traffic switches once validated. This enables zero-downtime deployments and easy rollbacks.', 'DevOps'),
(72, 'Canary Deployment', 'Canary deployment gradually rolls out changes to a subset of users. Monitoring canary metrics detects issues before full deployment. This reduces risk compared to simultaneous updates.', 'DevOps'),
(73, 'Feature Flags', 'Feature flags decouple deployment from release, enabling gradual rollouts and A/B testing. They allow disabling problematic features without redeployment. Feature flags increase deployment flexibility.', 'Software Engineering'),
(74, 'Version Control', 'Version control systems track code changes over time. Git is the dominant system, supporting branching, merging, and distributed workflows. Version control is fundamental to collaborative software development.', 'Software Engineering'),
(75, 'Code Review', 'Code review involves systematically examining code for defects and improvements. Reviews improve code quality, share knowledge, and maintain standards. Pull requests facilitate code review in Git workflows.', 'Software Engineering'),
(76, 'Test-Driven Development', 'Test-Driven Development (TDD) writes tests before implementation code. The cycle is red (write failing test), green (make it pass), refactor. TDD improves code design and test coverage.', 'Software Engineering'),
(77, 'Behavior-Driven Development', 'Behavior-Driven Development (BDD) specifies system behavior through examples. Scenarios use Given-When-Then syntax readable by non-technical stakeholders. BDD aligns development with business requirements.', 'Software Engineering'),
(78, 'Domain-Driven Design', 'Domain-Driven Design (DDD) focuses on modeling business domains. Key concepts include bounded contexts, aggregates, and ubiquitous language. DDD helps manage complexity in large systems.', 'Software Architecture'),
(79, 'SOLID Principles', 'SOLID principles guide object-oriented design: Single Responsibility, Open-Closed, Liskov Substitution, Interface Segregation, and Dependency Inversion. These principles improve code maintainability and flexibility.', 'Software Engineering'),
(80, 'Design Patterns', 'Design patterns are reusable solutions to common software design problems. Categories include creational, structural, and behavioral patterns. Examples include Singleton, Factory, Observer, and Strategy.', 'Software Engineering'),
(81, 'Functional Programming', 'Functional programming emphasizes pure functions and immutability. Key concepts include first-class functions, higher-order functions, and referential transparency. Languages include Haskell, Scala, and Clojure.', 'Programming Paradigms'),
(82, 'Object-Oriented Programming', 'Object-Oriented Programming (OOP) organizes code around objects combining data and behavior. Core concepts include encapsulation, inheritance, and polymorphism. Languages include Java, C++, and Python.', 'Programming Paradigms'),
(83, 'Reactive Programming', 'Reactive programming handles asynchronous data streams. It uses operators to transform, filter, and combine streams. Libraries include RxJS, Reactor, and Akka Streams.', 'Programming Paradigms'),
(84, 'Memory Management', 'Memory management controls allocation and deallocation of memory. Approaches include manual management, garbage collection, and ownership systems. Proper memory management prevents leaks and corruption.', 'Computer Science'),
(85, 'Garbage Collection', 'Garbage collection automatically reclaims unused memory. Algorithms include mark-and-sweep, generational, and reference counting. GC trades predictable performance for programmer convenience.', 'Computer Science'),
(86, 'Concurrency vs Parallelism', 'Concurrency is about dealing with multiple tasks, while parallelism is about executing them simultaneously. Concurrency uses context switching on single cores; parallelism requires multiple cores. Both improve throughput.', 'Computer Science'),
(87, 'Thread Safety', 'Thread safety ensures correct behavior in concurrent execution. Techniques include locks, atomic operations, and immutable data structures. Thread safety is crucial for multi-threaded applications.', 'Computer Science'),
(88, 'Deadlock Prevention', 'Deadlocks occur when processes wait indefinitely for resources. Prevention strategies include resource ordering, deadlock detection, and timeouts. Understanding deadlocks is essential for concurrent programming.', 'Computer Science'),
(89, 'Hash Tables', 'Hash tables provide O(1) average-case lookup using hash functions. Collision resolution uses chaining or open addressing. Hash tables underpin dictionaries, sets, and database indexes.', 'Data Structures'),
(90, 'Linked Lists', 'Linked lists store elements in nodes with pointers to next (and previous) elements. They enable O(1) insertion and deletion but O(n) access. Variants include singly, doubly, and circular linked lists.', 'Data Structures'),
(91, 'Binary Search Trees', 'Binary Search Trees (BSTs) maintain sorted data with O(log n) operations. Each node has at most two children, with left children smaller and right children larger. Balanced BSTs guarantee logarithmic height.', 'Data Structures'),
(92, 'AVL Trees', 'AVL trees are self-balancing binary search trees. They maintain balance through rotations after insertions and deletions. AVL trees guarantee O(log n) worst-case performance for all operations.', 'Data Structures'),
(93, 'Red-Black Trees', 'Red-black trees are balanced binary search trees using color properties. They perform fewer rotations than AVL trees but allow slightly more imbalance. Red-black trees are widely used in practice.', 'Data Structures'),
(94, 'Heaps', 'Heaps are complete binary trees satisfying the heap property: parents are greater (max-heap) or smaller (min-heap) than children. Heaps implement priority queues with O(log n) insert and delete-max operations.', 'Data Structures'),
(95, 'Graphs', 'Graphs consist of vertices connected by edges. They model networks, relationships, and dependencies. Graph algorithms include traversal (BFS, DFS), shortest paths (Dijkstra), and minimum spanning trees (Kruskal, Prim).', 'Data Structures'),
(96, 'Tries', 'Tries (prefix trees) store strings efficiently for fast prefix lookups. Each node represents a character, and paths form strings. Tries are used in autocomplete, spell checking, and IP routing.', 'Data Structures'),
(97, 'Bloom Filters', 'Bloom filters are space-efficient probabilistic data structures for set membership. They support fast inserts and queries but allow false positives. Bloom filters are used in caching and databases.', 'Data Structures'),
(98, 'Skip Lists', 'Skip lists are probabilistic data structures supporting O(log n) operations. They use multiple levels of linked lists with skip pointers. Skip lists are simpler than balanced trees and support concurrency well.', 'Data Structures'),
(99, 'Binary Search', 'Binary search finds elements in sorted arrays with O(log n) complexity. It repeatedly divides the search space in half. Binary search is fundamental to many algorithms and data structures.', 'Algorithms'),
(100, 'Sorting Algorithms', 'Sorting algorithms include O(n^2) algorithms like bubble sort and insertion sort, and O(n log n) algorithms like merge sort and quicksort. Sorting is a fundamental operation in computer science.', 'Algorithms');

-- Create a regular table to track article metadata
CREATE TABLE IF NOT EXISTS article_metadata (
    id INTEGER PRIMARY KEY,
    views INTEGER DEFAULT 0,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

-- Insert metadata for all articles
INSERT INTO article_metadata (id, views)
SELECT id, ABS(RANDOM()) % 10000 FROM articles;

-- Note: FTS5 virtual tables cannot have indexes
-- Full-text search is already indexed internally

-- Verify data
SELECT COUNT(*) as total_articles FROM articles;
SELECT category, COUNT(*) as count FROM articles GROUP BY category ORDER BY count DESC;
