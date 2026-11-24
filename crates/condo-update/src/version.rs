//! `MAJOR.MINOR.PATCH` versions. Pre-release suffixes are rejected on purpose: only plain
//! versions are ever published as updates.

use std::fmt;
use std::str::FromStr;

use crate::UpdateError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl FromStr for Version {
    type Err = UpdateError;

    /// Accepts `1.2.3` and `v1.2.3`.
    fn from_str(text: &str) -> Result<Self, UpdateError> {
        let bad = || UpdateError::BadVersion(text.to_owned());
        let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
        let mut next = || -> Result<u64, UpdateError> {
            let part = parts.next().ok_or_else(bad)?;
            // `parse` would also accept a leading `+`; a version is digits only.
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            part.parse().map_err(|_| bad())
        };
        let version = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return Err(bad());
        }
        Ok(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        text.parse().unwrap()
    }

    #[test]
    fn parses_and_orders_numerically() {
        assert_eq!(v("v1.2.3"), v("1.2.3"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert!(v("0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("1.0.10") > v("1.0.9"));
    }

    #[test]
    fn rejects_anything_that_is_not_plain_major_minor_patch() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1.2.3-build.5",
            "1.2.3+meta",
            "+1.2.3",
            "1..3",
            " 1.2.3",
            "-1.2.3",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad:?}");
        }
    }
}
