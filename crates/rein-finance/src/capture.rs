//! Data-tool capture layer: every pull lands in the CAS at retrieval time
//! with its stamps in the captures table — which is what makes past-cutoff
//! epochs possible at all (invariant 13). PIT enforcement lives HERE, at the
//! tool boundary where it is real:
//!
//! - **eval mode**: live pulls always refuse (frozen corpus only);
//! - **production mode with a past cutoff**: live pulls refuse — a live API
//!   serves current-vintage figures no query parameter can unwind; only
//!   own-CAS captures with `retrieved_at ≤ source_cutoff` are admissible;
//! - **production, cutoff ≥ now**: live pulls permitted, captured, stamped.

use crate::datum::{AsOfBasis, Stamped};
use crate::fmp::{EquityEndpoint, FmpClient, FmpError};
use rein_core::canon::Sha256Digest;
use rein_core::entities::Epoch;
use rein_core::time::Timestamp;
use rein_runtime::cas::Cas;
use rein_runtime::store::{CaptureRow, Store, StoreError};
use serde_json::Value;
use std::sync::Arc;

/// Captures per host cap (invariant 19): syndication must not read as
/// corroboration; the cap and the counts are both reported.
pub const MAX_CAPTURES_PER_HOST: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("PIT refusal: {mode} epoch `{epoch}` with source_cutoff {cutoff} does not permit live pulls — a past-cutoff epoch may read only its own CAS captures with retrieved_at ≤ cutoff (invariant 13; live vendor data is current-vintage and no query parameter can unwind restatements)")]
    LivePullRefused {
        mode: &'static str,
        epoch: String,
        cutoff: String,
    },
    #[error(transparent)]
    Fmp(#[from] FmpError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Cas(#[from] rein_runtime::cas::CasError),
    #[error(transparent)]
    Datum(#[from] crate::datum::DatumError),
    #[error("host cap: {host} already has {MAX_CAPTURES_PER_HOST} captures this session — syndication is not corroboration (invariant 19)")]
    HostCapReached { host: String },
    #[error("fetch failed for {url}: {reason}")]
    Fetch { url: String, reason: String },
    #[error("search failed: {0}")]
    Search(String),
}

/// The PIT gate. Every live tool calls this first.
pub fn ensure_live_permitted(epoch: &Epoch, now: Timestamp) -> Result<(), CaptureError> {
    let refuse = |mode: &'static str| CaptureError::LivePullRefused {
        mode,
        epoch: epoch.epoch_ref.as_str().to_string(),
        cutoff: epoch.source_cutoff.canonical(),
    };
    match epoch.pit_mode {
        rein_core::context_pack::PitMode::Eval => Err(refuse("eval")),
        rein_core::context_pack::PitMode::Production => {
            if epoch.source_cutoff >= now {
                Ok(())
            } else {
                Err(refuse("production (past cutoff)"))
            }
        }
    }
}

/// Own-CAS reads under a past cutoff: admissible iff retrieved before it.
pub fn capture_admissible(row: &CaptureRow, epoch: &Epoch) -> bool {
    row.retrieved_at <= epoch.source_cutoff
}

pub struct CaptureStore<'a> {
    pub store: &'a mut Store,
    pub cas: Cas,
}

#[derive(Debug, Clone)]
pub struct PullResult {
    pub digest: Sha256Digest,
    pub rows: Vec<Stamped>,
    pub served_version: Option<String>,
}

impl<'a> CaptureStore<'a> {
    pub fn new(store: &'a mut Store, cas: Cas) -> Self {
        Self { store, cas }
    }

    /// Pull one FMP endpoint for one symbol under an epoch, capture the raw
    /// bytes, extract stamped figures. Refuses per the PIT gate.
    pub fn pull_equity(
        &mut self,
        client: &FmpClient,
        endpoint: EquityEndpoint,
        symbol: &str,
        epoch: &Epoch,
        now: Timestamp,
    ) -> Result<PullResult, CaptureError> {
        ensure_live_permitted(epoch, now)?;

        let params: Vec<(&str, &str)> = match endpoint {
            EquityEndpoint::IncomeStatement
            | EquityEndpoint::BalanceSheet
            | EquityEndpoint::CashFlow => {
                vec![("symbol", symbol), ("period", "annual"), ("limit", "5")]
            }
            EquityEndpoint::AnalystEstimates => vec![
                ("symbol", symbol),
                ("period", "annual"),
                ("page", "0"),
                ("limit", "4"),
            ],
            _ => vec![("symbol", symbol)],
        };
        let (bytes, served_version) = client.get_raw(endpoint.path(), &params)?;
        let digest = self.cas.put(&bytes)?;

        let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let rows = extract_stamped(endpoint, symbol, &parsed, now)?;
        let as_of = rows.first().map(|r| (r.as_of, r.as_of_basis));

        self.store.insert_capture(&CaptureRow {
            digest: digest.clone(),
            tool: endpoint.tool_name().to_string(),
            params: format!("symbol={symbol}&endpoint={}", endpoint.path()),
            provider: "Financial Modeling Prep".to_string(),
            media_type: "application/json".to_string(),
            as_of: as_of.map(|(t, _)| t),
            as_of_basis: as_of.map(|(_, b)| format!("{b:?}").to_lowercase()),
            retrieved_at: now,
            url: None,
            host: Some("financialmodelingprep.com".to_string()),
            note: Some(format!(
                "fmp:{}:{symbol}{}",
                endpoint.path(),
                served_version
                    .as_deref()
                    .map(|v| format!(" served-version={v}"))
                    .unwrap_or_default()
            )),
        })?;

        Ok(PullResult {
            digest,
            rows,
            served_version,
        })
    }

