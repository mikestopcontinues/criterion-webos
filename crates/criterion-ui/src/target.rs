use criterion_provider::{ContentTarget, MediaId};
/// Activation identity admitted by the provider/runtime, independent of artwork.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    Media(MediaId),
    Content(ContentTarget),
}
impl Target {
    pub fn media_id(&self) -> Option<&MediaId> {
        match self {
            Self::Media(id) | Self::Content(ContentTarget::Media { id, .. }) => Some(id),
            _ => None,
        }
    }
    pub(crate) fn page(&self) -> crate::Page {
        use crate::Page;
        match self {
            Self::Media(_) | Self::Content(ContentTarget::Media { .. }) => Page::Detail,
            Self::Content(ContentTarget::Home) => Page::Home,
            Self::Content(ContentTarget::New) => Page::New,
            Self::Content(ContentTarget::AllFilms) => Page::AllFilms,
            Self::Content(ContentTarget::MyList) => Page::MyList,
            Self::Content(ContentTarget::Subscribe) => Page::Login,
            Self::Content(ContentTarget::Discover(_)) => Page::Discovery,
        }
    }
}
impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Media(_) => "Media(<admitted>)",
            Self::Content(_) => "Content(<admitted>)",
        })
    }
}
