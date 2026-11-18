use chrono::{DateTime, SecondsFormat};
use rust_decimal::Decimal as D;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

pub const HEADER: &str = "transaction_id,timestamp,type,symbol,currency,quantity,unit_price,amount,fees,close_position,split_ratio,reported_pnl,notes";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Buy,
    Sell,
    OpeningBalance,
    Dividend,
    Split,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub transaction_id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub symbol: String,
    pub currency: String,
    pub quantity: Option<String>,
    pub unit_price: Option<String>,
    pub amount: Option<String>,
    pub fees: Option<String>,
    pub close_position: Option<bool>,
    pub split_ratio: Option<String>,
    pub reported_pnl: Option<String>,
    pub notes: Option<String>,
}

pub fn decimal(input: &str) -> Result<D, String> {
    let unsigned = input
        .strip_prefix('-')
        .or_else(|| input.strip_prefix('+'))
        .unwrap_or(input);
    let mut parts = unsigned.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if whole.is_empty()
        || whole.len() > 12
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction
            .is_some_and(|p| p.is_empty() || p.len() > 12 || !p.bytes().all(|b| b.is_ascii_digit()))
        || parts.next().is_some()
    {
        return Err(
            "Use a plain decimal with up to 12 digits on each side of the decimal point.".into(),
        );
    }
    D::from_str(input).map_err(|_| "Invalid decimal.".into())
}

fn parse_field(
    field: &mut Option<String>,
    label: &str,
    required: bool,
    allowed: bool,
    min: Option<D>,
) -> Result<(), String> {
    let Some(value) = field.as_mut() else {
        return if required {
            Err(format!("{label} is required."))
        } else {
            Ok(())
        };
    };
    if !allowed {
        return Err(format!("{label} does not apply to this transaction."));
    }
    *value = value.trim().into();
    let number = decimal(value).map_err(|e| format!("{label}: {e}"))?;
    if let Some(minimum) = min {
        if number < minimum {
            return Err(format!(
                "{label} must be {}.",
                if minimum.is_zero() {
                    "nonnegative"
                } else {
                    "positive"
                }
            ));
        }
    }
    Ok(())
}

pub fn timestamp(value: &str) -> Result<String, String> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|dt| dt.to_utc().to_rfc3339_opts(SecondsFormat::Nanos, true))
        .map_err(|_| "Use an ISO 8601 timestamp with a timezone.".into())
}

pub fn symbol(value: &str) -> Result<String, String> {
    let upper = value.trim().to_ascii_uppercase();
    if upper.is_empty()
        || upper.len() > 16
        || !upper.as_bytes()[0].is_ascii_alphabetic()
        || !upper
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err("Enter a US stock or ETF ticker (for example AAPL or BRK.B).".into());
    }
    Ok(upper)
}

impl Event {
    pub fn validate(mut self) -> Result<Self, String> {
        self.transaction_id = self.transaction_id.trim().into();
        if self.transaction_id.is_empty()
            || self.transaction_id.len() > 160
            || !self
                .transaction_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:+".contains(&b))
        {
            return Err("Enter a stable transaction ID using letters, numbers, or -_.:+.".into());
        }
        self.timestamp = timestamp(&self.timestamp)?;
        self.symbol = symbol(&self.symbol)?;
        if self.currency != "USD" {
            return Err("Only USD stock and ETF transactions are supported.".into());
        }
        let trade = matches!(self.kind, Kind::Buy | Kind::Sell);
        let opening = self.kind == Kind::OpeningBalance;
        let dividend = self.kind == Kind::Dividend;
        let sale = self.kind == Kind::Sell;
        let split = self.kind == Kind::Split;
        let positive = Some(D::new(1, 12));
        parse_field(
            &mut self.quantity,
            "Quantity",
            trade || opening,
            trade || opening,
            positive,
        )?;
        parse_field(&mut self.unit_price, "Unit price", trade, trade, positive)?;
        parse_field(
            &mut self.amount,
            "Amount",
            opening || dividend,
            trade || opening || dividend,
            if opening { Some(D::ZERO) } else { positive },
        )?;
        parse_field(
            &mut self.fees,
            "Fees",
            false,
            trade || dividend,
            Some(D::ZERO),
        )?;
        parse_field(&mut self.split_ratio, "Split ratio", split, split, positive)?;
        if self
            .split_ratio
            .as_deref()
            .is_some_and(|v| num(v) > D::from(10_000))
        {
            return Err("Split ratio exceeds 10,000.".into());
        }
        parse_field(&mut self.reported_pnl, "Reported PnL", false, sale, None)?;
        if !sale && self.close_position == Some(true) {
            return Err("Only a sale can close a holding.".into());
        }
        self.close_position = self.close_position.filter(|&v| v);
        self.notes = self
            .notes
            .take()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if self.gross().abs() > D::from(1_000_000_000_000_000_000u64) {
            return Err("Gross amount exceeds supported range.".into());
        }
        Ok(self)
    }

