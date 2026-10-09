//! One measured rail descriptor for navigation, painting and pointer ownership.
use crate::{LoginView, Page, RailItem, icons::Icon};

pub(crate) struct Entry {
    pub(crate) item: RailItem,
    pub(crate) page: Page,
    pub(crate) icon: Icon,
    label: &'static str,
    y: f32,
    subscriber_y: Option<f32>,
}
const ENTRIES: [Entry; 6] = [
    Entry {
        item: RailItem::Search,
        page: Page::Search,
        icon: Icon::Search,
        label: "SEARCH",
        y: 208.0,
        subscriber_y: None,
    },
    Entry {
        item: RailItem::Home,
        page: Page::Home,
        icon: Icon::Home,
        label: "HOME",
        y: 356.0,
        subscriber_y: None,
    },
    Entry {
        item: RailItem::New,
        page: Page::New,
        icon: Icon::Sparkle,
        label: "NEW",
        y: 430.0,
        subscriber_y: None,
    },
    Entry {
        item: RailItem::MyList,
        page: Page::MyList,
        icon: Icon::MyList,
        label: "MY LIST",
        y: 504.0,
        subscriber_y: None,
    },
    Entry {
        item: RailItem::AllFilms,
        page: Page::AllFilms,
        icon: Icon::FilmReel,
        label: "ALL FILMS",
        y: 504.0,
        subscriber_y: Some(578.0),
    },
    Entry {
        item: RailItem::Login,
        page: Page::Login,
        icon: Icon::Account,
        label: "LOG IN",
        y: 649.0,
        subscriber_y: Some(726.0),
    },
];
pub(crate) fn entries(login: LoginView<'_>) -> impl Iterator<Item = &'static Entry> + Clone {
    let signed_in = matches!(login, LoginView::SignedIn);
    ENTRIES
        .iter()
        .filter(move |entry| entry.item != RailItem::MyList || signed_in)
}
impl Entry {
    pub(crate) fn y(&self, login: LoginView<'_>) -> f32 {
        if matches!(login, LoginView::SignedIn) {
            self.subscriber_y.unwrap_or(self.y)
        } else {
            self.y
        }
    }
    pub(crate) fn label(&self, login: LoginView<'_>) -> &'static str {
        if self.item == RailItem::Login
            && matches!(login, LoginView::SignedIn | LoginView::SigningOut)
        {
            "ACCOUNT"
        } else {
            self.label
        }
    }
}
