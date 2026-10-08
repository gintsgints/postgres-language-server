use serde::{Deserialize, Serialize};
#[cfg(feature = "db")]
use sqlx::postgres::PgPool;

use crate::columns::Column;
use crate::functions::Function;
use crate::indexes::Index;
use crate::policies::Policy;
use crate::schemas::Schema;
use crate::sequences::Sequence;
use crate::tables::Table;
use crate::types::PostgresType;
use crate::versions::Version;
use crate::{Extension, Role, Trigger};

#[derive(Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SchemaCache {
    pub schemas: Vec<Schema>,
    pub tables: Vec<Table>,
    pub functions: Vec<Function>,
    pub types: Vec<PostgresType>,
    pub version: Version,
    pub columns: Vec<Column>,
    pub policies: Vec<Policy>,
    pub extensions: Vec<Extension>,
    pub triggers: Vec<Trigger>,
    pub roles: Vec<Role>,
    pub indexes: Vec<Index>,
    pub sequences: Vec<Sequence>,
}

/// Loads a non-essential part of the schema cache.
///
/// A failure is logged and yields an empty list, so that a single bad catalog
/// row cannot discard the entire cache - which would disable all
/// database-backed features, not just the one that failed to load.
#[cfg(feature = "db")]
macro_rules! load_lenient {
    ($item:ty, $pool:expr) => {
        async {
            match <$item>::load($pool).await {
                Ok(items) => items,
                Err(err) => {
                    tracing::warn!(
                        "Failed to load {} into the schema cache: {}",
                        stringify!($item),
                        err
                    );
                    Vec::new()
                }
            }
        }
    };
}

impl SchemaCache {
    #[cfg(feature = "db")]
    pub async fn load(pool: &PgPool) -> Result<SchemaCache, sqlx::Error> {
        let (
            schemas,
            tables,
            columns,
            versions,
            functions,
            types,
            policies,
            triggers,
            roles,
            extensions,
            indexes,
            sequences,
        ) = futures_util::join!(
            Schema::load(pool),
            Table::load(pool),
            Column::load(pool),
            Version::load(pool),
            load_lenient!(Function, pool),
            load_lenient!(PostgresType, pool),
            load_lenient!(Policy, pool),
            load_lenient!(Trigger, pool),
            load_lenient!(Role, pool),
            load_lenient!(Extension, pool),
            load_lenient!(Index, pool),
            load_lenient!(Sequence, pool),
        );

        // schemas, tables and columns are what the core features are built on,
        // so a failure there is still a hard error.
        let schemas = schemas?;
        let tables = tables?;
        let columns = columns?;

        let version = versions?.into_iter().next().unwrap_or_default();

        Ok(SchemaCache {
            schemas,
            tables,
            functions,
            types,
            version,
            columns,
            policies,
            triggers,
            roles,
            extensions,
            indexes,
            sequences,
        })
    }

