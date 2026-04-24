use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Error as SqlxError;
use std::env;

#[derive(Clone)]
pub struct DatabaseConfig {
    pub user: String,
    pub host: String,
    pub database: String,
    pub password: String,
    pub port: u16,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            user: env::var("DB_USER").unwrap_or_else(|_| "postgres".to_string()),
            host: env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string()),
            database: env::var("DB_NAME").unwrap_or_else(|_| "terromia_rust".to_string()),
            password: env::var("DB_PASSWORD").unwrap_or_else(|_| "1234".to_string()),
            port: env::var("DB_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432),
        }
    }
}

pub struct Database {
    pool: Option<PgPool>,
    config: DatabaseConfig,
}

impl Database {
    pub fn new() -> Self {
        Self {
            pool: None,
            config: DatabaseConfig::default(),
        }
    }

    pub fn with_config(config: DatabaseConfig) -> Self {
        Self {
            pool: None,
            config,
        }
    }

    pub async fn connect(&mut self) -> Result<&PgPool, SqlxError> {
        if self.pool.is_some() {
            return Ok(self.pool.as_ref().unwrap());
        }

        // First connect to postgres database to check/create target database
        let postgres_url = format!(
            "postgres://{}:{}@{}:{}/postgres",
            self.config.user, self.config.password, self.config.host, self.config.port
        );

        let temp_pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&postgres_url)
            .await?;

        // Check if database exists, create if not
        let db_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)"
        )
        .bind(&self.config.database)
        .fetch_one(&temp_pool)
        .await?;

        if !db_exists {
            sqlx::query(&format!("CREATE DATABASE {}", self.config.database))
                .execute(&temp_pool)
                .await?;
        }

        temp_pool.close().await;

        // Connect to the target database
        let database_url = format!(
            "postgres://{}:{}@{}:{}/{}",
            self.config.user,
            self.config.password,
            self.config.host,
            self.config.port,
            self.config.database
        );

        self.pool = Some(
            PgPoolOptions::new()
                .max_connections(10)
                .connect(&database_url)
                .await?,
        );

        println!("Connected to PostgreSQL database.");

        self.initialize_schema().await?;

        Ok(self.pool.as_ref().unwrap())
    }

    async fn initialize_schema(&self) -> Result<(), SqlxError> {
        let pool = self.pool.as_ref().expect("Pool not initialized");

        // Create players table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS players (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE,
                x REAL, y REAL, z REAL,
                inventory JSONB,
                unlocked_schematics JSONB,
                gamemode INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create villages table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS villages (
                id TEXT PRIMARY KEY,
                name TEXT,
                culture TEXT,
                reputation INTEGER,
                resources JSONB,
                x REAL, y REAL, z REAL
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create villagers table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS villagers (
                id TEXT PRIMARY KEY,
                village_id TEXT,
                role TEXT,
                current_goal TEXT,
                x REAL, y REAL, z REAL,
                inventory JSONB,
                FOREIGN KEY(village_id) REFERENCES villages(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create world_settings table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS world_settings (
                key TEXT PRIMARY KEY,
                value TEXT
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create chunks table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                cx INTEGER,
                cy INTEGER,
                cz INTEGER,
                data BYTEA,
                PRIMARY KEY (cx, cy, cz)
            )
            "#,
        )
        .execute(pool)
        .await?;

        // Create block_entities table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS block_entities (
                cx INTEGER,
                cy INTEGER,
                cz INTEGER,
                lx INTEGER,
                ly INTEGER,
                lz INTEGER,
                type TEXT,
                data JSONB,
                PRIMARY KEY (cx, cy, cz, lx, ly, lz)
            )
            "#,
        )
        .execute(pool)
        .await?;

        println!("PostgreSQL schema initialized.");
        Ok(())
    }

    pub fn get(&self) -> Result<&PgPool, String> {
        self.pool
            .as_ref()
            .ok_or_else(|| "Database not connected. Call connect() first.".to_string())
    }

    pub async fn close(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
            println!("PostgreSQL connection pool closed.");
        }
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}
