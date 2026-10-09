//! Public catalog metadata with fixed live origins and a fixture transport seam.
//!
//! Catalog responses are bounded before typed projection. Playback sources, subscriber
//! credentials and license URLs have no interface here. Provider text is display text;
//! callers render it as escaped text and keep decoded image limits at their renderer.
//! Debug output and errors omit provider payloads and identifiers.
#![forbid(unsafe_code)]

use std::future::Future;
mod discovery;
mod discovery_model;
mod discovery_wire;
mod flight;
mod model;
mod record_json;
mod wire;
pub use discovery_model::*;
pub use model::*;
use wire::{WireMedia, WireOptions, WirePage, WireSearch, check_json_bounds, check_text};

mod transport;
pub use transport::HttpTransport;

#[cfg(test)]
mod transport_tests;

/// Maximum decoded body accepted for one public catalog/discovery response.
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub struct Request {
    pub url: url::Url,
}

pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Unavailable,
    InvalidResponse,
    InvalidRequest,
    HttpStatus(u16),
    ResponseTooLarge,
    Deadline,
    Busy,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("catalog is unavailable"),
            Self::InvalidResponse => formatter.write_str("catalog response is invalid"),
            Self::InvalidRequest => formatter.write_str("catalog request is invalid"),
            Self::HttpStatus(status) => write!(formatter, "catalog returned HTTP {status}"),
            Self::ResponseTooLarge => {
                formatter.write_str("catalog response exceeds the size limit")
            }
            Self::Deadline => formatter.write_str("catalog request exceeded its deadline"),
            Self::Busy => formatter.write_str("catalog request capacity is full"),
        }
    }
}

impl std::error::Error for Error {}

/// The HTTP/fixture seam. Live adapters bound reads before producing a response.
/// Errors retain no request URLs, provider payloads or underlying error chains.
pub trait RequestTransport {
    fn get(&self, request: Request) -> impl Future<Output = Result<Response, Error>> + Send;
}

/// Public browse, search and detail metadata. Async operations use a Tokio runtime.
/// Instances hold no subscriber session or mutable publication state.
pub struct Catalog<T = HttpTransport> {
    transport: T,
}

impl Catalog<HttpTransport> {
    pub fn new() -> Result<Self, Error> {
        Ok(Self::with_transport(HttpTransport::new()?))
    }
}

impl<T: RequestTransport> Catalog<T> {
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    /// Anonymous editorial Home/New discovery, in supplied block and card order.
    pub async fn discovery(&self, route: DiscoveryRoute) -> Result<DiscoveryPage, Error> {
        let url = url::Url::parse(match route {
            DiscoveryRoute::Home => "https://www.criterionchannel.com/",
            DiscoveryRoute::New => "https://www.criterionchannel.com/new",
        })
        .map_err(|_| Error::Unavailable)?;
        let response = self.transport.get(Request { url }).await?;
        discovery::parse(response)
    }

    async fn request<D: serde::de::DeserializeOwned>(&self, url: url::Url) -> Result<D, Error> {
        let response = self.transport.get(Request { url }).await?;
        if !(200..300).contains(&response.status) {
            return Err(Error::HttpStatus(response.status));
        }
        if response.body.len() > MAX_RESPONSE_BYTES {
            return Err(Error::ResponseTooLarge);
        }
        if response.content_type.len() > 1024
            || !response
                .content_type
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        {
            return Err(Error::InvalidResponse);
        }
        check_json_bounds(&response.body)?;
        serde_json::from_slice(&response.body).map_err(|_| Error::InvalidResponse)
    }

