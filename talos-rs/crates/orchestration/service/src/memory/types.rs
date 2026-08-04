use chrono::NaiveDate;
use talos_error::{TalosError, TalosResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryDate(NaiveDate);

impl MemoryDate {
    pub fn parse(value: &str) -> TalosResult<Self> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    "memory date must use YYYY-MM-DD".to_string(),
                )
            })
    }

    pub fn file_stem(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }
}
