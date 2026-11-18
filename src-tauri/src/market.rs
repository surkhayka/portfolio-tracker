use crate::{
    ledger::{self, Account, Record},
    storage::{self, err, Result},
};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::America::New_York;
use reqwest::{Client, RequestBuilder, StatusCode};
use rusqlite::{params, Connection};
use rust_decimal::Decimal as D;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Deserialize, Serialize)]
pub struct Quote {
    pub price: D,
    pub at: String,
    pub reference: Option<D>,
    pub reference_at: Option<String>,
    pub fetched_at: String,
    pub manual: bool,
}

#[derive(Serialize)]
pub struct Holding {
    symbol: String,
    #[serde(flatten)]
    account: Account,
    average: Option<D>,
    value: Option<D>,
    allocation: Option<D>,
    unrealized: Option<D>,
    unrealized_percent: Option<D>,
    total: Option<D>,
    daily: Option<D>,
    daily_percent: Option<D>,
    quote: Option<Quote>,
    has_override: bool,
}

#[derive(Serialize)]
pub struct Summary {
    basis: D,
    invested: D,
    value: Option<D>,
    unrealized: Option<D>,
    unrealized_percent: Option<D>,
    daily: Option<D>,
    daily_percent: Option<D>,
    realized: D,
    dividends: D,
    total: Option<D>,
    return_on_invested: Option<D>,
    incomplete: bool,
    missing: Vec<String>,
}

#[derive(Serialize)]
pub struct Portfolio {
    #[serde(flatten)]
    valuation: Valuation,
    transactions: Vec<DisplayRecord>,
}

#[derive(Serialize)]
pub struct Valuation {
    revision: i64,
    holdings: Vec<Holding>,
    summary: Summary,
}

#[derive(Serialize)]
struct DisplayRecord {
    #[serde(flatten)]
    record: Record,
    gross: Option<D>,
}

fn share(num: D, den: D) -> Option<D> {
    if den.is_zero() {
        None
    } else {
        num.checked_div(den)
    }
}

fn split_factor(account: &Account, before: &str, through: &str) -> Option<D> {
    account
        .splits
        .iter()
        .filter(|(date, _)| date.as_str() > before && date.as_str() <= through)
        .try_fold(D::ONE, |factor, (_, ratio)| {
            factor.checked_mul(*ratio).filter(|n| !n.is_zero())
        })
}

pub fn portfolio(db: &Connection) -> Result<Portfolio> {
    let records = storage::load(db)?;
    let accounts = ledger::replay(records.values())?.accounts;
    let valuation = valuation(db, &accounts)?;
    let mut transactions: Vec<_> = records.into_values().collect();
    transactions
        .sort_by(|a, b| (&b.event.timestamp, b.sequence).cmp(&(&a.event.timestamp, a.sequence)));
    let transactions = transactions
        .into_iter()
        .map(|record| DisplayRecord {
            gross: (record.event.kind != ledger::Kind::Split).then(|| record.event.gross()),
            record,
        })
        .collect();
    Ok(Portfolio {
        valuation,
        transactions,
    })
}

