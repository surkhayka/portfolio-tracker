use crate::ledger::{self, Account, Adjustment, CsvRow, Event, Kind, Record, HEADER};
use chrono::Utc;
use rusqlite::{backup::Backup, params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};

pub type Result<T> = std::result::Result<T, String>;
pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
type Records = BTreeMap<String, Record>;
const VERSION: i64 = 1;

pub fn open(path: &Path) -> Result<Connection> {
    let mut db = Connection::open(path).map_err(err)?;
    db.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA busy_timeout=5000;",
    )
    .map_err(err)?;
    let version = version(&db)?;
    if version > VERSION {
        return Err("This database requires a newer version of Portfolio.".into());
    }
    if version < VERSION {
        let tables: i64 = db
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        if version != 0 || tables > 0 {
            backup_file(
                &db,
                &path.with_file_name(format!(
                    "pre-migration-{}.sqlite",
                    Utc::now().timestamp_millis()
                )),
            )?;
        }
        let tx = db.transaction().map_err(err)?;
        tx.execute_batch("
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO meta VALUES ('revision','0'),('timezone','system');
            CREATE TABLE ledger (id TEXT PRIMARY KEY, record TEXT NOT NULL);
            CREATE TABLE batches (id INTEGER PRIMARY KEY, filename TEXT NOT NULL, time TEXT NOT NULL, added INTEGER NOT NULL, replaced INTEGER NOT NULL, skipped INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE changes (revision INTEGER NOT NULL, id TEXT NOT NULL, before_state TEXT, after_state TEXT, operation TEXT NOT NULL, batch_id INTEGER REFERENCES batches(id), PRIMARY KEY(revision,id));
            CREATE TABLE prices (symbol TEXT PRIMARY KEY, quote TEXT NOT NULL);
            CREATE TABLE overrides (symbol TEXT PRIMARY KEY, quote TEXT NOT NULL);
            PRAGMA application_id=1347572815;
            PRAGMA user_version=1;
        ").map_err(err)?;
        tx.commit().map_err(err)?;
    }
    Ok(db)
}

fn version(db: &Connection) -> Result<i64> {
    db.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(err)
}

pub fn setting(db: &Connection, key: &str) -> Result<String> {
    db.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .map_err(err)
}

pub fn save_setting(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute("UPDATE meta SET value=?2 WHERE key=?1", params![key, value])
        .map(|_| ())
        .map_err(err)
}

pub fn revision(db: &Connection) -> Result<i64> {
    setting(db, "revision")?.parse().map_err(err)
}

pub fn load(db: &Connection) -> Result<Records> {
    let mut query = db.prepare("SELECT record FROM ledger").map_err(err)?;
    let rows = query
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(err)?;
    rows.map(|row| {
        let mut record: Record = serde_json::from_str(&row.map_err(err)?).map_err(err)?;
        record.event = record.event.validate()?;
        if let Some(q) = &record.reconciled_quantity {
            let value = q.parse::<rust_decimal::Decimal>().map_err(err)?;
            if value <= rust_decimal::Decimal::ZERO {
                return Err("Invalid reconciled quantity.".into());
            }
        }
        Ok((record.event.transaction_id.clone(), record))
    })
    .collect()
}

#[derive(Serialize)]
pub struct Batch {
    id: i64,
    filename: String,
    time: String,
    added: i64,
    replaced: i64,
    skipped: i64,
    undone: bool,
}

pub fn batches(db: &Connection) -> Result<Vec<Batch>> {
    let mut statement = db
        .prepare(
            "SELECT id,filename,time,added,replaced,skipped,undone FROM batches ORDER BY id DESC",
        )
        .map_err(err)?;
    let rows = statement
        .query_map([], |r| {
            Ok(Batch {
                id: r.get(0)?,
                filename: r.get(1)?,
                time: r.get(2)?,
                added: r.get(3)?,
                replaced: r.get(4)?,
                skipped: r.get(5)?,
                undone: r.get(6)?,
            })
        })
        .map_err(err)?;
    rows.collect::<rusqlite::Result<_>>().map_err(err)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Save {
        event: Event,
        editing: bool,
        opening_average: Option<String>,
    },
    Exclude {
        id: String,
        excluded: bool,
    },
    Import {
        filename: String,
        contents: String,
        choices: BTreeMap<u64, Choice>,
        replace_opening: BTreeSet<String>,
    },
    Undo {
        batch: i64,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Skip,
    Include,
    Replace,
}

#[derive(Serialize)]
pub struct ImportRow {
    #[serde(flatten)]
    row: CsvRow,
    status: &'static str,
    message: Option<String>,
}

#[derive(Serialize)]
pub struct Impact {
    symbol: String,
    before: Account,
    after: Account,
}

#[derive(Serialize)]
pub struct Preview {
    pub revision: i64,
    rows: Vec<ImportRow>,
    pub conflicts: Vec<String>,
    warnings: Vec<String>,
    pub adjustments: Vec<Adjustment>,
    impacts: Vec<Impact>,
    pub changed: usize,
    opening_conflicts: BTreeSet<String>,
}

struct Plan {
    preview: Preview,
    before: Records,
    after: Records,
    counts: (usize, usize, usize),
}

fn preview_plan(db: &Connection, action: &Action, confirmed: &BTreeSet<String>) -> Result<Plan> {
    let revision = revision(db)?;
    let before = load(db)?;
    let mut after = before.clone();
    let mut preview = Preview {
        revision,
        rows: vec![],
        conflicts: vec![],
        warnings: vec![],
        adjustments: vec![],
        impacts: vec![],
        changed: 0,
        opening_conflicts: BTreeSet::new(),
    };
    let mut counts = (0, 0, 0);
    let mut seq = before
        .values()
        .map(|r| r.sequence)
        .max()
        .unwrap_or_default();
    let mut put = |events: &mut Records, event: Event| {
        let (sequence, reconciled_quantity) = match events.get(&event.transaction_id) {
            Some(prior) => (
                prior.sequence,
                prior
                    .event
                    .equivalent(&event)
                    .then(|| prior.reconciled_quantity.clone())
                    .flatten(),
            ),
            None => {
                seq += 1;
                (seq, None)
            }
        };
        events.insert(
            event.transaction_id.clone(),
            Record {
                event,
                sequence,
                excluded: false,
                reconciled_quantity,
                revision: revision + 1,
            },
        );
    };
    match action {
        Action::Save {
            event,
            editing,
            opening_average,
        } => {
            let mut event = event.clone();
            if let Some(average) = opening_average {
                if event.kind != Kind::OpeningBalance || event.amount.is_some() {
                    return Err("Enter either the total basis or average cost.".into());
                }
                event.amount = Some(opening_counterpart(
                    event.quantity.as_deref().unwrap_or_default(),
                    average,
                    true,
                )?);
            }
            let event = event.validate()?;
            let prior = before.get(&event.transaction_id);
            if *editing != prior.is_some() {
                return Err("The transaction changed. Reload before editing.".into());
            }
            if prior.is_some_and(|row| row.excluded) {
                return Err("Include the transaction before editing.".into());
            }
            put(&mut after, event);
        }
        Action::Exclude { id, excluded } => {
            let entry = after.get_mut(id).ok_or("Transaction was not found.")?;
            entry.excluded = *excluded;
            entry.revision = revision + 1;
        }
        Action::Import {
            contents,
            choices,
            replace_opening,
            ..
        } => {
            if contents.len() > 8 * 1024 * 1024 {
                return Err("CSV exceeds 8 MB.".into());
            }
            let mut rows = ledger::parse_csv(contents)?;
            // Preserve order within identical timestamps, independent of chronological sorting.
            rows.sort_by(|a, b| {
                a.event
                    .as_ref()
                    .map(|e| &e.timestamp)
                    .cmp(&b.event.as_ref().map(|e| &e.timestamp))
            });
            let mut batch_ids = BTreeSet::new();
            for row in rows {
                let choice = choices.get(&row.row);
                let mut message = None;
                let status = if choice == Some(&Choice::Skip) {
                    "skipped"
                } else if let Some(event) = &row.event {
                    let id = &event.transaction_id;
                    let prior = after.get(id);
                    let opening = after.values().find(|r| {
                        !r.excluded
                            && r.event.kind == Kind::OpeningBalance
                            && r.event.symbol == event.symbol
                            && r.event.transaction_id != *id
                    });
                    if prior.is_some_and(|r| r.excluded) {
                        "excluded"
                    } else if prior.is_some_and(|r| r.event.equivalent(event)) {
                        "duplicate"
                    } else if !batch_ids.insert(id.clone()) {
                        message = Some(
                            "This ID has different values in two selected rows. Skip one row."
                                .into(),
                        );
                        "blocked"
                    } else {
                        let overlapping = opening.filter(|r| {
                            event.timestamp <= r.event.timestamp
                                || event.kind == Kind::OpeningBalance
                        });
                        let overlapping_symbol = overlapping.map(|r| r.event.symbol.clone());
                        let replace_id = overlapping
                            .filter(|r| {
                                replace_opening.contains(&r.event.symbol)
                                    && event.kind == Kind::Buy
                                    && before.contains_key(&r.event.transaction_id)
                            })
                            .map(|r| r.event.transaction_id.clone());
                        if let Some(symbol) = overlapping_symbol.filter(|_| replace_id.is_none()) {
                            preview.opening_conflicts.insert(symbol);
                            message = Some("This row overlaps an opening balance. Skip the earlier rows to keep the balance, or replace it with complete purchase history.".into());
                            "blocked"
                        } else if prior.is_some() && choice != Some(&Choice::Replace) {
                            message = Some("The saved transaction with this ID differs. Choose which version to keep.".into());
                            "conflict"
                        } else if prior.is_none()
                            && choice != Some(&Choice::Include)
                            && after
                                .values()
                                .any(|r| !r.excluded && r.event.economic_match(event))
                        {
                            message = Some("A different ID has the same trade details. Confirm this is a separate transaction.".into());
                            "possible_duplicate"
                        } else {
                            let replacing = prior.is_some();
                            if let Some(opening_id) = replace_id {
                                let record = after.get_mut(&opening_id).unwrap();
                                record.excluded = true;
                                record.revision = revision + 1;
                                counts.1 += 1;
                            }
                            put(&mut after, event.clone());
                            if replacing {
                                counts.1 += 1;
                                "replace"
                            } else {
                                counts.0 += 1;
                                "new"
                            }
                        }
                    }
                } else {
                    message = row.error.clone();
                    "blocked"
                };
                if matches!(status, "blocked" | "conflict" | "possible_duplicate") {
                    preview.conflicts.push(format!(
                        "CSV row {}: {}",
                        row.row,
                        message.as_deref().unwrap_or("Needs review.")
                    ));
                }
                if matches!(status, "skipped" | "excluded" | "duplicate") {
                    counts.2 += 1;
                }
                preview.rows.push(ImportRow {
                    row,
                    status,
                    message,
                });
            }
            preview.rows.sort_by_key(|r| r.row.row);
        }
        Action::Undo { batch } => {
            let active = db
                .query_row("SELECT undone=0 FROM batches WHERE id=?1", [batch], |r| {
                    r.get::<_, bool>(0)
                })
                .optional()
                .map_err(err)?
                .unwrap_or(false);
            if !active {
                return Err("This batch is unavailable or already undone.".into());
            }
            let mut statement = db
                .prepare("SELECT id,before_state,after_state FROM changes WHERE batch_id=?1")
                .map_err(err)?;
            let changes = statement
                .query_map([batch], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                })
                .map_err(err)?;
            for row in changes {
                let (id, old, imported) = row.map_err(err)?;
                let imported: Option<Record> = imported
                    .map(|s| serde_json::from_str(&s))
                    .transpose()
                    .map_err(err)?;
                if before.get(&id) != imported.as_ref() {
                    preview.conflicts.push(format!("{id} was changed after this import. Review the transaction before undoing."));
                    continue;
                }
                match old {
                    Some(old) => {
                        let mut record: Record = serde_json::from_str(&old).map_err(err)?;
                        record.revision = revision + 1;
                        after.insert(id, record);
                    }
                    None => {
                        after.remove(&id);
                    }
                }
            }
        }
    }
    let old = ledger::replay(before.values())?;
    match ledger::replay(after.values()) {
        Ok(mut new) => {
            preview.warnings = new
                .warnings
                .into_iter()
                .filter(|w| !old.warnings.contains(w))
                .collect();
            for adj in &mut new.adjustments {
                if confirmed.contains(&adj.id) && !adj.confirmed {
                    let record = after.get_mut(&adj.id).unwrap();
                    record.reconciled_quantity = Some(adj.effective.to_string());
                    record.revision = revision + 1;
                    adj.confirmed = true;
                }
            }
            preview.adjustments = new
                .adjustments
                .into_iter()
                .filter(|a| {
                    !old.adjustments.iter().any(|prior| {
                        prior.id == a.id
                            && prior.original == a.original
                            && prior.effective == a.effective
                            && prior.confirmed
                            && a.confirmed
                    })
                })
                .collect();
            let symbols: BTreeSet<_> = old.accounts.keys().chain(new.accounts.keys()).collect();
            for symbol in symbols {
                let previous = old.accounts.get(symbol).cloned().unwrap_or_default();
                let next = new.accounts.get(symbol).cloned().unwrap_or_default();
                if previous != next {
                    preview.impacts.push(Impact {
                        symbol: symbol.clone(),
                        before: previous,
                        after: next,
                    });
                }
            }
        }
        Err(error) => preview.conflicts.push(error),
    }
    preview.changed = changed_ids(&before, &after).len();
    Ok(Plan {
        preview,
        before,
        after,
        counts,
    })
}

fn changed_ids<'a>(before: &'a Records, after: &'a Records) -> BTreeSet<&'a String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|id| before.get(*id) != after.get(*id))
        .collect()
}

