//! Owner-scoped single-file and type-ahead reads over every File table.
//!
//! Each call runs one query per File table through [`FileQueryAll`], so Valence's read
//! privacy still applies to every row, and each table can use its own indexes.

use crate::generated::{FileModel, FileQueryAll};
use std::collections::BTreeSet;
use valence::{DataUsePurpose, RecordId, RecordPredicate, StringPredicate, Valence};

/// Most names [`suggest_names`] returns.
pub const MAX_NAME_SUGGESTIONS: usize = 10;

/// Rows read per table while collecting distinct suggestions.
const SUGGEST_ROWS_PER_TABLE: u32 = 50;

/// Every table that implements File, from the trait registry.
pub fn file_tables() -> Vec<String> {
    valence::TraitRegistry::global()
        .tables_for_trait("File")
        .into_iter()
        .map(|t| t.to_string())
        .collect()
}

/// Owner-scoped File query over one table.
fn owned_table_query<'a>(
    base: &FileQueryAll<'a>,
    table: &str,
    owner: &RecordId,
) -> FileQueryAll<'a> {
    let (mut core, v) = base.clone().into_parts();
    core.table = table.to_string();
    FileQueryAll::from_parts(core, v).where_uploaded_by(RecordPredicate::Equals(owner.clone()))
}

/// Load one of `owner`'s files by its full id (`table:id`), reading only that id's table.
///
/// Returns `None` when the row is missing, belongs to someone else, is hidden by the
/// table's read policy, or the id's table isn't a File table. Callers should answer all of
/// those the same way, so a foreign id never reveals that it exists.
///
/// # Errors
///
/// Valence query failures.
pub async fn get_by_id(
    v: &Valence,
    purpose: DataUsePurpose,
    owner: &RecordId,
    id: &RecordId,
) -> valence::Result<Option<FileModel>> {
    if !file_tables().iter().any(|t| t == id.table()) {
        return Ok(None);
    }
    let base = FileQueryAll::query(v, purpose);
    let (core, v) = owned_table_query(&base, id.table(), owner).into_parts();
    let core = core.where_string(
        "id".to_string(),
        StringPredicate::Equals(id.id().to_string()),
    );
    FileQueryAll::from_parts(core, v).first().await
}

/// File names of `owner`'s files that start with `prefix`, for type-ahead.
///
/// Returns at most `limit` distinct names (capped at [`MAX_NAME_SUGGESTIONS`]), sorted.
/// An empty prefix returns nothing. Matching is case-insensitive for ASCII on SQLite and
/// case-sensitive on Postgres.
///
/// # Errors
///
/// Valence query failures.
pub async fn suggest_names(
    v: &Valence,
    purpose: DataUsePurpose,
    owner: &RecordId,
    prefix: &str,
    limit: usize,
) -> valence::Result<Vec<String>> {
    let limit = limit.min(MAX_NAME_SUGGESTIONS);
    if prefix.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let base = FileQueryAll::query(v, purpose);
    let mut names = BTreeSet::new();
    for table in file_tables() {
        let rows = owned_table_query(&base, &table, owner)
            .where_file_name(StringPredicate::StartsWith(prefix.to_string()))
            .order_by_file_name(valence::SortDirection::Asc)
            .limit(SUGGEST_ROWS_PER_TABLE)
            .await?;
        names.extend(rows.into_iter().map(|row| {
            use crate::generated::FileFields;
            row.file_name().clone()
        }));
    }
    Ok(names.into_iter().take(limit).collect())
}