    /// Search a trimmed, nonempty query of at most 256 UTF-8 bytes.
    pub async fn search(&self, query: &str) -> Result<SearchResults, Error> {
        let query = query.trim();
        if query.is_empty() || query.len() > 256 || query.chars().any(char::is_control) {
            return Err(Error::InvalidRequest);
        }
        let mut url = url::Url::parse("https://www.criterionchannel.com/api/search")
            .map_err(|_| Error::Unavailable)?;
        url.query_pairs_mut().append_pair("q", query);
        let search: WireSearch = self.request(url).await?;
        if search.playlist.len() > 100
            || search.type_counts.len() > 4
            || search.type_counts.values().any(|count| *count > 1_000_000)
        {
            return Err(Error::InvalidResponse);
        }
        Ok(SearchResults {
            items: search
                .playlist
                .into_iter()
                .map(WireMedia::into_summary)
                .collect::<Result<Vec<_>, Error>>()?,
            type_counts: search
                .type_counts
                .into_iter()
                .map(|(kind, count)| {
                    Ok(KindCount {
                        kind: MediaKind::parse(&kind)?,
                        count,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
        })
    }

    /// Read the provider's current filter labels/values and typed sort choices.
    pub async fn options(&self) -> Result<BrowseOptions, Error> {
        let url = url::Url::parse("https://www.criterionchannel.com/api/all-films/filters")
            .map_err(|_| Error::Unavailable)?;
        let options: WireOptions = self.request(url).await?;
        if options.filter_groups.len() > 4
            || options.sort_options.len() > 5
            || options
                .filter_groups
                .iter()
                .map(|group| group.options.len())
                .sum::<usize>()
                > 4096
        {
            return Err(Error::InvalidResponse);
        }
        for group in &options.filter_groups {
            check_text(&group.label, 512, false)?;
            if group.options.len() > 2048 {
                return Err(Error::InvalidResponse);
            }
            for option in &group.options {
                check_text(&option.label, 512, false)?;
            }
        }
        for option in &options.sort_options {
            check_text(&option.label, 512, false)?;
        }
        Ok(BrowseOptions {
            filter_groups: options
                .filter_groups
                .into_iter()
                .map(|group| {
                    Ok(FilterOptions {
                        group: FilterGroup::parse(&group.value)?,
                        label: group.label,
                        options: group
                            .options
                            .into_iter()
                            .map(|option| {
                                Ok(FilterOption {
                                    label: option.label,
                                    value: FilterValue::new(&option.value)
                                        .map_err(|_| Error::InvalidResponse)?,
                                })
                            })
                            .collect::<Result<Vec<_>, Error>>()?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
            sort_options: options
                .sort_options
                .into_iter()
                .map(|option| {
                    Ok(SortOption {
                        label: option.label,
                        sort: Sort::parse(&option.value)?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
        })
    }

    /// Read one matching media record and its display playlists.
    pub async fn detail(&self, id: &MediaId) -> Result<MediaDetail, Error> {
        let url = url::Url::parse(&format!(
            "https://www.criterionchannel.com/api/media/{}",
            id.as_str()
        ))
        .map_err(|_| Error::Unavailable)?;
        let media: WireMedia = self.request(url).await?;
        media.into_detail(id)
    }

    /// Browse at most 100 items with an opaque cursor and up to 64 selected filters.
    pub async fn browse(&self, request: &BrowseRequest) -> Result<CatalogPage, Error> {
        if !(1..=100).contains(&request.page_limit) || request.filters.len() > 64 {
            return Err(Error::InvalidRequest);
        }
        let mut url = url::Url::parse("https://www.criterionchannel.com/api/all-films/results")
            .map_err(|_| Error::Unavailable)?;
        url.query_pairs_mut()
            .append_pair("page_limit", &request.page_limit.to_string())
            .append_pair("sort", request.sort.as_str())
            .append_pair(
                "sortDir",
                match request.direction {
                    SortDirection::Ascending => "asc",
                    SortDirection::Descending => "desc",
                },
            );
        for group in [
            FilterGroup::Genres,
            FilterGroup::Decades,
            FilterGroup::Countries,
            FilterGroup::Directors,
        ] {
            let values: Vec<_> = request
                .filters
                .iter()
                .filter(|filter| filter.group == group)
                .map(|filter| filter.value.as_str())
                .collect();
            if !values.is_empty() {
                url.query_pairs_mut()
                    .append_pair(group.as_str(), &values.join(","));
            }
        }
        if let Some(cursor) = &request.cursor {
            url.query_pairs_mut()
                .append_pair("pagination_key", cursor.value());
        }
        let page: WirePage = self.request(url).await?;
        if page.items.len() > 100
            || page.items.len() > usize::from(request.page_limit)
            || page.total > 1_000_000
            || (page.total as usize) < page.items.len()
        {
            return Err(Error::InvalidResponse);
        }
        Ok(CatalogPage {
            total: page.total,
            next_cursor: page
                .paging
                .next_pagination_key
                .filter(|cursor| !cursor.is_empty())
                .map(|cursor| PageCursor::new(&cursor).map_err(|_| Error::InvalidResponse))
                .transpose()?,
            items: page
                .items
                .into_iter()
                .map(WireMedia::into_summary)
                .collect::<Result<Vec<_>, Error>>()?,
        })
    }
}
