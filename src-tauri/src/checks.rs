use crate::{
    ledger::{self, Kind},
    market::{self, Quote},
    storage::{self, Action, Choice},
};
use rusqlite::Connection;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("portfolio-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn open(&self) -> Connection {
        storage::open(&self.0.join("portfolio.sqlite")).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const EXAMPLE: &str = include_str!("../../public/portfolio-example.csv");

fn import(csv: impl Into<String>) -> Action {
    Action::Import {
        filename: "synthetic.csv".into(),
        contents: csv.into(),
        choices: BTreeMap::new(),
        replace_opening: BTreeSet::new(),
    }
}

fn csv(rows: &str) -> String {
    format!("{}\n{rows}\n", ledger::HEADER)
}

fn save(db: &mut Connection, action: &Action) {
    let preview = storage::preview(db, action).unwrap();
    assert!(preview.conflicts.is_empty(), "{:?}", preview.conflicts);
    storage::commit(db, action, preview.revision, &BTreeSet::new()).unwrap();
}

fn portfolio(db: &Connection) -> serde_json::Value {
    serde_json::to_value(market::portfolio(db).unwrap()).unwrap()
}

#[test]
fn weighted_average_sale_fees_and_full_close_then_reopen() {
    let temp = Temp::new();
    let mut db = temp.open();
    save(&mut db, &import(EXAMPLE));
    let records = storage::load(&db).unwrap();
    let replay = ledger::replay(records.values()).unwrap();
    let aapl = &replay.accounts["AAPL"];
    assert_eq!(aapl.realized, ledger::num("100"));
    assert_eq!(aapl.quantity, ledger::num("15"));
    assert_eq!(aapl.basis, ledger::num("1650"));
    assert_eq!(aapl.dividends, ledger::num("2.50"));
    assert!(replay.accounts["MSFT"].incomplete);
    assert_eq!(replay.accounts["GOOGL"].quantity, ledger::num("0"));
    save(
        &mut db,
        &import(csv(concat!(
            "sale,2025-01-14T15:30:00Z,SELL,AAPL,USD,15,120,,2,true,,999,\n",
            "reopen,2025-01-15T15:30:00Z,BUY,AAPL,USD,0.5,90,,1,,,,\n",
            "split,2025-01-16T15:30:00Z,SPLIT,AAPL,USD,,,,,,2,,"
        ))),
    );
    let records = storage::load(&db).unwrap();
    let p = ledger::replay(records.values()).unwrap();
    assert_eq!(p.accounts["AAPL"].quantity, ledger::num("1"));
    assert_eq!(p.accounts["AAPL"].basis, ledger::num("46"));
    assert_eq!(p.accounts["AAPL"].realized, ledger::num("248"));
}

#[test]
fn csv_reimport_reordering_and_id_conflicts() {
    let temp = Temp::new();
    let mut db = temp.open();
    save(&mut db, &import(EXAMPLE));
    let reordered = EXAMPLE
        .split_once('\n')
        .unwrap()
        .1
        .lines()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let preview = storage::preview(&db, &import(csv(&reordered))).unwrap();
    assert_eq!(preview.changed, 0);
    assert!(preview.conflicts.is_empty());
    let changed =
        csv("example-buy-1,2025-01-06T15:30:00Z,BUY,AAPL,USD,10,101,,,,,,Synthetic example");
    let mut action = import(changed);
    assert!(!storage::preview(&db, &action).unwrap().conflicts.is_empty());
    if let Action::Import { choices, .. } = &mut action {
        choices.insert(2, Choice::Replace);
    }
    save(&mut db, &action);
    assert_eq!(storage::load(&db).unwrap().len(), 7);
    let maybe = import(csv(
        "different,2025-01-06T15:30:00Z,BUY,AAPL,USD,10,101,,,,,,",
    ));
    assert!(storage::preview(&db, &maybe).unwrap().conflicts[0].contains("different ID"));
    assert_eq!(
        storage::load(&db).unwrap()["example-buy-1"]
            .event
            .unit_price
            .as_deref(),
        Some("101")
    );
}

#[test]
fn excludes_survive_reimport_and_restart_and_undo_handles_conflicts() {
    let temp = Temp::new();
    let mut db = temp.open();
    save(&mut db, &import(EXAMPLE));
    let invalid = Action::Exclude {
        id: "example-buy-3".into(),
        excluded: true,
    };
    assert!(storage::preview(&db, &invalid).unwrap().conflicts[0].contains("closing quantity"));
    save(
        &mut db,
        &Action::Exclude {
            id: "example-buy-1".into(),
            excluded: true,
        },
    );
    drop(db);
    let mut db = temp.open();
    assert!(storage::load(&db).unwrap()["example-buy-1"].excluded);
    assert_eq!(storage::preview(&db, &import(EXAMPLE)).unwrap().changed, 0);
    assert!(storage::preview(&db, &Action::Undo { batch: 1 })
        .unwrap()
        .conflicts[0]
        .contains("changed after"));
    let separate = import(csv("one,2025-01-15T15:30:00Z,BUY,AMD,USD,1,50,,,,,,"));
    save(&mut db, &separate);
    save(&mut db, &Action::Undo { batch: 2 });
    assert!(!storage::load(&db).unwrap().contains_key("one"));
    assert!(storage::preview(&db, &Action::Undo { batch: 2 }).is_err());
}

#[test]
fn stale_preview_and_opening_overlap() {
    let temp = Temp::new();
    let mut db = temp.open();
    let opening = import(csv(
        "opening,2025-01-07T15:30:00Z,OPENING_BALANCE,AMD,USD,1,,60,,,,,",
    ));
    let history = import(csv("buy,2025-01-06T15:30:00Z,BUY,AMD,USD,1,60,,,,,,"));
    let early = storage::preview(&db, &history).unwrap();
    save(&mut db, &opening);
    assert!(
        storage::commit(&mut db, &history, early.revision, &BTreeSet::new())
            .unwrap_err()
            .contains("portfolio changed")
    );
    let mut history = history;
    assert!(!storage::preview(&db, &history)
        .unwrap()
        .conflicts
        .is_empty());
    if let Action::Import {
        replace_opening, ..
    } = &mut history
    {
        replace_opening.insert("AMD".into());
    }
    save(&mut db, &history);
    let records = storage::load(&db).unwrap();
    let p = ledger::replay(records.values()).unwrap();
    assert!(records["opening"].excluded);
    assert!(!p.accounts["AMD"].incomplete);
    assert_eq!(p.accounts["AMD"].quantity, ledger::num("1"));
    assert_eq!(
        storage::opening_counterpart("2.5", "160", true).unwrap(),
        "400"
    );
    assert_eq!(
        storage::opening_counterpart("2.5", "400", false).unwrap(),
        "160"
    );
}

#[test]
fn sale_uses_execution_price_and_requires_close_rounding_confirmation() {
    let temp = Temp::new();
    let mut db = temp.open();
    let action = import(csv(concat!(
        "buy1,2025-01-06T15:30:00Z,BUY,ACME,USD,1.2501,100,125.01,,,,,\n",
        "buy2,2025-01-06T17:30:00Z,BUY,ACME,USD,0.5000,120,60,,,,,\n",
        "sell,2025-01-07T15:30:00Z,SELL,ACME,USD,1.7500,146,,,true,,77.20,"
    )));
    let preview = storage::preview(&db, &action).unwrap();
    assert_eq!(preview.adjustments.len(), 1);
    assert!(storage::commit(&mut db, &action, preview.revision, &BTreeSet::new()).is_err());
    storage::commit(
        &mut db,
        &action,
        preview.revision,
        &BTreeSet::from(["sell".into()]),
    )
    .unwrap();
    let records = storage::load(&db).unwrap();
    assert_eq!(
        records["sell"].reconciled_quantity.as_deref(),
        Some("1.7501")
    );
    assert_eq!(
        ledger::replay(records.values()).unwrap().accounts["ACME"].realized,
        ledger::num("70.49")
    );
    let mut edit = records["sell"].event.clone();
    edit.quantity = Some("1.75".into());
    let action = Action::Save {
        event: edit,
        editing: true,
        opening_average: None,
    };
    assert!(storage::preview(&db, &action).unwrap().conflicts.is_empty());
    let mut bad = records["sell"].event.clone();
    bad.quantity = Some("1.74".into());
    let action = Action::Save {
        event: bad,
        editing: true,
        opening_average: None,
    };
    assert!(!storage::preview(&db, &action).unwrap().conflicts.is_empty());
}

#[test]
fn unsupported_rows_are_reported_and_may_be_skipped() {
    let temp = Temp::new();
    let mut db = temp.open();
    let mut action = import(csv(concat!(
        "ok,2025-01-06T15:00:00Z,BUY,AMD,USD,1,50,,,,,,\n",
        "eur,2025-01-06T15:30:00Z,BUY,ASML,EUR,1,100,,,,,,\n",
        "coin,2025-01-06T15:30:00Z,BUY,BTC/USD,USD,1,100,,,,,,"
    )));
    let preview = storage::preview(&db, &action).unwrap();
    assert_eq!(preview.conflicts.len(), 2);
    assert!(storage::commit(&mut db, &action, preview.revision, &BTreeSet::new()).is_err());
    assert!(storage::load(&db).unwrap().is_empty());
    if let Action::Import { choices, .. } = &mut action {
        choices.extend([(3, Choice::Skip), (4, Choice::Skip)]);
    }
    save(&mut db, &action);
    assert_eq!(storage::load(&db).unwrap().len(), 1);
}

#[test]
fn backup_restore_and_export_preserve_expected_records() {
    let temp = Temp::new();
    let mut db = temp.open();
    save(&mut db, &import(EXAMPLE));
    storage::save_setting(&db, "timezone", "Asia/Hong_Kong").unwrap();
    save(
        &mut db,
        &Action::Exclude {
            id: "example-buy-2".into(),
            excluded: true,
        },
    );
    let backup = temp.0.join("backup.sqlite");
    storage::backup_file(&db, &backup).unwrap();
    let before = portfolio(&db);
    save(
        &mut db,
        &Action::Exclude {
            id: "example-buy-2".into(),
            excluded: false,
        },
    );
    let safety = temp.0.join("safety.sqlite");
    storage::restore(&mut db, &backup, &safety).unwrap();
    assert!(safety.exists());
    let restored = portfolio(&db);
    assert!(restored["revision"].as_i64() > before["revision"].as_i64());
    for field in ["holdings", "summary", "transactions"] {
        assert_eq!(before[field], restored[field]);
    }
    assert_eq!(storage::setting(&db, "timezone").unwrap(), "Asia/Hong_Kong");
    let file = temp.0.join("export.csv");
    storage::export(&db, &file).unwrap();
    let exported = ledger::parse_csv(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert_eq!(exported.len(), 6);
    assert!(exported.iter().all(|r| r.error.is_none()));
    assert!(exported
        .iter()
        .any(|r| r.event.as_ref().unwrap().kind == Kind::Sell));
    assert!(!format!("{:?}", std::fs::read(&backup).unwrap()).contains("APCA-API-SECRET-KEY"));
}

#[test]
fn prices_missing_and_split_adjusted_baseline() {
    let temp = Temp::new();
    let mut db = temp.open();
    save(
        &mut db,
        &import(csv(concat!(
            "buy,2025-01-06T15:30:00Z,BUY,AMD,USD,10,100,,,,,,\n",
            "split,2025-01-08T14:30:00Z,SPLIT,AMD,USD,,,,,,2,,"
        ))),
    );
    assert_eq!(portfolio(&db)["summary"]["missing"][0], "AMD");
    let quote = Quote {
        price: ledger::num("55"),
        at: "2025-01-08T15:01:00.000000000Z".into(),
        reference: Some(ledger::num("106")),
        reference_at: Some("2025-01-07T21:00:00.000000000Z".into()),
        fetched_at: "2025-01-08T15:17:00.000000000Z".into(),
        manual: false,
    };
    market::save_quotes(&mut db, BTreeMap::from([("AMD".into(), quote)])).unwrap();
    let result = portfolio(&db);
    assert_eq!(result["summary"]["value"], "1100");
    assert_eq!(
        ledger::num(result["holdings"][0]["daily"].as_str().unwrap()),
        ledger::num("40")
    );
    market::set_override(&db, "AMD", Some("60"), Some("2025-01-08T16:00:00Z")).unwrap();
    let result = portfolio(&db);
    assert_eq!(result["summary"]["value"], "1200");
    assert!(result["holdings"][0]["daily"].is_null());
}

#[test]
fn refresh_only_writes_new_or_corrected_quotes() {
    let temp = Temp::new();
    let mut db = temp.open();
    let mut quote = Quote {
        price: ledger::num("55"),
        at: "2025-01-08T15:01:00.000000000Z".into(),
        reference: Some(ledger::num("50")),
        reference_at: Some("2025-01-07T21:00:00.000000000Z".into()),
        fetched_at: "2025-01-08T15:17:00.000000000Z".into(),
        manual: false,
    };
    let persist = |db: &mut Connection, q: &Quote| {
        market::save_quotes(db, BTreeMap::from([("AMD".into(), q.clone())])).unwrap()
    };
    assert!(persist(&mut db, &quote));
    quote.fetched_at = "2025-01-08T15:19:00.000000000Z".into();
    assert!(!persist(&mut db, &quote));
    quote.price = ledger::num("55.10");
    assert!(persist(&mut db, &quote));
    quote.at = "2025-01-08T15:00:00.000000000Z".into();
    assert!(!persist(&mut db, &quote));
}
