use crate::clock::Stamp;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

/// Failed logins, counted three ways, so neither rotating IPs nor rotating
/// emails gets around the lock (in memory, or in the database with
/// `CACHE_STORE=database` so every server shares the counts):
///
/// - 5 per email and IP a minute, the lock a user who mistyped runs into;
/// - 20 per email in 15 minutes, whatever the IP (guessing one account from
///   many addresses);
/// - 50 per IP in 15 minutes, whatever the email (trying many accounts).
pub(crate) struct LoginThrottle {
    pair: Throttle,
    account: Throttle,
    ip: Throttle,
    /// Shared counters instead of the in-memory ones.
    db: Option<crate::db::Db>,
}

impl LoginThrottle {
    pub fn new(db: Option<crate::db::Db>) -> Self {
        Self {
            pair: Throttle::new(5, Duration::from_secs(60)),
            account: Throttle::new(20, Duration::from_secs(15 * 60)),
            ip: Throttle::new(50, Duration::from_secs(15 * 60)),
            db,
        }
    }

    fn keys(email: &str, ip: Option<IpAddr>) -> [String; 3] {
        let email = email.trim().to_lowercase();
        let ip = ip.map(|ip| ip.to_string()).unwrap_or_default();
        [format!("{email}|{ip}"), email, ip]
    }

    /// The three counters with their key in the shared table.
    fn named<'a>(&'a self, keys: &'a [String; 3]) -> [(&'a Throttle, String); 3] {
        [
            (&self.pair, format!("login:pair:{}", keys[0])),
            (&self.account, format!("login:account:{}", keys[1])),
            (&self.ip, format!("login:ip:{}", keys[2])),
        ]
    }

    /// Seconds until this email may try again from this IP, if it is locked out.
    pub async fn blocked_for(&self, email: &str, ip: Option<IpAddr>) -> Option<u64> {
        let keys = Self::keys(email, ip);
        let Some(db) = &self.db else {
            return [
                self.pair.blocked_for(&keys[0]),
                self.account.blocked_for(&keys[1]),
                self.ip.blocked_for(&keys[2]),
            ]
            .into_iter()
            .flatten()
            .max();
        };
        let mut longest = None;
        for (throttle, key) in self.named(&keys) {
            match crate::counters::read(db, &key).await {
                Ok(Some((count, ends))) if count >= throttle.max_attempts => {
                    let wait = crate::counters::seconds_until(ends);
                    longest = Some(longest.map_or(wait, |l: u64| l.max(wait)));
                }
                Ok(_) => {}
                Err(err) => tracing::warn!(error = ?err, "could not read the login lock"),
            }
        }
        longest
    }

    pub async fn fail(&self, email: &str, ip: Option<IpAddr>) {
        let keys = Self::keys(email, ip);
        let Some(db) = &self.db else {
            self.pair.fail(&keys[0]);
            self.account.fail(&keys[1]);
            self.ip.fail(&keys[2]);
            return;
        };
        for (throttle, key) in self.named(&keys) {
            if let Err(err) = crate::counters::increment(db, &key, throttle.window).await {
                tracing::warn!(error = ?err, "could not count a failed login");
            }
        }
    }

    /// After a successful login. The IP's count stays: one account the
    /// guesser owns shouldn't reset their tries at the others.
    pub async fn clear(&self, email: &str, ip: Option<IpAddr>) {
        let keys = Self::keys(email, ip);
        let Some(db) = &self.db else {
            self.pair.clear(&keys[0]);
            self.account.clear(&keys[1]);
            return;
        };
        for (_, key) in self.named(&keys).into_iter().take(2) {
            if let Err(err) = crate::counters::clear(db, &key).await {
                tracing::warn!(error = ?err, "could not clear the login lock");
            }
        }
    }
}

/// Counts failed attempts per key in memory.
pub(crate) struct Throttle {
    max_attempts: u32,
    window: Duration,
    attempts: Mutex<HashMap<String, (u32, Stamp)>>,
}

impl Throttle {
    pub fn new(max_attempts: u32, window: Duration) -> Self {
        Self {
            max_attempts,
            window,
            attempts: Mutex::new(HashMap::new()),
        }
    }

    /// Seconds until `key` may try again, if it is locked out.
    pub fn blocked_for(&self, key: &str) -> Option<u64> {
        let attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        let (count, since) = attempts.get(key)?;
        let elapsed = since.elapsed();
        (*count >= self.max_attempts && elapsed < self.window)
            .then(|| (self.window - elapsed).as_secs().max(1))
    }

    pub fn fail(&self, key: &str) {
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        attempts.retain(|_, (_, since)| since.elapsed() < self.window);
        let entry = attempts.entry(key.to_owned()).or_insert((0, Stamp::now()));
        entry.0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.attempts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn locks_an_account_across_ips_and_an_ip_across_accounts() {
        let throttle = LoginThrottle::new(None);
        for i in 0..20 {
            throttle
                .fail("Alex@example.com", format!("10.0.0.{i}").parse().ok())
                .await;
        }
        let other_ip = "10.9.9.9".parse().ok();
        assert!(
            throttle
                .blocked_for("alex@example.com", other_ip)
                .await
                .is_some()
        );
        assert!(
            throttle
                .blocked_for("ben@example.com", other_ip)
                .await
                .is_none()
        );

        let throttle = LoginThrottle::new(None);
        let ip = "10.0.0.1".parse().ok();
        for i in 0..50 {
            throttle.fail(&format!("user{i}@example.com"), ip).await;
        }
        assert!(throttle.blocked_for("new@example.com", ip).await.is_some());
        let elsewhere = "10.0.0.2".parse().ok();
        assert!(
            throttle
                .blocked_for("new@example.com", elsewhere)
                .await
                .is_none()
        );
    }

    #[test]
    fn locks_out_after_max_attempts() {
        let throttle = Throttle::new(3, Duration::from_secs(60));
        for _ in 0..2 {
            throttle.fail("a");
        }
        assert!(throttle.blocked_for("a").is_none());
        throttle.fail("a");
        assert!(throttle.blocked_for("a").unwrap() <= 60);
        assert!(throttle.blocked_for("b").is_none());
        throttle.clear("a");
        assert!(throttle.blocked_for("a").is_none());
    }

    #[test]
    fn windows_expire() {
        let throttle = Throttle::new(1, Duration::from_millis(1));
        throttle.fail("a");
        std::thread::sleep(Duration::from_millis(5));
        assert!(throttle.blocked_for("a").is_none());
    }
}
