use talos_types::SessionQuery;

pub const DEFAULT_ADMIN_SESSION_PAGE_SIZE: usize = 20;
pub const MAX_ADMIN_SESSION_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone)]
pub struct AdminSessionPage {
    pub query: SessionQuery,
    pub count_query: SessionQuery,
    pub limit: usize,
    pub offset: usize,
}

impl AdminSessionPage {
    pub fn from_query(mut query: SessionQuery) -> Self {
        let limit = query
            .limit
            .unwrap_or(DEFAULT_ADMIN_SESSION_PAGE_SIZE)
            .clamp(1, MAX_ADMIN_SESSION_PAGE_SIZE);
        let offset = query.offset.unwrap_or(0);
        query.limit = Some(limit);
        query.offset = Some(offset);

        let mut count_query = query.clone();
        count_query.limit = None;
        count_query.offset = None;

        Self {
            query,
            count_query,
            limit,
            offset,
        }
    }
}
