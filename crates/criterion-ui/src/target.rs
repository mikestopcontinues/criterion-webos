use criterion_provider::{ContentTarget, MediaId};
/// Activation identity admitted by the provider/runtime, independent of artwork.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    Media(MediaId),
    /// Native account origin; the original row ID stays independent of its action.
    Native(MediaId),
    Content(ContentTarget),
}
impl Target {
    pub fn media_id(&self) -> Option<&MediaId> {
        match self {
            Self::Media(id) | Self::Native(id) | Self::Content(ContentTarget::Media { id, .. }) => {
                Some(id)
            }
            _ => None,
        }
    }
    pub(crate) fn page(&self) -> crate::Page {
        use crate::Page;
        match self {
            Self::Media(_) | Self::Native(_) | Self::Content(ContentTarget::Media { .. }) => {
                Page::Detail
            }
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
            Self::Native(_) => "Native(<admitted>)",
            Self::Content(_) => "Content(<admitted>)",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_public_targets_keep_the_same_id_with_distinct_activation_origin() {
        let id = MediaId::new("Film0001").unwrap();
        let native = Target::Native(id.clone());
        let public = Target::Media(id.clone());
        assert_ne!(native, public);
        assert_eq!(native.media_id(), Some(&id));
        assert_eq!(public.media_id(), Some(&id));
        assert_eq!(native.page(), crate::Page::Detail);
        assert_eq!(public.page(), crate::Page::Detail);
        assert_eq!(format!("{native:?}"), "Native(<admitted>)");
        assert_eq!(format!("{public:?}"), "Media(<admitted>)");
    }
}
