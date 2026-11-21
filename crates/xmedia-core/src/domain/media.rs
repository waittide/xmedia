use serde::{Deserialize, Serialize};

use crate::domain::error::{DomainError, DomainResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProbedAudioStream {
    pub stream_index: u32,
    pub codec: String,
    pub channels: u8,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProbedSubtitleStream {
    pub stream_index: u32,
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_text_based: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

impl SortOrder {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            other => Err(DomainError::Validation(format!(
                "unknown sort order: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagedResult<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}

impl<T> PagedResult<T> {
    pub fn new(items: Vec<T>, total: u64, page: u32, page_size: u32) -> Self {
        Self {
            items,
            total,
            page,
            page_size,
        }
    }

    pub fn has_next(&self) -> bool {
        self.page >= 1
            && self.page_size > 0
            && u64::from(self.page).saturating_mul(u64::from(self.page_size)) < self.total
    }

    pub fn has_prev(&self) -> bool {
        self.page > 1
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> PagedResult<U> {
        PagedResult {
            items: self.items.into_iter().map(f).collect(),
            total: self.total,
            page: self.page,
            page_size: self.page_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_order_parse_and_roundtrip() {
        assert_eq!(SortOrder::parse("ASC").unwrap(), SortOrder::Asc);
        assert_eq!(SortOrder::parse(" desc ").unwrap(), SortOrder::Desc);
        assert!(SortOrder::parse("random").is_err());
        assert_eq!(SortOrder::Asc.as_str(), "asc");
        assert_eq!(SortOrder::Desc.as_str(), "desc");
    }

    #[test]
    fn paged_result_navigation_and_map() {
        let p1 = PagedResult::new(vec![1u32, 2], 5, 1, 2);
        assert!(!p1.has_prev());
        assert!(p1.has_next());

        let p3 = PagedResult::new(vec![5u32], 5, 3, 2);
        assert!(p3.has_prev());
        assert!(!p3.has_next());

        let mapped = p1.map(|n| n * 10);
        assert_eq!(mapped.items, vec![10, 20]);
        assert_eq!(mapped.total, 5);
    }
}
