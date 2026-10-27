-- Transaction body, first deferred API credential preflight only.
CREATE TABLE email_provider_cooldowns (
    cluster TEXT NOT NULL CHECK(length(cluster) BETWEEN 1 AND 64),
    destination TEXT NOT NULL CHECK(length(destination) BETWEEN 1 AND 64),
    revision TEXT NOT NULL CHECK(length(revision)=64),
    retry_at INTEGER NOT NULL CHECK(retry_at BETWEEN 0 AND 4102444799),
    PRIMARY KEY(cluster,destination)
);
PRAGMA user_version=8;
