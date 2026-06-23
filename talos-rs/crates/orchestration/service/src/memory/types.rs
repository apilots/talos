use chrono::NaiveDate;
use talos_error::{TalosError, TalosResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryDate(NaiveDate);

impl MemoryDate {
    pub fn parse(value: &str) -> TalosResult<Self> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(Self)
            .map_err(|_| TalosError::ValidationError("memory date must use YYYY-MM-DD".to_string()))
    }

    pub fn file_stem(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryListCommand {
    pub limit: Option<usize>,
    pub before_date: Option<MemoryDate>,
    pub date_from: Option<MemoryDate>,
    pub date_to: Option<MemoryDate>,
}

#[derive(Debug, Clone)]
pub struct MemorySearchCommand {
    pub query: String,
    pub limit: Option<usize>,
    pub date_from: Option<MemoryDate>,
    pub date_to: Option<MemoryDate>,
}