    /// Capture a fetched web page (research.visit). Applies the host cap.
    pub fn capture_page(
        &mut self,
        url: &str,
        bytes: &[u8],
        media_type: &str,
        epoch: &Epoch,
        now: Timestamp,
    ) -> Result<Sha256Digest, CaptureError> {
        ensure_live_permitted(epoch, now)?;
        let host = url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or("unknown")
            .to_lowercase();
        let host_count = self
            .store
            .list_captures()?
            .iter()
            .filter(|c| c.host.as_deref() == Some(host.as_str()))
            .count() as u32;
        if host_count >= MAX_CAPTURES_PER_HOST {
            return Err(CaptureError::HostCapReached { host });
        }
        let digest = self.cas.put(bytes)?;
        self.store.insert_capture(&CaptureRow {
            digest: digest.clone(),
            tool: "research.visit".to_string(),
            params: url.to_string(),
            provider: host.clone(),
            media_type: media_type.to_string(),
            as_of: Some(now),
            as_of_basis: Some("retrieval_time".to_string()),
            retrieved_at: now,
            url: Some(url.to_string()),
            host: Some(host),
            note: None,
        })?;
        Ok(digest)
    }
}

/// Extract stamped figures per endpoint. Where the provider payload carries
/// its own temporal field, the as-of basis is `Provider`; where it does not
/// (profile), the honest stamp is retrieval time and the record says so.
fn extract_stamped(
    endpoint: EquityEndpoint,
    symbol: &str,
    payload: &Value,
    retrieved_at: Timestamp,
) -> Result<Vec<Stamped>, CaptureError> {
    let tool = endpoint.tool_name();
    let mut out = Vec::new();
    let first = payload.get(0).unwrap_or(payload);

    let f = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64);
    let provider = "Financial Modeling Prep";

    match endpoint {
        EquityEndpoint::Quote => {
            // Provider timestamp (unix seconds) is the as-of — never local.
            let ts = first
                .get("timestamp")
                .and_then(Value::as_i64)
                .map(|s| Timestamp::from_unix_millis(s * 1000));
            let as_of = ts.map(|t| (t, AsOfBasis::Provider));
            for (name, key, unit) in [
                ("price", "price", "ccy/share"),
                ("market_cap", "marketCap", "ccy"),
                ("shares_outstanding", "sharesOutstanding", "shares"),
            ] {
                if let Some(v) = f(first, key) {
                    out.push(Stamped::new(
                        tool,
                        format!("{symbol}.{name}"),
                        v,
                        unit,
                        as_of,
                        provider,
                        retrieved_at,
                        None,
                    )?);
                }
            }
        }
        EquityEndpoint::Profile => {
            // No provider as-of exists on profile: the stamp is retrieval
            // time, stated as such — and inadmissible as history (inv 13).
            let as_of = Some((retrieved_at, AsOfBasis::RetrievalTime));
            for (name, key, unit) in [
                ("beta", "beta", "ratio"),
                ("market_cap", "marketCap", "ccy"),
            ] {
                if let Some(v) = f(first, key) {
                    out.push(Stamped::new(
                        tool,
                        format!("{symbol}.{name}"),
                        v,
                        unit,
                        as_of,
                        provider,
                        retrieved_at,
                        None,
                    )?);
                }
            }
        }
        EquityEndpoint::IncomeStatement
        | EquityEndpoint::BalanceSheet
        | EquityEndpoint::CashFlow => {
            let keys: &[(&str, &str)] = match endpoint {
                EquityEndpoint::IncomeStatement => &[("revenue", "revenue"), ("ebitda", "ebitda")],
                EquityEndpoint::BalanceSheet => &[
                    ("total_debt", "totalDebt"),
                    ("cash", "cashAndCashEquivalents"),
                    ("minority_interest", "minorityInterest"),
                ],
                _ => &[("free_cash_flow", "freeCashFlow")],
            };
            if let Some(rows) = payload.as_array() {
                for row in rows {
                    let period_end = row
                        .get("date")
                        .and_then(Value::as_str)
                        .and_then(|d| Timestamp::parse(&format!("{d}T00:00:00Z")).ok());
                    let Some(pe) = period_end else { continue };
                    for (name, key) in keys {
                        if let Some(v) = f(row, key) {
                            out.push(Stamped::new(
                                tool,
                                format!("{symbol}.{name}.{}", pe.canonical()),
                                v,
                                "ccy",
                                Some((pe, AsOfBasis::Provider)),
                                provider,
                                retrieved_at,
                                Some(pe),
                            )?);
                        }
                    }
                }
            }
        }
        EquityEndpoint::AnalystEstimates => {
            if let Some(rows) = payload.as_array() {
                for row in rows {
                    let date = row
                        .get("date")
                        .and_then(Value::as_str)
                        .and_then(|d| Timestamp::parse(&format!("{d}T00:00:00Z")).ok());
                    let Some(d) = date else { continue };
                    if let Some(v) = f(row, "revenueAvg") {
                        out.push(Stamped::new(
                            tool,
                            format!("{symbol}.revenue_estimate.{}", d.canonical()),
                            v,
                            "ccy",
                            Some((d, AsOfBasis::Provider)),
                            provider,
                            retrieved_at,
                            Some(d),
                        )?);
                    }
                }
            }
        }
        EquityEndpoint::PricesEod => {
            if let Some(rows) = payload.as_array() {
                for row in rows.iter().take(30) {
                    let date = row
                        .get("date")
                        .and_then(Value::as_str)
                        .and_then(|d| Timestamp::parse(&format!("{d}T00:00:00Z")).ok());
                    let (Some(d), Some(close)) =
                        (date, f(row, "close").or_else(|| f(row, "price")))
                    else {
                        continue;
                    };
                    out.push(Stamped::new(
                        tool,
                        format!("{symbol}.close.{}", d.canonical()),
                        close,
                        "ccy/share",
                        Some((d, AsOfBasis::Provider)),
                        provider,
                        retrieved_at,
                        Some(d),
                    )?);
                }
            }
        }
    }
    Ok(out)
}

