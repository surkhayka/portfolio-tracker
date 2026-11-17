use crate::{
    ledger,
    market::{self, Alpaca, Credentials, MarketProvider, Portfolio, Valuation},
    storage::{self, err, Action, Batch, Preview, Result},
};
use chrono::Utc;
use rusqlite::Connection;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

type Accounts = BTreeMap<String, ledger::Account>;

struct AppState {
    db: Mutex<Connection>,
    path: PathBuf,
    alpaca: Alpaca,
    network: tauri::async_runtime::Mutex<()>,
    credentials: Mutex<Option<Option<Credentials>>>,
    accounts: Mutex<Option<(i64, Accounts)>>,
}

impl AppState {
    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.db.lock().map_err(err)
    }

    fn credentials(&self) -> Result<Option<Credentials>> {
        let mut cached = self.credentials.lock().map_err(err)?;
        if let Some(value) = &*cached {
            return Ok(value.clone());
        }
        let value = market::credentials()?;
        *cached = Some(value.clone());
        Ok(value)
    }

    fn set_credentials(&self, value: Option<Credentials>) -> Result<()> {
        let mut cached = self.credentials.lock().map_err(err)?;
        market::store_credentials(value.as_ref())?;
        *cached = Some(value);
        self.alpaca.clear_price_cache()
    }

    fn with_accounts<T>(
        &self,
        db: &Connection,
        f: impl FnOnce(&Accounts) -> Result<T>,
    ) -> Result<T> {
        let revision = storage::revision(db)?;
        let mut cached = self.accounts.lock().map_err(err)?;
        if cached
            .as_ref()
            .is_none_or(|(version, _)| *version != revision)
        {
            let records = storage::load(db)?;
            *cached = Some((revision, ledger::replay(records.values())?.accounts));
        }
        f(&cached.as_ref().expect("accounts populated").1)
    }
}

#[derive(Serialize)]
struct Settings {
    connected: bool,
    credential_error: Option<String>,
    timezone: String,
}