pub fn valuation(db: &Connection, accounts: &BTreeMap<String, Account>) -> Result<Valuation> {
    let mut prices = table(db, false)?;
    let overrides = table(db, true)?;
    let mut summary = Summary {
        basis: D::ZERO,
        invested: D::ZERO,
        value: Some(D::ZERO),
        unrealized: None,
        unrealized_percent: None,
        daily: None,
        daily_percent: None,
        total: None,
        return_on_invested: None,
        realized: D::ZERO,
        dividends: D::ZERO,
        incomplete: false,
        missing: vec![],
    };
    let mut previous_value = Some(D::ZERO);
    let mut daily_session: Option<(String, String)> = None;
    let mut holdings = vec![];
    for (symbol, account) in accounts {
        let active = !account.quantity.is_zero();
        let has_override = overrides.contains_key(symbol);
        let quote = overrides
            .get(symbol)
            .cloned()
            .or_else(|| prices.remove(symbol))
            .filter(|quote| active && !account.splits.iter().any(|(date, _)| date > &quote.at));
        let value = if active {
            quote
                .as_ref()
                .and_then(|q| account.quantity.checked_mul(q.price))
        } else {
            Some(D::ZERO)
        };
        summary.basis += account.basis;
        summary.invested += account.invested;
        summary.realized += account.realized;
        summary.dividends += account.dividends;
        summary.incomplete |= account.incomplete;
        if value.is_none() {
            summary.missing.push(symbol.clone());
        }
        summary.value = summary.value.zip(value).map(|(a, b)| a + b);
        let unrealized = value.map(|v| v - account.basis);
        let average = share(account.basis, account.quantity);
        let unrealized_percent = unrealized.and_then(|u| share(u, account.basis));
        let total = unrealized.map(|u| u + account.realized + account.dividends);
        let reference = quote.as_ref().and_then(|q| {
            let factor = split_factor(account, q.reference_at.as_deref()?, &q.at)?;
            q.reference?.checked_div(factor).filter(|p| *p > D::ZERO)
        });
        let difference = quote
            .as_ref()
            .zip(reference)
            .map(|(quote, prior)| quote.price - prior);
        let daily = difference.and_then(|d| account.quantity.checked_mul(d));
        let daily_percent = difference.zip(reference).and_then(|(a, b)| share(a, b));
        if active {
            previous_value = previous_value.and_then(|sum| {
                let quote = quote.as_ref()?;
                // Regular US sessions fit within one UTC date. Quote timestamps are stored in UTC.
                let session = (quote.at.get(..10)?, quote.reference_at.as_deref()?);
                if let Some((date, close)) = &daily_session {
                    if (date.as_str(), close.as_str()) != session {
                        return None;
                    }
                } else {
                    daily_session = Some((session.0.into(), session.1.into()));
                }
                sum.checked_add(account.quantity.checked_mul(reference?)?)
            });
        }
        holdings.push(Holding {
            symbol: symbol.clone(),
            account: account.clone(),
            average,
            value,
            allocation: None,
            unrealized,
            unrealized_percent,
            total,
            daily,
            daily_percent,
            quote,
            has_override,
        });
    }
    summary.unrealized = summary.value.map(|v| v - summary.basis);
    summary.unrealized_percent = summary.unrealized.and_then(|u| share(u, summary.basis));
    let previous_value = previous_value.filter(|value| !value.is_zero());
    summary.daily = summary
        .value
        .zip(previous_value)
        .and_then(|(current, prior)| current.checked_sub(prior));
    summary.daily_percent = summary
        .daily
        .zip(previous_value)
        .and_then(|(daily, prior)| share(daily, prior));
    summary.total = summary
        .unrealized
        .map(|u| u + summary.realized + summary.dividends);
    summary.return_on_invested = summary.total.and_then(|gain| share(gain, summary.invested));
    for holding in &mut holdings {
        holding.allocation = if holding.account.quantity.is_zero() {
            None
        } else {
            holding
                .value
                .zip(summary.value)
                .and_then(|(value, total)| share(value, total))
        };
    }
    Ok(Valuation {
        revision: storage::revision(db)?,
        holdings,
        summary,
    })
}

fn table(db: &Connection, manual: bool) -> Result<BTreeMap<String, Quote>> {
    let sql = if manual {
        "SELECT symbol,quote FROM overrides"
    } else {
        "SELECT symbol,quote FROM prices"
    };
    let mut query = db.prepare(sql).map_err(err)?;
    let rows = query
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(err)?;
    rows.map(|row| {
        let (symbol, json) = row.map_err(err)?;
        let quote: Quote = serde_json::from_str(&json).map_err(err)?;
        if quote.price <= D::ZERO
            || quote.price >= D::from(1_000_000_000_000u64)
            || ledger::timestamp(&quote.at)? != quote.at
            || quote.manual != manual
        {
            return Err("Invalid stored price.".into());
        }
        ledger::timestamp(&quote.fetched_at)?;
        if let Some(reference) = quote.reference_at.as_deref() {
            ledger::timestamp(reference)?;
        }
        Ok((ledger::symbol(&symbol)?, quote))
    })
    .collect()
}

