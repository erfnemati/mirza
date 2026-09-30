//! What the account has spent, from the provider's API where it has one.
//! Soniox: GET /v1/usage/summary, one bucket per model plus a total, each with
//! per-day arrays lined up with `days` and amounts as decimal strings.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub today: f64,
    pub month: f64,
    pub month_minutes: f64,
    pub requests: u64,
    /// Spend since the credit date, when a credit is set.
    pub since_credit: Option<f64>,
}

/// A UTC calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Date {
    pub fn today() -> Self {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        Self::from_days(secs.div_euclid(86_400))
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.trim().splitn(3, '-');
        let (y, m, d) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
        ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some(Self { y, m, d })
    }

    /// Days since 1970-01-01 (Howard Hinnant's algorithm).
    pub fn days(self) -> i64 {
        let y = self.y as i64 - (self.m <= 2) as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (self.m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + self.d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        Self { y: (yoe + era * 400 + (m <= 2) as i64) as i32, m, d }
    }

    pub fn first_of_month(self) -> Self {
        Self { d: 1, ..self }
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.days() + n)
    }

    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

/// An HTTPS client that goes through the proxy, if there is one.
pub fn http_client(proxy: Option<&Url>, timeout: Duration) -> Result<reqwest::Client, String> {
    crate::net::init_tls();
    let mut b = reqwest::Client::builder().timeout(timeout).no_proxy();
    if let Some(p) = proxy {
        let url = p.as_str().replacen("socks5://", "socks5h://", 1);
        b = b.proxy(reqwest::Proxy::all(url).map_err(|e| e.to_string())?);
    }
    b.build().map_err(|e| e.to_string())
}

/// Soniox spend this month and today, and since `credit_since` if given.
pub async fn soniox(api_key: &str, proxy: Option<&Url>, credit_since: Option<Date>) -> Result<Usage, String> {
    let http = http_client(proxy, Duration::from_secs(20))?;
    let today = Date::today();
    let tomorrow = today.add_days(1);
    let month = summary(&http, api_key, today.first_of_month(), tomorrow).await?;
    let mut u = Usage {
        month: decimal(&month["total"]["total_cost_usd"]),
        requests: month["total"]["total_num_requests"].as_u64().unwrap_or(0),
        month_minutes: decimal(&month["total"]["total_input_audio_duration_ms"]) / 60_000.0,
        today: cost_on(&month["total"], &today.iso()),
        since_credit: None,
    };
    if let Some(since) = credit_since {
        u.since_credit = Some(if since == today.first_of_month() {
            u.month
        } else {
            decimal(&summary(&http, api_key, since, tomorrow).await?["total"]["total_cost_usd"])
        });
    }
    Ok(u)
}

async fn summary(http: &reqwest::Client, key: &str, from: Date, to: Date) -> Result<Value, String> {
    let url = format!(
        "https://api.soniox.com/v1/usage/summary?start_time={}T00:00:00Z&end_time={}T00:00:00Z",
        from.iso(),
        to.iso()
    );
    let resp = http.get(url).bearer_auth(key).send().await.map_err(|e| format!("usage: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("usage request failed ({})", status.as_u16()));
    }
    resp.json().await.map_err(|e| format!("usage: {e}"))
}

/// One day's cost out of the `days` / `cost_usd` arrays.
fn cost_on(bucket: &Value, day: &str) -> f64 {
    let Some(days) = bucket["days"].as_array() else { return 0.0 };
    days.iter().position(|d| d.as_str() == Some(day)).map(|i| decimal(&bucket["cost_usd"][i])).unwrap_or(0.0)
}

/// Amounts come as decimal strings like "0.0237635000".
fn decimal(v: &Value) -> f64 {
    match v {
        Value::String(s) => s.parse().unwrap_or(0.0),
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// "$0.17", or "$0.0042" for small amounts so they don't show as zero.
pub fn money(v: f64) -> String {
    if v != 0.0 && v.abs() < 0.01 { format!("${v:.4}") } else { format!("${v:.2}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let d = Date { y: 2026, m: 9, d: 30 };
        assert_eq!(Date::from_days(d.days()), d);
        assert_eq!(d.add_days(1).iso(), "2026-10-01");
        assert_eq!(Date { y: 2024, m: 2, d: 28 }.add_days(1).iso(), "2024-02-29");
        assert_eq!(Date { y: 1970, m: 1, d: 1 }.days(), 0);
        assert_eq!(Date::parse("2026-09-01"), Some(Date { y: 2026, m: 9, d: 1 }));
        assert_eq!(Date::parse("2026-13-01"), None);
    }

    #[test]
    fn reads_summary_buckets() {
        let v: Value = serde_json::from_str(
            r#"{"total":{"days":["2026-09-29","2026-09-30"],"cost_usd":["0.01","0.0237635000"],"total_cost_usd":"0.1677260000"}}"#,
        )
        .unwrap();
        assert_eq!(cost_on(&v["total"], "2026-09-30"), 0.0237635);
        assert_eq!(cost_on(&v["total"], "2026-09-01"), 0.0);
        assert_eq!(decimal(&v["total"]["total_cost_usd"]), 0.167726);
        assert_eq!(money(0.167726), "$0.17");
        assert_eq!(money(0.0042), "$0.0042");
    }
}