#[tauri::command]
async fn portfolio(app: AppHandle) -> Result<Portfolio> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        market::portfolio(&db)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn list_batches(app: AppHandle) -> Result<Vec<Batch>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        storage::batches(&db)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn preview(app: AppHandle, action: Action) -> Result<Preview> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        storage::preview(&db, &action)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn commit(
    app: AppHandle,
    action: Action,
    revision: i64,
    confirm: BTreeSet<String>,
) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut db = state.db()?;
        storage::commit(&mut db, &action, revision, &confirm)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> Result<Settings> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let timezone = storage::setting(&*state.db()?, "timezone")?;
        let (connected, credential_error) = match state.credentials() {
            Ok(credentials) => (credentials.is_some(), None),
            Err(error) => (false, Some(error)),
        };
        Ok(Settings {
            connected,
            timezone,
            credential_error,
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn save_timezone(state: State<'_, AppState>, timezone: String) -> Result<()> {
    if timezone != "system" && timezone.parse::<chrono_tz::Tz>().is_err() {
        return Err("Use an IANA timezone such as America/New_York or Asia/Hong_Kong.".into());
    }
    storage::save_setting(&*state.db()?, "timezone", &timezone)
}

#[tauri::command]
async fn connect(state: State<'_, AppState>, mut credentials: Credentials) -> Result<()> {
    let _guard = state.network.lock().await;
    credentials.key = credentials.key.trim().into();
    credentials.secret = credentials.secret.trim().into();
    if credentials.key.is_empty() || credentials.secret.is_empty() {
        return Err("Enter both the Alpaca key ID and secret.".into());
    }
    state.alpaca.validate(&credentials, Utc::now()).await?;
    state.set_credentials(Some(credentials))
}

#[tauri::command]
async fn disconnect(state: State<'_, AppState>) -> Result<()> {
    let _guard = state.network.lock().await;
    state.set_credentials(None)
}

#[derive(Serialize)]
struct PriceRefresh {
    received: usize,
    valuation: Option<Valuation>,
}

#[tauri::command]
async fn refresh_prices(state: State<'_, AppState>, manual: bool) -> Result<PriceRefresh> {
    let _guard = state
        .network
        .try_lock()
        .map_err(|_| "A price refresh or credential check is already running.")?;
    let creds = state
        .credentials()?
        .ok_or("Connect Alpaca in Settings to refresh prices.")?;
    let symbols: Vec<_> = state.with_accounts(&*state.db()?, |accounts| {
        Ok(accounts
            .iter()
            .filter(|(_, account)| !account.quantity.is_zero())
            .map(|(symbol, _)| symbol.clone())
            .collect())
    })?;
    let prices = state.alpaca.refresh(&creds, &symbols, Utc::now()).await?;
    let received = prices.len();
    let mut db = state.db()?;
    let changed = market::save_quotes(&mut db, prices)?;
    let valuation = if changed || manual {
        Some(state.with_accounts(&db, |accounts| market::valuation(&db, accounts))?)
    } else {
        None
    };
    Ok(PriceRefresh {
        received,
        valuation,
    })
}

#[tauri::command]
fn opening_counterpart(quantity: String, cost: String, average: bool) -> Result<String> {
    storage::opening_counterpart(&quantity, &cost, average)
}

#[tauri::command]
fn set_override(
    state: State<'_, AppState>,
    symbol: String,
    value: Option<String>,
    at: Option<String>,
) -> Result<()> {
    market::set_override(&*state.db()?, &symbol, value.as_deref(), at.as_deref())
}

fn protect_database(selected: &Path, database: &Path) -> Result<()> {
    let selected = selected
        .canonicalize()
        .unwrap_or_else(|_| selected.to_owned());
    let database = database
        .canonicalize()
        .unwrap_or_else(|_| database.to_owned());
    let sidecars = [
        "portfolio.sqlite-wal",
        "portfolio.sqlite-shm",
        "portfolio.sqlite-journal",
    ];
    if selected == database
        || (selected.parent() == database.parent()
            && sidecars.iter().any(|s| selected.ends_with(s)))
    {
        return Err("Choose a file other than the active database.".into());
    }
    Ok(())
}

#[tauri::command]
async fn transfer(app: AppHandle, operation: String) -> Result<bool> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let dialog = app.dialog().file();
        let asset = match operation.as_str() {
            "template" => Some((
                "portfolio-template.csv",
                include_str!("../../public/portfolio-template.csv"),
            )),
            "example" => Some((
                "portfolio-example.csv",
                include_str!("../../public/portfolio-example.csv"),
            )),
            "guide" => Some((
                "broker-pdf-to-csv.md",
                include_str!("../../public/broker-pdf-to-csv.md"),
            )),
            _ => None,
        };
        let selected = match operation.as_str() {
            _ if asset.is_some() => dialog.set_file_name(asset.unwrap().0).blocking_save_file(),
            "backup" => dialog
                .set_file_name("Portfolio-backup.sqlite")
                .add_filter("SQLite backup", &["sqlite"])
                .blocking_save_file(),
            "restore" => dialog
                .add_filter("SQLite backup", &["sqlite"])
                .blocking_pick_file(),
            "export" => dialog
                .set_file_name("Portfolio-transactions.csv")
                .add_filter("CSV", &["csv"])
                .blocking_save_file(),
            _ => return Err("Unknown file operation.".into()),
        };
        let Some(file) = selected else {
            return Ok(false);
        };
        let path = file.into_path().map_err(err)?;
        protect_database(&path, &state.path)?;
        if let Some((_, contents)) = asset {
            storage::write_file(&path, contents.as_bytes())?;
            return Ok(true);
        }
        let mut db = state.db()?;
        match operation.as_str() {
            "backup" => storage::backup_file(&db, &path)?,
            "restore" => {
                let safety = state.path.with_file_name(format!(
                    "pre-restore-{}.sqlite",
                    Utc::now().timestamp_millis()
                ));
                storage::restore(&mut db, &path, &safety)?;
            }
            "export" => storage::export(&db, &path)?,
            _ => unreachable!(),
        }
        Ok(true)
    })
    .await
    .map_err(err)?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = std::env::var_os("PORTFOLIO_DATA_DIR")
                .map(PathBuf::from)
                .map(Ok)
                .unwrap_or_else(|| app.path().app_data_dir())?;
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("portfolio.sqlite");
            app.manage(AppState {
                db: Mutex::new(storage::open(&path)?),
                path,
                alpaca: Alpaca::new()?,
                network: Default::default(),
                credentials: Default::default(),
                accounts: Default::default(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            portfolio,
            list_batches,
            preview,
            commit,
            get_settings,
            save_timezone,
            connect,
            disconnect,
            refresh_prices,
            set_override,
            opening_counterpart,
            transfer
        ])
        .run(tauri::generate_context!())
        .expect("could not start Portfolio");
}
