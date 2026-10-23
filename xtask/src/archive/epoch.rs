//! One validated source timestamp shared by archive producers and readers.
use super::invalid;
use std::io;

/// Source UTC seconds representable by gzip, USTAR and the supported ZIP profiles.
/// ZIP metadata uses the preceding even second; TAR and gzip retain every second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct PackageEpoch(u32);

impl PackageEpoch {
    /// Validate UTC Unix seconds against the common packaging range.
    pub fn new(seconds: u64) -> io::Result<Self> {
        if !(315_532_800..=u64::from(u32::MAX)).contains(&seconds) {
            return Err(invalid(
                "Package epoch must be between 1980-01-01 UTC and u32::MAX seconds",
            ));
        }
        Ok(Self(seconds as u32))
    }

    /// Exact admitted source UTC seconds.
    pub fn seconds(self) -> u32 {
        self.0
    }

    /// ZIP's explicitly derived two-second timestamp, rounded down.
    pub fn floor_seconds(self) -> u32 {
        self.0 & !1
    }

    pub(super) fn zip_datetime(self) -> zip::DateTime {
        let mut days = (self.floor_seconds() - 315_532_800) / 86_400;
        let mut year = 1980;
        loop {
            let length = if leap(year) { 366 } else { 365 };
            if days < length {
                break;
            }
            days -= length;
            year += 1;
        }
        let months = [
            31,
            if leap(year) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut month = 0;
        while days >= months[month] {
            days -= months[month];
            month += 1;
        }
        let seconds = self.floor_seconds() % 86_400;
        zip::DateTime::from_date_and_time(
            year as u16,
            (month + 1) as u8,
            (days + 1) as u8,
            (seconds / 3600) as u8,
            ((seconds / 60) % 60) as u8,
            (seconds % 60) as u8,
        )
        .expect("validated epoch has a representable UTC ZIP date")
    }
}
fn leap(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
impl TryFrom<u64> for PackageEpoch {
    type Error = io::Error;
    fn try_from(seconds: u64) -> io::Result<Self> {
        Self::new(seconds)
    }
}
impl From<PackageEpoch> for u64 {
    fn from(epoch: PackageEpoch) -> Self {
        u64::from(epoch.seconds())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_utc_dates_bounds_and_two_second_precision() {
        for (seconds, date, time) in [
            (315_532_800, 0x0021, 0),
            (1_609_459_201, 0x5221, 0),
            (951_827_697, 0x285d, 0x645c),   // 2000-02-29 12:34:57 UTC
            (4_107_542_399, 0xf05c, 0xbf7d), // 2100-02-28 23:59:59 UTC (not a leap year)
            (4_294_967_295, 0xfc47, 0x3387), // 2106-02-07 06:28:15 UTC
        ] {
            let epoch = PackageEpoch::new(seconds).unwrap();
            assert_eq!(epoch.zip_datetime().datepart(), date, "{seconds}");
            assert_eq!(epoch.zip_datetime().timepart(), time, "{seconds}");
            assert_eq!(epoch.floor_seconds(), epoch.seconds() & !1);
            assert_eq!(
                serde_json::from_str::<PackageEpoch>(&seconds.to_string()).unwrap(),
                epoch
            );
        }
        for invalid in [0, 315_532_799, 4_294_967_296, u64::MAX] {
            assert!(PackageEpoch::new(invalid).is_err());
            assert!(serde_json::from_str::<PackageEpoch>(&invalid.to_string()).is_err());
        }
        for invalid in ["null", "-1", "1609459201.0", "\"1609459201\"", "{}"] {
            assert!(serde_json::from_str::<PackageEpoch>(invalid).is_err());
        }
    }
}