pub fn validate_cache(db: &Connection) -> Result<()> {
    table(db, false)?;
    table(db, true)?;
    Ok(())
}

pub fn save_quotes(db: &mut Connection, quotes: BTreeMap<String, Quote>) -> Result<bool> {
    if quotes.is_empty() {
        return Ok(false);
    }
    let old = table(db, false)?;
    let changes: Vec<_> = quotes
        .into_iter()
        .filter(|(symbol, quote)| {
            old.get(symbol).is_none_or(|previous| {
                previous.at < quote.at
                    || (previous.at == quote.at
                        && (previous.price != quote.price
                            || previous.reference != quote.reference
                            || previous.reference_at != quote.reference_at))
            })
        })
        .collect();
    if changes.is_empty() {
        return Ok(false);
    }
    let tx = db.transaction().map_err(err)?;
    for (symbol, quote) in changes {
        tx.execute(
            "INSERT OR REPLACE INTO prices VALUES (?1,?2)",
            params![symbol, serde_json::to_string(&quote).map_err(err)?],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(true)
}

pub fn set_override(
    db: &Connection,
    symbol: &str,
    value: Option<&str>,
    at: Option<&str>,
) -> Result<()> {
    let symbol = ledger::symbol(symbol)?;
    if let Some(value) = value {
        let price = ledger::decimal(value)?;
        if price <= D::ZERO {
            return Err("Price must be positive.".into());
        }
        let at = ledger::timestamp(at.ok_or("Enter a price timestamp.")?)?;
        if DateTime::parse_from_rfc3339(&at).map_err(err)?.to_utc() > Utc::now() {
            return Err("Price timestamp cannot be in the future.".into());
        }
        let quote = Quote {
            price,
            at,
            reference: None,
            reference_at: None,
            fetched_at: iso(Utc::now()),
            manual: true,
        };
        db.execute(
            "INSERT OR REPLACE INTO overrides VALUES (?1,?2)",
            params![symbol, serde_json::to_string(&quote).map_err(err)?],
        )
        .map_err(err)?;
    } else {
        db.execute("DELETE FROM overrides WHERE symbol=?1", [symbol])
            .map_err(err)?;
    }
    Ok(())
}

fn iso(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub key: String,
    pub secret: String,
    pub live: bool,
}

const KEY_SERVICE: &str = "app.portfolio.local";
fn keyring() -> Result<keyring::Entry> {
    keyring::Entry::new(KEY_SERVICE, "alpaca")
        .map_err(|_| "Cannot access the OS credential store.".into())
}

pub fn credentials() -> Result<Option<Credentials>> {
    match keyring()?.get_password() {
        Ok(data) => serde_json::from_str(&data)
            .map(Some)
            .map_err(|_| "Saved Alpaca credentials could not be read.".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Cannot read Alpaca credentials from the OS credential store.".into()),
    }
}

pub fn store_credentials(value: Option<&Credentials>) -> Result<()> {
    let entry = keyring()?;
    match value {
        Some(creds) => entry.set_password(&serde_json::to_string(creds).map_err(err)?),
        None => match entry.delete_credential() {
            Err(keyring::Error::NoEntry) => Ok(()),
            result => result,
        },
    }
    .map_err(|_| "Cannot update the OS credential store.".into())
}

#[derive(Clone, Deserialize)]
struct CalendarDay {
    date: NaiveDate,
    open: String,
    close: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Session {
    date: NaiveDate,
    open: DateTime<Utc>,
    close: DateTime<Utc>,
}

impl CalendarDay {
    fn session(self) -> Option<Session> {
        let local = |text: &str| {
            let time = NaiveTime::parse_from_str(text, "%H:%M")
                .or_else(|_| text.parse())
                .ok()?;
            New_York
                .from_local_datetime(&self.date.and_time(time))
                .single()
                .map(|dt| dt.to_utc())
        };
        Some(Session {
            date: self.date,
            open: local(&self.open)?,
            close: local(&self.close)?,
        })
    }
}

#[derive(Deserialize)]
struct Bars {
    bars: Option<BTreeMap<String, Vec<Bar>>>,
    next_page_token: Option<String>,
}

#[derive(Clone, Deserialize)]
struct Bar {
    #[serde(rename = "t")]
    at: DateTime<Utc>,
    #[serde(rename = "c")]
    close: serde_json::Number,
}

impl Bar {
    fn price(&self) -> Option<D> {
        self.close
            .as_str()
            .parse::<D>()
            .ok()
            .filter(|p| *p > D::ZERO && *p < D::from(1_000_000_000_000u64))
    }
}

pub trait MarketProvider {
    async fn refresh(
        &self,
        credentials: &Credentials,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<BTreeMap<String, Quote>>;
}

#[derive(Default)]
struct PriceHistory {
    completed_close: Option<DateTime<Utc>>,
    daily: BTreeMap<String, Vec<Bar>>,
    session_open: Option<DateTime<Utc>>,
    minutes: BTreeMap<String, MinuteMark>,
}

struct MinuteMark {
    checked_through: DateTime<Utc>,
    latest: Option<Bar>,
}

pub struct Alpaca {
    client: Client,
    calendar: std::sync::Mutex<Option<(NaiveDate, bool, Vec<Session>)>>,
    prices: std::sync::Mutex<PriceHistory>,
    data_url: String,
    calendar_url: Option<String>,
}

impl Alpaca {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(err)?,
            calendar: Default::default(),
            prices: Default::default(),
            data_url: "https://data.alpaca.markets/v2/stocks/bars".into(),
            calendar_url: None,
        })
    }

    pub fn clear_price_cache(&self) -> Result<()> {
        *self.prices.lock().map_err(err)? = PriceHistory::default();
        Ok(())
    }

    async fn read<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        key: &Credentials,
    ) -> Result<T> {
        let response = request
            .header("APCA-API-KEY-ID", &key.key)
            .header("APCA-API-SECRET-KEY", &key.secret)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "Alpaca timed out. Cached prices are still available.".to_string()
                } else {
                    "Alpaca is unreachable. Check your connection.".into()
                }
            })?;
        match response.status() {
            StatusCode::OK => response
                .json()
                .await
                .map_err(|_| "Alpaca returned an unreadable response.".into()),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(
                "Alpaca denied access. Check the key, account mode, and market data access.".into(),
            ),
            StatusCode::TOO_MANY_REQUESTS => {
                Err("Alpaca rate limit reached. Try again in a minute.".into())
            }
            status => Err(format!("Alpaca returned HTTP {}.", status.as_u16())),
        }
    }

    async fn sessions(
        &self,
        key: &Credentials,
        day: NaiveDate,
        use_cache: bool,
    ) -> Result<Vec<Session>> {
        if use_cache {
            if let Some((date, live, sessions)) = self.calendar.lock().map_err(err)?.as_ref() {
                if *date == day && *live == key.live {
                    return Ok(sessions.clone());
                }
            }
        }
        let default_url = if key.live {
            "https://api.alpaca.markets/v2/calendar"
        } else {
            "https://paper-api.alpaca.markets/v2/calendar"
        };
        let start = (day - Duration::days(14)).to_string();
        let calendar: Vec<CalendarDay> = self
            .read(
                self.client
                    .get(self.calendar_url.as_deref().unwrap_or(default_url))
                    .query(&[("start", start), ("end", day.to_string())]),
                key,
            )
            .await?;
        let mut sessions: Vec<_> = calendar
            .into_iter()
            .filter_map(CalendarDay::session)
            .collect();
        sessions.sort_by_key(|s| s.open);
        if sessions.len() < 2 {
            return Err("Alpaca did not provide enough market sessions for this date.".into());
        }
        *self.calendar.lock().map_err(err)? = Some((day, key.live, sessions.clone()));
        Ok(sessions)
    }

    async fn bars(
        &self,
        key: &Credentials,
        symbols: &[String],
        timeframe: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<BTreeMap<String, Vec<Bar>>> {
        let mut token = None::<String>;
        let mut seen = BTreeSet::new();
        let mut result = BTreeMap::<String, Vec<Bar>>::new();
        let symbols = symbols.join(",");
        let start = iso(start);
        let end = iso(end);
        loop {
            let mut req = self.client.get(&self.data_url).query(&[
                ("symbols", symbols.as_str()),
                ("feed", "sip"),
                ("timeframe", timeframe),
                ("start", &start),
                ("end", &end),
                ("adjustment", "raw"),
                ("limit", "10000"),
            ]);
            if let Some(page) = token.as_deref() {
                req = req.query(&[("page_token", page)]);
            }
            let response: Bars = self.read(req, key).await?;
            for (symbol, mut bars) in response.bars.unwrap_or_default() {
                result.entry(symbol).or_default().append(&mut bars);
            }
            token = response.next_page_token;
            let Some(next) = token.as_ref() else {
                return Ok(result);
            };
            if seen.len() >= 40 || !seen.insert(next.clone()) {
                return Err("Alpaca returned too many or repeated pages.".into());
            }
        }
    }

    pub async fn validate(&self, key: &Credentials, now: DateTime<Utc>) -> Result<()> {
        let sessions = self
            .sessions(key, now.with_timezone(&New_York).date_naive(), false)
            .await?;
        let cutoff = delayed_cutoff(now);
        let session = sessions
            .iter()
            .rev()
            .find(|s| s.close <= cutoff)
            .ok_or("No completed pricing session.")?;
        let probe = self
            .bars(
                key,
                &["SPY".into()],
                "1Day",
                session.open - Duration::hours(10),
                cutoff,
            )
            .await?;
        if probe.is_empty() {
            return Err("Alpaca accepted the credentials but supplied no delayed SIP data.".into());
        }
        Ok(())
    }
}