// ---- SearXNG search ---------------------------------------------------------

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub content: String,
}

pub struct SearxClient {
    base: String,
    agent: ureq::Agent,
}

impl SearxClient {
    pub fn new(base_url: &str) -> Result<Self, CaptureError> {
        let tls = native_tls::TlsConnector::new()
            .map_err(|e| CaptureError::Search(format!("tls: {e}")))?;
        Ok(Self {
            base: base_url.trim_end_matches('/').to_string(),
            agent: ureq::AgentBuilder::new()
                .tls_connector(Arc::new(tls))
                .timeout(std::time::Duration::from_secs(20))
                .build(),
        })
    }

    pub fn search(&self, query: &str, max: usize) -> Result<Vec<SearchHit>, CaptureError> {
        let resp = self
            .agent
            .get(&format!("{}/search", self.base))
            .set("user-agent", "rein-finance/0.1")
            .query("q", query)
            .query("format", "json")
            .call()
            .map_err(|e| CaptureError::Search(e.to_string()))?;
        let body: Value = resp
            .into_json()
            .map_err(|e| CaptureError::Search(e.to_string()))?;
        let hits = body
            .get("results")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .take(max)
                    .collect()
            })
            .unwrap_or_default();
        Ok(hits)
    }
}

/// research.visit: fetch today's bytes of a URL (production mode). The
/// temporal honesty is in the stamps, not the fetch.
pub fn fetch_url(url: &str) -> Result<(Vec<u8>, String), CaptureError> {
    let tls = native_tls::TlsConnector::new().map_err(|e| CaptureError::Fetch {
        url: url.to_string(),
        reason: format!("tls: {e}"),
    })?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let resp = agent
        .get(url)
        .set("user-agent", "rein-finance/0.1 (local research harness)")
        .call()
        .map_err(|e| CaptureError::Fetch {
            url: url.to_string(),
            reason: e.to_string(),
        })?;
    let media = resp
        .header("content-type")
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .unwrap_or("application/octet-stream")
        .to_string();
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| CaptureError::Fetch {
            url: url.to_string(),
            reason: e.to_string(),
        })?;
    Ok((bytes, media))
}
