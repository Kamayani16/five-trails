//! Connection caps (global and per client address) and a per-connection message rate limiter.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct ConnLimiter {
    per_ip: Mutex<HashMap<IpAddr, usize>>,
    max_total: usize,
    max_per_ip: usize,
}

/// Held for the life of a connection; releases its slot on drop.
pub struct ConnGuard {
    limiter: Arc<ConnLimiter>,
    key: IpAddr,
}

/// IPv6 clients can rotate through a whole /64, so count them per /64.
fn bucket(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
    }
}

impl ConnLimiter {
    pub fn new(max_total: usize, max_per_ip: usize) -> Arc<Self> {
        Arc::new(Self { per_ip: Mutex::new(HashMap::new()), max_total, max_per_ip })
    }

    pub fn try_acquire(self: &Arc<Self>, ip: IpAddr) -> Option<ConnGuard> {
        let key = bucket(ip);
        let mut map = self.per_ip.lock().unwrap_or_else(|e| e.into_inner());
        let total: usize = map.values().sum();
        let mine = map.get(&key).copied().unwrap_or(0);
        if total >= self.max_total || mine >= self.max_per_ip {
            return None;
        }
        map.insert(key, mine + 1);
        Some(ConnGuard { limiter: Arc::clone(self), key })
    }

    pub fn active(&self) -> usize {
        self.per_ip.lock().unwrap_or_else(|e| e.into_inner()).values().sum()
    }
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        let mut map = self.limiter.per_ip.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = map.get_mut(&self.key) {
            *n -= 1;
            if *n == 0 {
                map.remove(&self.key);
            }
        }
    }
}

/// Classic token bucket: `capacity` burst, refilled at `rate` tokens per second.
pub struct TokenBucket {
    capacity: f64,
    tokens: f64,
    rate: f64,
    last: Instant,
}

impl TokenBucket {
    pub fn new(capacity: f64, rate: f64) -> Self {
        Self { capacity, tokens: capacity, rate, last: Instant::now() }
    }

    pub fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * self.rate).min(self.capacity);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_per_address_and_releases_on_drop() {
        let lim = ConnLimiter::new(10, 2);
        let ip: IpAddr = "203.0.113.5".parse().unwrap();
        let a = lim.try_acquire(ip).unwrap();
        let _b = lim.try_acquire(ip).unwrap();
        assert!(lim.try_acquire(ip).is_none());
        drop(a);
        assert!(lim.try_acquire(ip).is_some());
    }

    #[test]
    fn ipv6_counts_per_slash_64() {
        let lim = ConnLimiter::new(10, 1);
        let _a = lim.try_acquire("2001:db8::1".parse().unwrap()).unwrap();
        assert!(lim.try_acquire("2001:db8::ffff".parse().unwrap()).is_none());
        assert!(lim.try_acquire("2001:db8:0:1::1".parse().unwrap()).is_some());
    }

    #[test]
    fn bucket_limits_bursts() {
        let mut b = TokenBucket::new(3.0, 0.0);
        assert!(b.take() && b.take() && b.take());
        assert!(!b.take());
    }
}