fn delayed_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - Duration::seconds(905)
}

fn daily_marks(
    bars: &[Bar],
    sessions: &[Session],
    cutoff: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, D)> {
    let mut marks: Vec<_> = bars
        .iter()
        .filter_map(|bar| {
            let date = bar.at.with_timezone(&New_York).date_naive();
            let close = sessions.iter().find(|s| s.date == date)?.close;
            (close <= cutoff)
                .then(|| bar.price().map(|value| (close, value)))
                .flatten()
        })
        .collect();
    marks.sort_by_key(|(at, _)| *at);
    marks
}

fn latest_minute<'a>(
    session: &Session,
    minutes: &'a [Bar],
    cutoff: DateTime<Utc>,
) -> Option<&'a Bar> {
    minutes
        .iter()
        .filter(|bar| {
            let end = bar.at + Duration::minutes(1);
            bar.at >= session.open && end <= cutoff && end <= session.close && bar.price().is_some()
        })
        .max_by_key(|bar| bar.at)
}

fn select_quote(
    session: &Session,
    calendar: &[Session],
    daily: &[Bar],
    minutes: &[Bar],
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<Quote> {
    let marks = daily_marks(daily, calendar, cutoff);
    let intraday = latest_minute(session, minutes, cutoff).and_then(|bar| {
        bar.price()
            .map(|price| (bar.at + Duration::minutes(1), price))
    });
    let (at, price) = marks
        .iter()
        .copied()
        .chain(intraday)
        .max_by_key(|(at, _)| *at)?;
    let date = at.with_timezone(&New_York).date_naive();
    let reference = calendar
        .iter()
        .rev()
        .find(|s| s.date < date)
        .and_then(|previous| marks.iter().find(|(at, _)| *at == previous.close).copied());
    Some(Quote {
        price,
        at: iso(at),
        reference: reference.map(|(_, price)| price),
        reference_at: reference.map(|(at, _)| iso(at)),
        fetched_at: iso(now),
        manual: false,
    })
}

impl MarketProvider for Alpaca {
    async fn refresh(
        &self,
        key: &Credentials,
        symbols: &[String],
        now: DateTime<Utc>,
    ) -> Result<BTreeMap<String, Quote>> {
        if symbols.is_empty() {
            return Ok(BTreeMap::new());
        }
        let cutoff = delayed_cutoff(now);
        let sessions = self
            .sessions(key, now.with_timezone(&New_York).date_naive(), true)
            .await?;
        let current = sessions
            .iter()
            .rev()
            .find(|s| s.open + Duration::minutes(1) <= cutoff)
            .ok_or("No delayed market session is available.")?;
        let completed = sessions
            .iter()
            .rev()
            .find(|session| session.close <= cutoff)
            .ok_or("No completed pricing session.")?;
        let missing_daily = {
            let mut history = self.prices.lock().map_err(err)?;
            if history.completed_close != Some(completed.close) {
                history.completed_close = Some(completed.close);
                history.daily.clear();
            }
            if history.session_open != Some(current.open) {
                history.session_open = Some(current.open);
                history.minutes.clear();
            }
            symbols
                .iter()
                .filter(|s| !history.daily.contains_key(*s))
                .cloned()
                .collect::<Vec<_>>()
        };
        for group in missing_daily.chunks(50) {
            let start = sessions.first().expect("nonempty calendar").open - Duration::hours(10);
            let mut bars = self
                .bars(key, group, "1Day", start, completed.close)
                .await?;
            let mut history = self.prices.lock().map_err(err)?;
            for symbol in group {
                history
                    .daily
                    .insert(symbol.clone(), bars.remove(symbol).unwrap_or_default());
            }
        }

        if current.close > cutoff {
            let mut ranges = BTreeMap::<DateTime<Utc>, Vec<String>>::new();
            {
                let history = self.prices.lock().map_err(err)?;
                for symbol in symbols {
                    let start = if let Some(mark) = history.minutes.get(symbol) {
                        if mark.checked_through.timestamp() / 60 >= cutoff.timestamp() / 60 {
                            continue;
                        }
                        (mark.checked_through - Duration::minutes(2)).max(current.open)
                    } else {
                        (cutoff - Duration::minutes(20)).max(current.open)
                    };
                    ranges.entry(start).or_default().push(symbol.clone());
                }
            }
            for (start, symbols) in ranges {
                for group in symbols.chunks(50) {
                    let mut bars = self.bars(key, group, "1Min", start, cutoff).await?;
                    // Backfill sparse symbols only once on entry, then check successive minutes.
                    let sparse = {
                        let history = self.prices.lock().map_err(err)?;
                        group
                            .iter()
                            .filter(|s| {
                                start > current.open
                                    && !history.minutes.contains_key(*s)
                                    && latest_minute(
                                        current,
                                        bars.get(*s).map(Vec::as_slice).unwrap_or_default(),
                                        cutoff,
                                    )
                                    .is_none()
                            })
                            .cloned()
                            .collect::<Vec<_>>()
                    };
                    if !sparse.is_empty() {
                        for (symbol, mut older) in
                            self.bars(key, &sparse, "1Min", current.open, start).await?
                        {
                            bars.entry(symbol).or_default().append(&mut older);
                        }
                    }
                    let mut history = self.prices.lock().map_err(err)?;
                    for symbol in group {
                        let observed = latest_minute(
                            current,
                            bars.get(symbol).map(Vec::as_slice).unwrap_or_default(),
                            cutoff,
                        );
                        let previous = history
                            .minutes
                            .get(symbol)
                            .and_then(|mark| mark.latest.as_ref());
                        let latest = match (previous, observed) {
                            (Some(old), Some(new)) if old.at > new.at => Some(old.clone()),
                            (_, Some(new)) => Some(new.clone()),
                            (old, None) => old.cloned(),
                        };
                        history.minutes.insert(
                            symbol.clone(),
                            MinuteMark {
                                checked_through: cutoff,
                                latest,
                            },
                        );
                    }
                }
            }
        }

        let history = self.prices.lock().map_err(err)?;
        let mut quotes = BTreeMap::new();
        for symbol in symbols {
            let minute = (current.close > cutoff)
                .then(|| {
                    history
                        .minutes
                        .get(symbol)
                        .and_then(|mark| mark.latest.as_ref())
                })
                .flatten();
            if let Some(quote) = select_quote(
                current,
                &sessions,
                history
                    .daily
                    .get(symbol)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                minute.map(std::slice::from_ref).unwrap_or_default(),
                cutoff,
                now,
            ) {
                quotes.insert(symbol.clone(), quote);
            }
        }
        Ok(quotes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn at(time: &str) -> DateTime<Utc> {
        time.parse().unwrap()
    }

    fn session(date: &str, close: &str) -> Session {
        CalendarDay {
            date: date.parse().unwrap(),
            open: "09:30".into(),
            close: close.into(),
        }
        .session()
        .unwrap()
    }

    fn bar(time: &str, value: &str) -> Bar {
        Bar {
            at: at(time),
            close: value.parse().unwrap(),
        }
    }

    #[test]
    fn regular_session_early_close_weekend_and_dst() {
        let previous = session("2025-11-26", "16:00");
        let short = session("2025-11-28", "13:00");
        assert_eq!(short.close, at("2025-11-28T18:00:00Z"));
        assert_eq!(
            session("2025-07-03", "13:00").close,
            at("2025-07-03T17:00:00Z")
        );
        let calendar = [previous, short.clone()];
        let daily = [
            bar("2025-11-26T05:00:00Z", "100"),
            bar("2025-11-28T05:00:00Z", "113"),
        ];
        let minutes = [
            bar("2025-11-28T17:58:00Z", "110"),
            bar("2025-11-28T17:59:00Z", "111"),
            bar("2025-11-28T18:00:00Z", "150"),
        ];
        let now = at("2025-11-28T18:15:00Z");
        let quote = select_quote(
            &short,
            &calendar,
            &daily,
            &minutes,
            delayed_cutoff(now),
            now,
        )
        .unwrap();
        assert_eq!(quote.price, ledger::num("110"));
        assert_eq!(quote.reference, Some(ledger::num("100")));
        let weekend = at("2025-11-29T17:00:00Z");
        let quote = select_quote(
            &short,
            &calendar,
            &daily,
            &[],
            delayed_cutoff(weekend),
            weekend,
        )
        .unwrap();
        assert_eq!(quote.price, ledger::num("113"));
        assert_eq!(quote.reference, Some(ledger::num("100")));
        let sparse = select_quote(
            &short,
            &calendar,
            &daily[1..],
            &[],
            delayed_cutoff(weekend),
            weekend,
        )
        .unwrap();
        assert!(sparse.reference.is_none());
    }

    #[test]
    fn delayed_sip_provider_paginates_and_reports_api_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for _ in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    request.push_str(&line);
                }
                let line = request.lines().next().unwrap();
                let (status, body) = if request.contains("bad-key") {
                    ("403 Forbidden", "{}")
                } else if request.contains("rate-key") {
                    ("429 Too Many Requests", "{}")
                } else if line.contains("/calendar") {
                    (
                        "200 OK",
                        r#"[{"date":"2025-01-08","open":"09:30","close":"16:00"},{"date":"2025-01-09","open":"09:30","close":"16:00"}]"#,
                    )
                } else if line.contains("page_token=second") {
                    (
                        "200 OK",
                        r#"{"bars":{"MSFT":[{"t":"2025-01-09T05:00:00Z","c":409.60}]},"next_page_token":null}"#,
                    )
                } else {
                    assert!(
                        line.contains("feed=sip")
                            && line.contains("timeframe=1Day")
                            && line.contains("adjustment=raw")
                    );
                    (
                        "200 OK",
                        r#"{"bars":{"AAPL":[{"t":"2025-01-09T05:00:00Z","c":242.05}]},"next_page_token":"second"}"#,
                    )
                };
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        tauri::async_runtime::block_on(async {
            let mut provider = Alpaca::new().unwrap();
            provider.data_url = format!("{base}/bars");
            provider.calendar_url = Some(format!("{base}/calendar"));
            let key = |key: &str| Credentials {
                key: key.into(),
                secret: "mock-only".into(),
                live: false,
            };
            let now = at("2025-01-10T03:00:00Z");
            let quotes = provider
                .refresh(
                    &key("mock"),
                    &["AAPL".into(), "MSFT".into(), "AMD".into()],
                    now,
                )
                .await
                .unwrap();
            assert_eq!(quotes.len(), 2);
            assert_eq!(quotes["MSFT"].price, ledger::num("409.60"));
            let repeated = provider
                .refresh(
                    &key("mock"),
                    &["AAPL".into(), "MSFT".into(), "AMD".into()],
                    now + Duration::minutes(2),
                )
                .await
                .unwrap();
            assert_eq!(repeated.len(), 2);
            assert!(provider
                .validate(&key("bad-key"), now)
                .await
                .unwrap_err()
                .contains("denied access"));
            assert!(provider
                .validate(&key("rate-key"), now)
                .await
                .unwrap_err()
                .contains("rate limit"));
        });
        server.join().unwrap();
    }
    #[test]
    fn intraday_refresh_fetches_only_new_minutes_after_first_call() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
            let mut requests = vec![];
            while requests.len() < 4 && std::time::Instant::now() < deadline {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let body = if line.contains("/calendar") {
                    r#"[{"date":"2025-01-08","open":"09:30","close":"16:00"},{"date":"2025-01-09","open":"09:30","close":"16:00"}]"#
                } else if line.contains("timeframe=1Day") {
                    r#"{"bars":{"AAPL":[{"t":"2025-01-08T05:00:00Z","c":95}]},"next_page_token":null}"#
                } else if requests.len() == 2 {
                    r#"{"bars":{"AAPL":[{"t":"2025-01-09T15:59:00Z","c":100}]},"next_page_token":null}"#
                } else {
                    r#"{"bars":{"AAPL":[{"t":"2025-01-09T16:01:00Z","c":102}]},"next_page_token":null}"#
                };
                requests.push(line);
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        tauri::async_runtime::block_on(async {
            let mut provider = Alpaca::new().unwrap();
            provider.data_url = format!("{base}/bars");
            provider.calendar_url = Some(format!("{base}/calendar"));
            let key = Credentials {
                key: "mock".into(),
                secret: "mock-only".into(),
                live: false,
            };
            let symbols = ["AAPL".into()];
            let now = at("2025-01-09T16:15:10Z");
            assert_eq!(
                provider.refresh(&key, &symbols, now).await.unwrap()["AAPL"].price,
                D::from(100)
            );
            let same_minute = provider
                .refresh(&key, &symbols, now + Duration::seconds(10))
                .await
                .unwrap();
            assert_eq!(same_minute["AAPL"].price, D::from(100));
            let next_minute = provider
                .refresh(&key, &symbols, now + Duration::minutes(2))
                .await
                .unwrap();
            assert_eq!(next_minute["AAPL"].price, D::from(102));
            assert_eq!(next_minute["AAPL"].reference, Some(D::from(95)));
        });
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(requests[2].contains("start=2025-01-09T15%3A40%3A05"));
        assert!(requests[3].contains("start=2025-01-09T15%3A58%3A05"));
    }
}
