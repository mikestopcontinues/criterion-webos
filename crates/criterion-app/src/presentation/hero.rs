// SPDX-License-Identifier: GPL-3.0-or-later
//! Immutable supplied slots and one local selected ordinal; no timers or provider work.
use super::{ImageSource, OwnedCard, Presentation, desktop_image, vec_bytes};
use criterion_provider::DiscoverySlide;
use criterion_ui::{Hero, HeroAction, HeroCarousel, HeroCursor, HeroDirection, LoadState, Target};

pub(super) struct OwnedSlideshow {
    block: u32,
    slots: Vec<Slot>,
    selected: usize,
}
struct Slot {
    id: u32,
    hero: Option<OwnedHero>,
    caption: String,
}
pub(super) struct OwnedHero {
    card: OwnedCard,
    description: String,
    action: String,
    background: String,
    logo: Option<String>,
}
impl OwnedHero {
    pub(super) fn view(&self) -> Hero<'_> {
        Hero {
            card: self.card.view(),
            description: &self.description,
            action: &self.action,
            action_kind: HeroAction::Open,
            background_key: Some(&self.background),
            title_logo_key: self.logo.as_deref(),
        }
    }
    fn references(&self, key: &str) -> bool {
        self.background == key || self.logo.as_deref() == Some(key)
    }
    fn heap_bytes(&self) -> usize {
        self.card.heap_bytes()
            + self.description.capacity()
            + self.action.capacity()
            + self.background.capacity()
            + self.logo.as_ref().map_or(0, String::capacity)
    }
}
impl OwnedSlideshow {
    pub(super) fn heap_bytes(&self) -> usize {
        vec_bytes(&self.slots)
            + self
                .slots
                .iter()
                .map(|slot| {
                    slot.caption.capacity() + slot.hero.as_ref().map_or(0, OwnedHero::heap_bytes)
                })
                .sum::<usize>()
    }
    pub(super) fn references(&self, key: &str) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.hero.as_ref().is_some_and(|hero| hero.references(key)))
    }
}
impl Presentation {
    pub(super) fn admit_slideshow(&mut self, block: u32, slides: Vec<DiscoverySlide>) {
        let total = slides.len();
        let mut slots = Vec::with_capacity(total);
        for (index, slide) in slides.into_iter().enumerate() {
            slots.push(Slot {
                id: slide.id,
                hero: self.admit_hero(slide),
                caption: format!("Slide {} of {total}", index + 1),
            });
        }
        self.slideshow = Some(OwnedSlideshow {
            block,
            slots,
            selected: 0,
        });
    }
    fn admit_hero(&mut self, slide: DiscoverySlide) -> Option<OwnedHero> {
        if slide.opens_new_window {
            self.gaps.new_window_targets += usize::from(slide.target.is_some());
            self.gaps.unavailable_heroes += 1;
            return None;
        }
        let (Some(target), Some(action), Some(background)) = (
            slide.target,
            slide.cta,
            desktop_image(&slide.artwork).cloned(),
        ) else {
            self.gaps.unavailable_heroes += 1;
            return None;
        };
        Some(OwnedHero {
            card: OwnedCard {
                target: Target::Content(target),
                kind: None,
                title: slide.title.unwrap_or_default(),
                year: String::new(),
                duration_label: None,
                artwork: None,
                saved_fraction: None,
                native_activation: None,
            },
            description: slide.title_prefix.unwrap_or_default(),
            action,
            background: self.bind_image(ImageSource::Editorial(background)),
            logo: slide
                .artwork
                .logo
                .map(|image| self.bind_image(ImageSource::Editorial(image))),
        })
    }
    pub(super) fn current_hero(&self) -> Option<&OwnedHero> {
        let value = self.slideshow.as_ref()?;
        value.slots.get(value.selected)?.hero.as_ref()
    }
    pub(super) fn hero_carousel(&self) -> Option<HeroCarousel<'_>> {
        let value = self.slideshow.as_ref()?;
        let slot = value.slots.get(value.selected)?;
        Some(HeroCarousel {
            block: value.block,
            index: value.selected,
            slide: slot.id,
            total: value.slots.len(),
            caption: &slot.caption,
            visit: None,
        })
    }
    pub(super) fn refresh_discovery_status(&mut self) {
        self.status = if self.total > 0
            || self.rails.iter().any(|rail| rail.action.is_some())
            || self
                .slideshow
                .as_ref()
                .is_some_and(|value| !value.slots.is_empty())
        {
            LoadState::Ready
        } else {
            LoadState::Empty
        };
    }
    pub(crate) fn hero_cursor(&self, visit: u64) -> Option<HeroCursor> {
        if self.status != LoadState::Ready {
            return None;
        }
        let mut value = self.hero_carousel()?;
        value.visit = Some(visit);
        value.cursor()
    }
    pub(crate) fn hero_target(&self) -> Option<&Target> {
        Some(&self.current_hero()?.card.target)
    }
    pub(crate) fn move_hero(&mut self, direction: HeroDirection) {
        let Some(value) = &mut self.slideshow else {
            return;
        };
        let total = value.slots.len();
        if total < 2 {
            return;
        }
        value.selected = match direction {
            HeroDirection::Previous => (value.selected + total - 1) % total,
            HeroDirection::Next => (value.selected + 1) % total,
        };
    }
    /// Source retention includes every slot; demand includes only the active slot.
    pub(crate) fn retain_current_artwork(
        &self,
        visible: &mut Vec<String>,
        cards: &[criterion_ui::CardLayout],
    ) {
        visible.retain(|key| {
            !self
                .slideshow
                .as_ref()
                .is_some_and(|value| value.references(key))
                || self.current_hero().is_some_and(|hero| hero.references(key))
                || cards.iter().any(|layout| {
                    let criterion_ui::Focus::Card { row, column } = layout.focus else {
                        return false;
                    };
                    self.rails
                        .get(row)
                        .and_then(|rail| rail.cards.get(column))
                        .is_some_and(|card| {
                            card.target == layout.key && card.artwork.as_deref() == Some(key)
                        })
                })
        });
    }
}
