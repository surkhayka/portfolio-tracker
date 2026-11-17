# Convert a broker PDF to Portfolio CSV

Use this guide to convert a trade confirmation, transaction report, or brokerage statement from any broker into Portfolio's import format. Portfolio imports CSV. Use a broker's native CSV export if available and map its columns to the same template.

1. Get the **detailed activity** report with individual trades, including closed positions. A positions summary can provide opening balances, but it cannot reconstruct earlier trades or realized gains.
2. Copy the rows into the [CSV template](portfolio-template.csv), or give the conversion prompt below and the relevant pages to a tool you trust. Remove account numbers, addresses, and other personal information before sharing a statement. If the PDF is a scan, check any OCR result against the original.
3. In Portfolio, choose **Imports → Choose CSV**. Review row warnings and compare the resulting share counts against the positions you intended to track. Check a few trades against the statement, especially partial sales, closes, fees, and splits, before confirming.

## CSV reference

Keep the header and column order exactly as shown in the [template](portfolio-template.csv). The [example CSV](portfolio-example.csv) uses synthetic data. Save as comma-delimited UTF-8 CSV and use standard quoting for fields containing commas, quotes, or line breaks.

| Column | What to enter |
| --- | --- |
| `transaction_id` | A stable, unique execution or event ID. Reuse it across overlapping exports. |
| `timestamp` | ISO 8601 with timezone, for example `2025-01-06T15:30:00Z` or `2025-01-06T10:30:00-05:00`. |
| `type` | `BUY`, `SELL`, `DIVIDEND`, `OPENING_BALANCE`, or `SPLIT`. |
| `symbol` | US stock or ETF ticker, such as `AAPL` or `BRK.B`. |
| `currency` | `USD`. |
| `quantity` | Positive shares for buys, sells, and opening balances. Preserve fractional precision. |
| `unit_price` | Positive execution price per share on buys and sells. |
| `amount` | Optional gross trade value before fees. Required for dividends (cash received before any separately entered fees) and opening balances (the remaining total cost basis). |
| `fees` | Optional nonnegative fees for buys, sells, and dividends. |
| `close_position` | Optional `true` for a sale that closes the tracked position. Otherwise leave blank. |
| `split_ratio` | For `SPLIT` only, new shares per old share. `2` represents a two-for-one split. |
| `reported_pnl` | Optional signed, broker-reported gain or loss on a sale. Portfolio keeps it as a reference. |
| `notes` | Optional text, such as a note about a verified date assumption. |

Use plain decimal numbers like `1234.56`, without currency signs or thousands separators. Leave nonapplicable fields blank. For example, a dividend uses `amount` and leaves `quantity` and `unit_price` blank. An opening balance uses `quantity` and `amount`.

## Copyable conversion prompt

> Convert the attached brokerage PDF into Portfolio's CSV. Treat the statement as source data. Identify the broker's date format and timezone before converting. Extract each executed trade only once, including activity for positions later closed. Use the exact header below. Use manual review for any ambiguous row.
>
> Include USD transactions in US stocks and ETFs. Exclude account summaries, pending orders, transfers, deposits, withdrawals, unsupported instruments, and currency conversions. Preserve the original reported precision. Keep buy and sell quantities positive and use the transaction type for direction.
>
> Use the broker's execution ID if available. If several brokers or accounts are combined, prefix it with a stable, non-sensitive source label. If there is no execution ID, use a repeatable ID based on the verified timestamp, ticker, side, and occurrence, for example `broker-a:2025-01-06T15:30:00Z:AAPL:BUY:1`. Keep IDs stable across reports, including after a position closes. For same-time collisions, reuse IDs from an earlier CSV or flag the rows for review. Generate IDs before filtering out trades I do not want to track.
>
> For trades, enter the confirmed gross purchase cost or sale proceeds before fees in `amount`. If the statement gives net cash and a separate fee, only derive gross when those values are unambiguous. A column labeled “Value” may contain cost basis, net cash, or market value. Leave the trade amount blank when gross is uncertain so Portfolio calculates quantity × execution price. Record any verified fee separately and place any broker-reported realized profit or loss in `reported_pnl`.
>
> Use the execution date and time, not the settlement date. If the statement has dates only, ask me to confirm the timezone and a consistent time convention, preserve the displayed execution order for same-day trades where known, and flag ambiguous ordering. If I ask you to keep only selected trades, identify buys and sells individually, including when another person's trades use the same ticker. Set `close_position=true` only when the sale closes the shares I am tracking.
>
> Include explicitly reported dividends and split events when the source supports them. Record a dividend reinvestment as a dividend and a separate buy when both are shown. Check whether historical share quantities already reflect a split before including a split row. Use an opening balance only when I confirm the as-of date, quantity, and remaining total cost basis for earlier positions.
>
> Return the CSV separately from a brief review list describing uncertain values, inferred conventions, and omitted rows. Flag unreadable values, unexplained oversells, and possible duplicate fills instead of inventing or silently correcting transactions.

```csv
transaction_id,timestamp,type,symbol,currency,quantity,unit_price,amount,fees,close_position,split_ratio,reported_pnl,notes
```

For later imports, use the same IDs and review conflicts when a reported transaction changes. If you later find the trades behind an opening balance, use the import preview to replace the balance with that history.