    pub fn find_schema(&self, name: &str) -> Option<&Schema> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.schemas.iter().find(|s| s.name == sanitized_name)
    }

    pub fn find_tables(&self, name: &str, schema: Option<&str>) -> Vec<&Table> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.tables
            .iter()
            .filter(|t| {
                t.name == sanitized_name
                    && schema
                        .map(Self::sanitize_identifier)
                        .as_deref()
                        .is_none_or(|s| s == t.schema.as_str())
            })
            .collect()
    }

    pub fn find_type(&self, name: &str, schema: Option<&str>) -> Option<&PostgresType> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.types.iter().find(|t| {
            t.name == sanitized_name
                && schema
                    .map(Self::sanitize_identifier)
                    .as_deref()
                    .is_none_or(|s| s == t.schema.as_str())
        })
    }

    pub fn find_type_by_id(&self, id: i64) -> Option<&PostgresType> {
        self.types.iter().find(|t| t.id == id)
    }

    pub fn find_table_by_id(&self, id: i64) -> Option<&Table> {
        self.tables.iter().find(|t| t.id == id)
    }

    pub fn find_function_by_id(&self, id: i64) -> Option<&Function> {
        self.functions.iter().find(|f| f.id == id)
    }

    pub fn find_schema_by_id(&self, id: i64) -> Option<&Schema> {
        self.schemas.iter().find(|s| s.id == id)
    }

    pub fn find_index_by_id(&self, id: i64) -> Option<&Index> {
        self.indexes.iter().find(|i| i.id == id)
    }

    pub fn find_sequence_by_id(&self, id: i64) -> Option<&Sequence> {
        self.sequences.iter().find(|s| s.id == id)
    }

    pub fn find_cols(&self, name: &str, table: Option<&str>, schema: Option<&str>) -> Vec<&Column> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.columns
            .iter()
            .filter(|c| {
                c.name.as_str() == sanitized_name
                    && table
                        .map(Self::sanitize_identifier)
                        .as_deref()
                        .is_none_or(|t| t == c.table_name.as_str())
                    && schema
                        .map(Self::sanitize_identifier)
                        .as_deref()
                        .is_none_or(|s| s == c.schema_name.as_str())
            })
            .collect()
    }

    pub fn find_types(&self, name: &str, schema: Option<&str>) -> Vec<&PostgresType> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.types
            .iter()
            .filter(|t| {
                t.name == sanitized_name
                    && schema
                        .map(Self::sanitize_identifier)
                        .as_deref()
                        .is_none_or(|s| s == t.schema.as_str())
            })
            .collect()
    }

    pub fn find_functions(&self, name: &str, schema: Option<&str>) -> Vec<&Function> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.functions
            .iter()
            .filter(|f| {
                f.name == sanitized_name
                    && schema
                        .map(Self::sanitize_identifier)
                        .as_deref()
                        .is_none_or(|s| s == f.schema.as_str())
            })
            .collect()
    }

    pub fn find_roles(&self, name: &str) -> Vec<&Role> {
        let sanitized_name = Self::sanitize_identifier(name);
        self.roles
            .iter()
            .filter(|r| r.name == sanitized_name)
            .collect()
    }

    fn sanitize_identifier(identifier: &str) -> String {
        identifier.replace('"', "")
    }
}

#[cfg(feature = "db")]
pub trait SchemaCacheItem {
    type Item;

    async fn load(pool: &PgPool) -> Result<Vec<Self::Item>, sqlx::Error>;
}

#[cfg(all(test, feature = "db"))]
mod tests {
    use std::collections::HashSet;

    use sqlx::{Executor, PgPool};

    use crate::SchemaCache;

    #[sqlx::test(migrator = "pgls_test_utils::MIGRATIONS")]
    async fn it_loads(test_db: PgPool) {
        SchemaCache::load(&test_db)
            .await
            .expect("Couldnt' load Schema Cache");
    }

    #[sqlx::test(migrator = "pgls_test_utils::MIGRATIONS")]
    async fn it_loads_with_an_orphaned_catalog_row(test_db: PgPool) {
        // a `pg_proc` row whose namespace does not exist anymore used to make the
        // `functions` query return a null schema, which failed to decode and
        // discarded the entire cache - including tables and columns.
        let setup = r#"
        CREATE TABLE public.users (id uuid PRIMARY KEY);

        CREATE FUNCTION public.orphaned() RETURNS int LANGUAGE sql AS 'select 1';
        CREATE FUNCTION public.healthy() RETURNS int LANGUAGE sql AS 'select 1';

        UPDATE pg_catalog.pg_proc
        SET pronamespace = 2147483647
        WHERE proname = 'orphaned';
        "#;

        test_db.execute(setup).await.unwrap();

        let cache = SchemaCache::load(&test_db)
            .await
            .expect("Couldn't load Schema Cache");

        assert!(cache.tables.iter().any(|t| t.name == "users"));
        // the orphaned row is skipped, but the other functions still load
        assert!(cache.functions.iter().any(|f| f.name == "healthy"));
        assert!(!cache.functions.iter().any(|f| f.name == "orphaned"));
    }

    #[sqlx::test(migrator = "pgls_test_utils::MIGRATIONS")]
    async fn it_does_not_have_duplicate_entries(test_db: PgPool) {
        // we had some duplicate columns in the schema_cache because of indices including the same column multiple times.
        // the columns were unnested as duplicates in the query
        let setup = r#"
        CREATE TABLE public.mfa_factors (
            id uuid PRIMARY KEY,
            factor_name text NOT NULL
        );

        -- a second index on id!
        CREATE INDEX idx_mfa_user_factor ON public.mfa_factors(id, factor_name);
        "#;

        test_db.execute(setup).await.unwrap();

        let cache = SchemaCache::load(&test_db)
            .await
            .expect("Couldn't load Schema Cache");

        let set: HashSet<String> = cache
            .columns
            .iter()
            .map(|c| format!("{}.{}.{}", c.schema_name, c.table_name, c.name))
            .collect();

        assert_eq!(set.len(), cache.columns.len());
    }
}