    pub fn equivalent(&self, other: &Self) -> bool {
        self.economic_match(other)
            && optional_number(&self.reported_pnl) == optional_number(&other.reported_pnl)
            && self.notes == other.notes
    }

    pub fn economic_match(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
            && self.kind == other.kind
            && self.symbol == other.symbol
            && self.currency == other.currency
            && optional_number(&self.quantity) == optional_number(&other.quantity)
            && optional_number(&self.unit_price) == optional_number(&other.unit_price)
            && self.gross() == other.gross()
            && val(&self.fees) == val(&other.fees)
            && self.close_position == other.close_position
            && optional_number(&self.split_ratio) == optional_number(&other.split_ratio)
    }

    pub fn gross(&self) -> D {
        optional_number(&self.amount).unwrap_or_else(|| val(&self.quantity) * val(&self.unit_price))
    }

    fn amount_warning(&self) -> Option<String> {
        let gross = optional_number(&self.amount)?;
        let q = self.quantity.as_ref()?;
        let p = self.unit_price.as_ref()?;
        let tolerance = D::new(5, 3) + half_unit(q) * num(p) + half_unit(p) * num(q);
        ((gross - num(q) * num(p)).abs() > tolerance)
            .then(|| format!("{}: stated gross amount differs from quantity × price. The stated amount will be used.", self.transaction_id))
    }
}