pub fn preview(db: &Connection, action: &Action) -> Result<Preview> {
    Ok(preview_plan(db, action, &BTreeSet::new())?.preview)
}

pub fn commit(
    db: &mut Connection,
    action: &Action,
    expected: i64,
    confirm: &BTreeSet<String>,
) -> Result<()> {
    let tx = db.transaction().map_err(err)?;
    if revision(&tx)? != expected {
        return Err("The portfolio changed. Run the preview again before saving.".into());
    }
    let plan = preview_plan(&tx, action, confirm)?;
    if !plan.preview.conflicts.is_empty() {
        return Err(plan.preview.conflicts.join("\n"));
    }
    if plan.preview.adjustments.iter().any(|a| !a.confirmed) {
        return Err("Confirm all quantity reconciliations before saving.".into());
    }
    if plan.preview.changed == 0 && !matches!(action, Action::Undo { .. }) {
        return Ok(());
    }
    let revision = expected + 1;
    let (batch, operation) = match action {
        Action::Import { filename, .. } => {
            tx.execute(
                "INSERT INTO batches(filename,time,added,replaced,skipped) VALUES(?1,?2,?3,?4,?5)",
                params![
                    filename,
                    Utc::now().to_rfc3339(),
                    plan.counts.0,
                    plan.counts.1,
                    plan.counts.2
                ],
            )
            .map_err(err)?;
            (Some(tx.last_insert_rowid()), "import")
        }
        Action::Undo { batch } => {
            tx.execute("UPDATE batches SET undone=1 WHERE id=?1", [batch])
                .map_err(err)?;
            (None, "undo")
        }
        Action::Save { .. } => (None, "save"),
        Action::Exclude { .. } => (None, "exclusion"),
    };
    for id in changed_ids(&plan.before, &plan.after) {
        let old = plan
            .before
            .get(id)
            .map(serde_json::to_string)
            .transpose()
            .map_err(err)?;
        let new = plan
            .after
            .get(id)
            .map(serde_json::to_string)
            .transpose()
            .map_err(err)?;
        match &new {
            Some(record) => {
                tx.execute("INSERT INTO ledger(id,record) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET record=excluded.record", params![id, record]).map_err(err)?;
            }
            None => {
                tx.execute("DELETE FROM ledger WHERE id=?1", [id])
                    .map_err(err)?;
            }
        }
        tx.execute(
            "INSERT INTO changes VALUES(?1,?2,?3,?4,?5,?6)",
            params![revision, id, old, new, operation, batch],
        )
        .map_err(err)?;
    }
    save_setting(&tx, "revision", &revision.to_string())?;
    tx.commit().map_err(err)
}

