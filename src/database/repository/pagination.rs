#[derive(Debug, Clone)]
pub struct PageRequest {
    pub page: u64,
    pub size: u64,
    pub sort: Vec<Sort>,
}

impl PageRequest {
    pub fn new(page: u64, size: u64) -> Self {
        Self {
            page,
            size,
            sort: Vec::new(),
        }
    }

    pub fn with_sort(mut self, sort: Vec<Sort>) -> Self {
        self.sort = sort;
        self
    }

    pub fn offset(&self) -> u64 {
        self.page * self.size
    }

    pub fn limit(&self) -> u64 {
        self.size
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            page: 0,
            size: 20,
            sort: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sort {
    pub field: String,
    pub direction: SortDirection,
}

impl Sort {
    pub fn asc(field: String) -> Self {
        Self {
            field,
            direction: SortDirection::Asc,
        }
    }

    pub fn desc(field: String) -> Self {
        Self {
            field,
            direction: SortDirection::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct Page<T> {
    pub content: Vec<T>,
    pub total_elements: u64,
    pub total_pages: u64,
    pub size: u64,
    pub number: u64,
}

impl<T> Page<T> {
    pub fn new(content: Vec<T>, total_elements: u64, page_request: &PageRequest) -> Self {
        let total_pages = if page_request.size > 0 {
            (total_elements + page_request.size - 1) / page_request.size
        } else {
            0
        };

        Self {
            content,
            total_elements,
            total_pages,
            size: page_request.size,
            number: page_request.page,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    pub fn has_content(&self) -> bool {
        !self.is_empty()
    }
}