fn optional_number(field: &Option<String>) -> Option<D> {
    field.as_deref().map(num)
}
pub fn num(s: &str) -> D {
    D::from_str(s).expect("validated decimal")
}
fn val(field: &Option<String>) -> D {
    optional_number(field).unwrap_or_default()
}
fn half_unit(s: &str) -> D {
    D::new(5, num(s).scale() + 1)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Record {
    pub event: Event,
    pub sequence: i64,
    pub excluded: bool,
    pub reconciled_quantity: Option<String>,
    pub revision: i64,
}

#[derive(Clone, Default, Debug, Serialize, PartialEq, Eq)]
pub struct Account {
    pub quantity: D,
    pub basis: D,
    pub realized: D,
    pub dividends: D,
    pub incomplete: bool,
    #[serde(skip)]
    pub invested: D,
    #[serde(skip)]
    precision: D,
    #[serde(skip)]
    pub splits: Vec<(String, D)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Adjustment {
    pub id: String,
    pub original: D,
    pub effective: D,
    pub confirmed: bool,
}

#[derive(Default)]
pub struct Replay {
    pub accounts: BTreeMap<String, Account>,
    pub warnings: Vec<String>,
    pub adjustments: Vec<Adjustment>,
}

pub fn round_scale(number: D) -> D {
    number.round_dp(16)
}

pub fn replay<'a>(records: impl IntoIterator<Item = &'a Record>) -> Result<Replay, String> {
    let mut records: Vec<_> = records.into_iter().filter(|row| !row.excluded).collect();
    records.sort_by(|a, b| (&a.event.timestamp, a.sequence).cmp(&(&b.event.timestamp, b.sequence)));
    let mut output = Replay::default();
    let mut prior = BTreeMap::<&str, (&str, Kind)>::new();
    let mut warned = BTreeSet::new();
    for row in records {
        let e = &row.event;
        let account_exists = output.accounts.contains_key(&e.symbol);
        let p = output.accounts.entry(e.symbol.clone()).or_default();
        let error = |message: &str| format!("{} · {}: {message}", e.symbol, e.transaction_id);
        let quantity = val(&e.quantity);
        let fees = val(&e.fees);
        let tolerance = e.quantity.as_deref().map(half_unit).unwrap_or_default();
        if let Some((time, kind)) = prior.insert(&e.symbol, (&e.timestamp, e.kind)) {
            if time == e.timestamp && kind != e.kind && warned.insert((&e.symbol, &e.timestamp)) {
                output.warnings.push(format!("{} has different event types at the same time. Calculations use their saved order. Edit the times if a different order is needed.", e.symbol));
            }
        }
        if let Some(warning) = e.amount_warning() {
            output.warnings.push(warning);
        }
        match e.kind {
            Kind::OpeningBalance => {
                if account_exists {
                    return Err(error("Earlier activity overlaps this opening balance."));
                }
                p.quantity = quantity;
                p.basis = e.gross();
                p.invested = p.basis;
                p.precision = tolerance;
                p.incomplete = true;
            }
            Kind::Buy => {
                p.quantity += quantity;
                let cost = e.gross() + fees;
                p.basis += cost;
                p.invested += cost;
                if p.basis > D::from(1_000_000_000_000_000_000u64) {
                    return Err(error("Cost basis exceeds supported range."));
                }
                p.precision += tolerance;
            }
            Kind::Sell => {
                let effective = if e.close_position == Some(true) && p.quantity != quantity {
                    let difference = (p.quantity - quantity).abs();
                    if p.quantity.is_zero()
                        || difference > p.precision + tolerance
                        || difference > p.quantity / D::from(1_000)
                    {
                        return Err(error("The closing quantity differs from the tracked position beyond rounding tolerance."));
                    }
                    output.adjustments.push(Adjustment {
                        id: e.transaction_id.clone(),
                        original: quantity,
                        effective: p.quantity,
                        confirmed: row
                            .reconciled_quantity
                            .as_deref()
                            .is_some_and(|q| num(q) == p.quantity),
                    });
                    p.quantity
                } else {
                    quantity
                };
                if effective > p.quantity {
                    return Err(error("Sale exceeds available shares. Check personal trade ownership and earlier purchases."));
                }
                let removed = if effective == p.quantity {
                    p.basis
                } else {
                    round_scale(p.basis * (effective / p.quantity)).min(p.basis)
                };
                p.realized += e.gross() - fees - removed;
                p.basis -= removed;
                p.quantity -= effective;
                p.precision = if p.quantity.is_zero() {
                    D::ZERO
                } else {
                    p.precision + tolerance
                };
            }
            Kind::Dividend => p.dividends += e.gross() - fees,
            Kind::Split => {
                if p.quantity.is_zero() {
                    return Err(error("No open shares to split."));
                }
                let ratio = val(&e.split_ratio);
                let next = p
                    .quantity
                    .checked_mul(ratio)
                    .map(round_scale)
                    .ok_or_else(|| error("Split exceeded supported precision."))?;
                if next.is_zero() || next > D::from(1_000_000_000_000u64) {
                    return Err(error("Split exceeded supported quantity."));
                }
                p.quantity = next;
                p.precision = round_scale(p.precision * ratio).min(p.quantity);
                p.splits.push((e.timestamp.clone(), ratio));
            }
        }
        if p.quantity > D::from(1_000_000_000_000u64) {
            return Err(error("Holding exceeds supported quantity."));
        }
    }
    Ok(output)
}

#[derive(Clone, Serialize)]
pub struct CsvRow {
    pub row: u64,
    pub event: Option<Event>,
    pub error: Option<String>,
}

pub fn parse_csv(text: &str) -> Result<Vec<CsvRow>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let expected: Vec<_> = HEADER.split(',').collect();
    if reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .collect::<Vec<_>>()
        != expected
    {
        return Err("CSV columns do not match the Portfolio template. Download a fresh template from Imports.".into());
    }
    let rows: Vec<_> = reader
        .deserialize::<Event>()
        .enumerate()
        .map(|(index, item)| {
            let row = index as u64 + 2;
            match item.map_err(|e| e.to_string()).and_then(Event::validate) {
                Ok(event) => CsvRow {
                    row,
                    event: Some(event),
                    error: None,
                },
                Err(error) => CsvRow {
                    row,
                    event: None,
                    error: Some(error),
                },
            }
        })
        .collect();
    if rows.len() > 20_000 {
        return Err("Split the import into batches of up to 20,000 transactions.".into());
    }
    Ok(rows)
}
