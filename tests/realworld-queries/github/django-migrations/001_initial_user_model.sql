-- Django User Model Migration (typical Django pattern)
-- Source: Django applications (auth app)
-- Pattern: OLTP - User management with constraints

CREATE TABLE auth_user (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    password VARCHAR(128) NOT NULL,
    last_login TIMESTAMP NULL,
    is_superuser BOOLEAN NOT NULL DEFAULT FALSE,
    username VARCHAR(150) NOT NULL UNIQUE,
    first_name VARCHAR(150) NOT NULL,
    last_name VARCHAR(150) NOT NULL,
    email VARCHAR(254) NOT NULL,
    is_staff BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    date_joined TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_auth_user_username ON auth_user(username);
CREATE INDEX idx_auth_user_email ON auth_user(email);

-- Typical Django query patterns
-- User login lookup
SELECT id, password, is_active, is_staff, is_superuser
FROM auth_user
WHERE username = 'john_doe' AND is_active = TRUE;

-- User listing with pagination
SELECT id, username, email, first_name, last_name, date_joined
FROM auth_user
WHERE is_active = TRUE
ORDER BY date_joined DESC
LIMIT 25 OFFSET 0;