pub fn backup_file(db: &Connection, path: &Path) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut destination = Connection::open(&tmp).map_err(err)?;
    let result = copy(db, &mut destination);
    drop(destination);
    let result = result.and_then(|_| replace_file(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    if destination.exists() {
        let displaced = destination.with_extension(format!("{}.old", uuid::Uuid::new_v4()));
        std::fs::rename(destination, &displaced).map_err(err)?;
        if let Err(error) = std::fs::rename(source, destination) {
            let _ = std::fs::rename(displaced, destination);
            return Err(err(error));
        }
        let _ = std::fs::remove_file(displaced);
        return Ok(());
    }
    std::fs::rename(source, destination).map_err(err)
}

fn copy(from: &Connection, to: &mut Connection) -> Result<()> {
    Backup::new(from, to)
        .map_err(err)?
        .run_to_completion(128, Duration::from_millis(10), None)
        .map_err(err)
}

pub fn restore(db: &mut Connection, path: &Path, safety_path: &Path) -> Result<()> {
    let source =
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(err)?;
    let mut staged = Connection::open_in_memory().map_err(err)?;
    copy(&source, &mut staged)?;
    let app_id: i64 = staged
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(err)?;
    if app_id != 1347572815 || version(&staged)? != VERSION {
        return Err("Select a compatible Portfolio SQLite backup.".into());
    }
    let result: String = staged
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(err)?;
    if result != "ok" {
        return Err("This backup failed SQLite integrity checks.".into());
    }
    let invalid_link = staged
        .prepare("PRAGMA foreign_key_check")
        .map_err(err)?
        .exists([])
        .map_err(err)?;
    if invalid_link {
        return Err("This backup contains invalid record links.".into());
    }
    let replay = ledger::replay(load(&staged)?.values())?;
    if replay.adjustments.iter().any(|a| !a.confirmed) {
        return Err("Backup has unresolved quantity adjustments.".into());
    }
    batches(&staged)?;
    crate::market::validate_cache(&staged)?;
    let timezone = setting(&staged, "timezone")?;
    if timezone != "system" && timezone.parse::<chrono_tz::Tz>().is_err() {
        return Err("Backup contains an invalid timezone.".into());
    }
    let next = revision(db)?.max(revision(&staged)?) + 1;
    save_setting(&staged, "revision", &next.to_string())?;
    backup_file(db, safety_path)?;
    copy(&staged, db)
}

pub fn export(db: &Connection, path: &Path) -> Result<()> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(vec![]);
    writer.write_record(HEADER.split(',')).map_err(err)?;
    let mut records: Vec<_> = load(db)?.into_values().filter(|r| !r.excluded).collect();
    records.sort_by(|a, b| (&a.event.timestamp, a.sequence).cmp(&(&b.event.timestamp, b.sequence)));
    for record in records {
        writer.serialize(record.event).map_err(err)?;
    }
    write_file(path, &writer.into_inner().map_err(err)?)
}

pub fn opening_counterpart(quantity: &str, cost: &str, average: bool) -> Result<String> {
    use rust_decimal::Decimal as D;
    let quantity = ledger::decimal(quantity)?;
    let cost = ledger::decimal(cost)?;
    if quantity <= D::ZERO || cost < D::ZERO {
        return Err("Enter a positive quantity and nonnegative cost.".into());
    }
    let result = if average {
        quantity.checked_mul(cost)
    } else {
        cost.checked_div(quantity)
    }
    .ok_or("The cost exceeds supported precision.")?
    .round_dp(12)
    .normalize()
    .to_string();
    ledger::decimal(&result)?;
    Ok(result)
}

pub fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = std::fs::write(&tmp, contents)
        .map_err(err)
        .and_then(|_| replace_file(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
