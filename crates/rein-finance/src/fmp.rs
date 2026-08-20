//! FMP client (data.equity.* backing). Blocking ureq with an explicit
//! native-tls connector — the pattern proven in the estate. The API key is
//! never inlined in errors; every surfaced error is redacted.
//!
//! Deviation from design §11 (consume the estate market crate), recorded in
//! the room: rein needs statements/estimates endpoints it does not wrap, and
//! linking it drags a full tokio build through its protocol stack into a blocking
//! client. The consumed *lesson* — provider-time stamping, key redaction,
//! `with_root` fixture override — is kept intact.

use std::sync::Arc;

pub const FMP_API_ROOT: &str = "https://financialmodelingprep.com/stable";
pub const FMP_KEY_ENV: &str = "FMP_API_KEY";

#[derive(Debug, thiserror::Error)]
pub enum FmpError {
    #[error("no FMP credential: set {FMP_KEY_ENV} or configure `fmp` in configRoot secrets.toml")]
    NoCredential,
    #[error("tls: {0}")]
    Tls(String),
    #[error("fmp request failed: {0}")]
    Request(String),
}

pub struct FmpClient {
    key: String,
    agent: ureq::Agent,
    root: String,
}

/// Scrub the key from any surfaced text.
fn redact(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "«redacted:fmp-key»")
    }
}

impl FmpClient {
    pub fn with_key(key: impl Into<String>) -> Result<Self, FmpError> {
        Self::with_key_and_root(key, FMP_API_ROOT)
    }

    /// Base-URL override — tests point this at a local fixture server so no
    /// suite ever needs the network.
    pub fn with_key_and_root(
        key: impl Into<String>,
        root: impl Into<String>,
    ) -> Result<Self, FmpError> {
        let tls = native_tls::TlsConnector::new().map_err(|e| FmpError::Tls(e.to_string()))?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(Arc::new(tls))
            .timeout(std::time::Duration::from_secs(45))
            .build();
        Ok(Self {
            key: key.into(),
            agent,
            root: root.into(),
        })
    }

    /// Resolve the key: env, then configRoot secrets (`fmp`), then an
    /// operator-named env file — refusal otherwise.
    pub fn discover(
        broker: &rein_runtime::workspace::SecretBroker,
        extra_env_file: Option<&std::path::Path>,
    ) -> Result<Self, FmpError> {
        if let Ok(k) = std::env::var(FMP_KEY_ENV) {
            if !k.trim().is_empty() {
                return Self::with_key(k.trim().to_string());
            }
        }
        if let Ok(id) = rein_core::ids::SecretRefId::parse("secret-ref:fmp") {
            if let Some(k) = broker.resolve(&id) {
                return Self::with_key(k.to_string());
            }
        }
        if let Some(path) = extra_env_file {
            if let Ok(text) = std::fs::read_to_string(path) {
                for line in text.lines() {
                    if let Some(rest) = line.trim().strip_prefix(&format!("{FMP_KEY_ENV}=")) {
                        let v = rest.trim().trim_matches('"');
                        if !v.is_empty() {
                            return Self::with_key(v.to_string());
                        }
                    }
                }
            }
        }
        Err(FmpError::NoCredential)
    }

    /// GET an endpoint; returns the raw bytes (the capture artifact) plus the
    /// served-version header when the provider sends one (invariant 8's
    /// per-call service-pin evidence).
    pub fn get_raw(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<(Vec<u8>, Option<String>), FmpError> {
        let url = format!("{}/{}", self.root.trim_end_matches('/'), path);
        let mut req = self
            .agent
            .get(&url)
            .set("user-agent", "rein-finance/0.1")
            .query("apikey", &self.key);
        for (k, v) in params {
            req = req.query(k, v);
        }
        let resp = req
            .call()
            .map_err(|e| FmpError::Request(redact(&e.to_string(), &self.key)))?;
        let served_version = resp
            .header("x-api-version")
            .or_else(|| resp.header("fmp-version"))
            .map(str::to_string);
        let mut bytes = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(16 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| FmpError::Request(redact(&e.to_string(), &self.key)))?;
        Ok((bytes, served_version))
    }
}

/// The equity endpoints rein wraps, each with how its as-of derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquityEndpoint {
    Quote,
    Profile,
    IncomeStatement,
    BalanceSheet,
    CashFlow,
    AnalystEstimates,
    PricesEod,
}

impl EquityEndpoint {
    pub fn path(&self) -> &'static str {
        match self {
            Self::Quote => "quote",
            Self::Profile => "profile",
            Self::IncomeStatement => "income-statement",
            Self::BalanceSheet => "balance-sheet-statement",
            Self::CashFlow => "cash-flow-statement",
            Self::AnalystEstimates => "analyst-estimates",
            Self::PricesEod => "historical-price-eod/full",
        }
    }

    pub fn tool_name(&self) -> &'static str {
        match self {
            Self::Quote => "data.equity.quote",
            Self::Profile => "data.equity.profile",
            Self::IncomeStatement | Self::BalanceSheet | Self::CashFlow => {
                "data.equity.fundamentals"
            }
            Self::AnalystEstimates => "data.equity.estimates",
            Self::PricesEod => "data.equity.prices",
        }
    }

    pub fn all() -> [EquityEndpoint; 7] {
        [
            Self::Quote,
            Self::Profile,
            Self::IncomeStatement,
            Self::BalanceSheet,
            Self::CashFlow,
            Self::AnalystEstimates,
            Self::PricesEod,
        ]
    }
}
