// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual SDL/GLES Feature journey; HTTP, issuer and decoded images are synthetic.
//! The reused factory enters Detail through Application input; SDL starts there.
use super::super::native_featured_tests::{assert_feature_projection, featured_fixture};
use super::*;
use crate::presentation::ImageSource;

const READS: [Kind; 7] = [
    Kind::ContinueWatching,
    Kind::WatchList,
    Kind::NativeDetail("Listed01"),
    Kind::MyListIds,
    Kind::NativeDetail("Related1"),
    Kind::MyListIds,
    Kind::MyListIds,
];

#[derive(Clone, Copy)]
pub(super) enum Capture {
    Initial,
    WarmBack,
    SignedOutBack,
}
pub(super) const CAPTURES: [Capture; 3] =
    [Capture::Initial, Capture::WarmBack, Capture::SignedOutBack];

fn read_prefix(calls: &[Kind]) -> bool {
    calls.len() <= READS.len() && calls == &READS[..calls.len()]
}
pub(super) fn reads_bounded(fixture: &Fixture) -> bool {
    read_prefix(&fixture.script.calls.lock().unwrap())
}

// Binding and expected identities are independent fixture literals. Same-ID
// Feature Film and ordinary Episode intentionally share their image source.
fn artwork_key(fixture: &Fixture) -> Option<String> {
    let key = fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let detail = view.detail.as_ref()?;
            let card = detail.featured.as_ref()?.cards.first()?;
            let ordinary = view.rails.first()?.cards.first()?;
            (matches!(card.key, Target::Native(id) if id.as_str() == "Related1")
                && card.artwork_key == ordinary.artwork_key)
                .then(|| card.artwork_key.map(str::to_owned))
                .flatten()
        })?;
    fixture
        .app
        .controller
        .view
        .artwork_bindings()
        .iter()
        .any(|binding| {
            binding.key == key
                && matches!(&binding.source, ImageSource::Media {
            id, label: criterion_provider::ImageLabel::Landscape,
            role: criterion_artwork::ImageRole::Card,
        } if id.as_str() == "Related1")
        })
        .then_some(key)
}

pub(super) fn fixture() -> Fixture {
    let mut fixture = featured_fixture(true);
    assert_feature_projection(&fixture, Some(0.2));
    // The CPU factory must preserve the initial atlas upload for the first
    // actual GLES paint. Never take/clear its output as a setup convenience.
    assert!(
        fixture
            .app
            .output
            .as_ref()
            .unwrap()
            .textures_delta
            .set
            .iter()
            .any(|(id, _)| *id == egui::TextureId::Managed(0)),
        "actual factory must retain initial managed font texture work"
    );
    fixture.script.steps.lock().unwrap().extend([
        detail("Related1", None),
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
        Step {
            kind: Kind::MyListIds,
            gate: None,
        },
    ]);
    admit_current_artwork(&mut fixture).expect("current exact synthetic artwork admission");
    fixture
}

// Offline Failed attempts intentionally discard their cache entries. Reset that
// existing owner and reinject the decoded fixture pattern at EACH capture stage;
// this admits the renderer seam, not loader success or warm image-cache retention.
fn admit_current_artwork(fixture: &mut Fixture) -> Result<(), &'static str> {
    let key = artwork_key(fixture).ok_or("current exact Feature artwork binding")?;
    fixture.app.artwork.clear();
    // This is synthetic decoded artwork at the established image seam, not
    // a successful Artwork loader/provider admission.
    fixture
        .app
        .ui
        .admit_image(
            &key,
            egui::ColorImage::new(
                [2, 2],
                vec![
                    egui::Color32::from_rgb(30, 50, 200),
                    egui::Color32::from_rgb(210, 30, 50),
                    egui::Color32::from_rgb(30, 50, 200),
                    egui::Color32::from_rgb(210, 30, 50),
                ],
            ),
        )
        .map_err(|_| "synthetic decoded image admission")?;
    Ok(())
}

impl Capture {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Initial => "synthetic-featured-initial.png",
            Self::WarmBack => "synthetic-featured-warm-back.png",
            Self::SignedOutBack => "synthetic-featured-signed-out-back.png",
        }
    }
    fn fraction(self) -> Option<f32> {
        if matches!(self, Self::Initial) {
            Some(0.2)
        } else {
            None
        }
    }
    pub(super) fn admit(
        self,
        fixture: &Fixture,
        output: &egui::FullOutput,
    ) -> Result<(), &'static str> {
        let signed_out = matches!(self, Self::SignedOutBack);
        require(
            fixture.app.ui.page() == Page::Detail
                && fixture.app.ui.focus() == Focus::FeaturedCard(0)
                && fixture.app.ui.scroll_y() == 632.0
                && fixture.native_ready("Listed01")
                && fixture.app.authentication.signed_in() != signed_out
                && fixture.app.membership_view()
                    == if signed_out {
                        criterion_ui::ListMembership::SignedOut
                    } else {
                        criterion_ui::ListMembership::Known { present: true }
                    },
            "exact current Feature root/focus/visit mismatch",
        )?;
        require(
            fixture
                .app
                .controller
                .view
                .with_view(fixture.app.authentication.view(), |view| {
                    let Some(detail) = &view.detail else {
                        return false;
                    };
                    let Some(featured) = &detail.featured else {
                        return false;
                    };
                    let Some(card) = featured.cards.first() else {
                        return false;
                    };
                    let Some(ordinary) = view.rails.first().and_then(|rail| rail.cards.first())
                    else {
                        return false;
                    };
                    detail.kind == DetailKind::Collection
                        && detail.card.title == "Synthetic Featured Collection"
                        && featured.title == Some("Synthetic supplied Feature heading")
                        && featured.cards.len() == 1
                        && card.title == "Synthetic Feature Film"
                        && card.action == criterion_ui::CardAction::Open
                        && card.duration_label == Some("1 min")
                        && card.saved_fraction == self.fraction()
                        && ordinary.title == "Synthetic ordinary Episode"
                        && ordinary.action == criterion_ui::CardAction::Play
                        && ordinary.duration_label == Some("1 min")
                        && ordinary.saved_fraction == self.fraction()
                        && matches!(ordinary.key, Target::Native(id) if id.as_str() == "Related1")
                        && detail.selected_playlist == Some(0)
                }),
            "independent Feature/ordinary identity, runtime or progress mismatch",
        )?;
        let key = artwork_key(fixture).ok_or("current Feature artwork ownership absent")?;
        require(
            fixture.app.ui.image_bytes(&key) == Some(16),
            "current synthetic decoded texture absent",
        )?;
        for (text, area) in [
            (
                "Synthetic supplied Feature heading",
                egui::Rect::from_min_max(egui::pos2(150.0, 334.0), egui::pos2(1770.0, 380.0)),
            ),
            (
                "Synthetic Feature Film",
                egui::Rect::from_min_max(egui::pos2(150.0, 636.0), egui::pos2(496.0, 680.0)),
            ),
            (
                "1 min",
                egui::Rect::from_min_max(egui::pos2(430.0, 674.0), egui::pos2(528.0, 710.0)),
            ),
        ] {
            require(
                text_inside(output, text, area),
                "complete visible Feature caption missing or elided",
            )?;
        }
        let image = egui::Rect::from_min_size(egui::pos2(150.0, 415.0), egui::vec2(378.0, 213.0));
        let focus = image.expand(8.0);
        require(
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default()
                && mesh.calc_bounds() == image && shape.clip_rect.contains_rect(image))
            }),
            "current Feature texture missing from actual image geometry",
        )?;
        require(
            output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
            egui::Shape::Rect(rect) if rect.rect == focus && rect.stroke.width == 8.0
                && rect.stroke.color == egui::Color32::from_rgb(181, 138, 22)
                && shape.clip_rect.contains_rect(focus))
            }),
            "complete canonical eight-pixel Feature focus missing",
        )?;
        let progress = egui::Rect::from_min_size(egui::pos2(150.0, 625.0), egui::vec2(378.0, 3.0));
        if self.fraction().is_some() {
            require(
                output.shapes.iter().any(|shape| {
                    matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect.min == progress.min
                    && (rect.rect.width() - 75.6).abs() < 0.001 && rect.rect.height() == 3.0
                    && rect.fill == egui::Color32::from_rgb(239, 239, 239))
                }),
                "literal twenty-percent Feature fill geometry missing",
            )?;
        } else {
            require(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.rect.height() == 3.0 && progress.intersects(rect.rect))),
                "departed private Feature progress retained in current shapes")?;
        }
        if signed_out {
            require(
                fixture.app.positions.is_none(),
                "logout retained private position snapshot",
            )?;
        }
        Ok(())
    }
    pub(super) fn pixels(self, pixels: &[u8]) -> bool {
        let blue = pixel(pixels, 180, 480);
        let red = pixel(pixels, 498, 480);
        if !(blue[2] > blue[0].saturating_add(80) && red[0] > red[2].saturating_add(80)) {
            return false;
        }
        let fill = pixel(pixels, 162, 626);
        let track = pixel(pixels, 260, 626);
        if self.fraction().is_some() {
            fill.iter().all(|value| *value > 220) && (90..=190).contains(&track[1])
        } else {
            fill[0] < 80 && fill[1] < 100 && fill[2] > 150
        }
    }
}

fn child_ready(fixture: &Fixture) -> bool {
    fixture.native_ready("Related1")
        && fixture.app.membership_view() == criterion_ui::ListMembership::Known { present: false }
        && fixture
            .app
            .controller
            .view
            .with_view(fixture.app.authentication.view(), |view| {
                view.detail.as_ref().is_some_and(|detail| {
                    detail.kind == DetailKind::Film
                        && detail
                            .primary_playback_target
                            .is_some_and(|id| id.as_str() == "Related1")
                })
            })
}
fn root_ready(fixture: &Fixture) -> bool {
    fixture.native_ready("Listed01")
        && fixture.app.membership_view() == criterion_ui::ListMembership::Known { present: true }
}
fn exact_final_census(fixture: &Fixture) -> bool {
    *fixture.script.calls.lock().unwrap() == READS
        && fixture.script.bootstrap.load(Ordering::SeqCst) == 1
        && fixture.script.maximum.load(Ordering::SeqCst) == 1
        && fixture.script.active.load(Ordering::SeqCst) == 0
        && fixture.script.steps.lock().unwrap().is_empty()
        && fixture.script.violation.lock().unwrap().is_none()
        && *fixture.public_requests.lock().unwrap() == ["/"]
        && fixture.issuer.tokens.load(Ordering::SeqCst) == 1
        && fixture.issuer.revokes.load(Ordering::SeqCst) == 1
}

pub(super) fn journey(rendered: &mut Rendered<'_>) -> Result<(), &'static str> {
    rendered.wait(root_ready)?;
    rendered.key(DOWN)?;
    rendered.key(DOWN)?;
    assert_feature_projection(rendered.fixture, Some(0.2));
    admit_current_artwork(rendered.fixture)?;
    rendered.capture(super::Capture::Featured(Capture::Initial))?;
    // Feature Film Open must select this exact address, despite the ordinary
    // Episode with identical ID. Do not activate the unavailable Episode player.
    *rendered.fixture.script.detail_body.lock().unwrap() = None;
    rendered.key(SELECT)?;
    rendered.wait(child_ready)?;
    require(
        rendered.fixture.script.calls.lock().unwrap().len() == 6,
        "Feature Film must issue one exact child Detail and membership read",
    )?;
    rendered.key(BACK)?;
    rendered
        .wait(|fixture| root_ready(fixture) && fixture.script.calls.lock().unwrap().len() == 7)?;
    assert_feature_projection(rendered.fixture, None);
    admit_current_artwork(rendered.fixture)?;
    rendered.capture(super::Capture::Featured(Capture::WarmBack))?;
    rendered.account()?;
    rendered.key(SELECT)?;
    rendered.wait(|fixture| {
        matches!(fixture.app.authentication.view(), LoginView::SignedOut)
            && fixture.issuer.revokes.load(Ordering::SeqCst) == 1
    })?;
    rendered.key(BACK)?;
    assert_feature_projection(rendered.fixture, None);
    admit_current_artwork(rendered.fixture)?;
    rendered.capture(super::Capture::Featured(Capture::SignedOutBack))?;
    require(
        exact_final_census(rendered.fixture),
        "exact seven-read Feature census or logout disposal boundary mismatch",
    )
}

#[test]
fn featured_capture_shapes_preserve_font_upload_and_match_actual_application_retirement() {
    let mut fixture = fixture();
    fixture.key(DOWN.0, DOWN.1);
    fixture.key(DOWN.0, DOWN.1);
    let fresh = |fixture: &mut Fixture| {
        admit_current_artwork(fixture).unwrap();
        // This separate CPU witness owns no GLES renderer. Retire its texture
        // work only after verifying the factory preserves the first atlas.
        if let Some(mut output) = fixture.app.take_output() {
            output.textures_delta.clear();
        }
        fixture.pump();
        let mut output = fixture.app.take_output().unwrap();
        output.textures_delta.clear();
        output
    };
    let output = fresh(&mut fixture);
    Capture::Initial.admit(&fixture, &output).unwrap();
    *fixture.script.detail_body.lock().unwrap() = None;
    fixture.key(SELECT.0, SELECT.1);
    fixture.wait(child_ready);
    fixture.key(BACK.0, BACK.1);
    fixture.wait(|fixture| root_ready(fixture) && fixture.script.calls.lock().unwrap().len() == 7);
    let output = fresh(&mut fixture);
    Capture::WarmBack.admit(&fixture, &output).unwrap();
    // CPU command retirement is distinct from the opt-in actual SDL Account route.
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| {
        matches!(fixture.app.authentication.view(), LoginView::SignedOut)
            && fixture.issuer.revokes.load(Ordering::SeqCst) == 1
    });
    let output = fresh(&mut fixture);
    Capture::SignedOutBack.admit(&fixture, &output).unwrap();
    assert!(exact_final_census(&fixture));
}

#[test]
fn featured_pixel_admission_refuses_placeholder_and_retained_private_progress() {
    let mut pixels = vec![39; 1920 * 1080 * 4];
    assert!(!Capture::Initial.pixels(&pixels));
    assert!(!Capture::WarmBack.pixels(&pixels));
    for (x, y, rgb) in [
        (180, 480, [30, 50, 200]),
        (498, 480, [210, 30, 50]),
        (162, 626, [239, 239, 239]),
        (260, 626, [160, 140, 180]),
    ] {
        let at = ((1079 - y) * 1920 + x) * 4;
        pixels[at..at + 3].copy_from_slice(&rgb);
    }
    assert!(Capture::Initial.pixels(&pixels));
    assert!(!Capture::WarmBack.pixels(&pixels));
    let at = ((1079 - 626) * 1920 + 162) * 4;
    pixels[at..at + 3].copy_from_slice(&[30, 50, 200]);
    assert!(!Capture::Initial.pixels(&pixels));
    assert!(Capture::WarmBack.pixels(&pixels));
    assert!(Capture::SignedOutBack.pixels(&pixels));
}

#[test]
fn featured_read_census_refuses_second_detail_or_wrong_order() {
    for length in 0..=READS.len() {
        assert!(read_prefix(&READS[..length]));
    }
    let mut extra = READS.to_vec();
    extra.push(Kind::NativeDetail("Related1"));
    assert!(!read_prefix(&extra));
    let mut wrong = READS;
    wrong.swap(4, 5);
    assert!(!read_prefix(&wrong));
}

#[test]
#[ignore = "synthetic offline Feature/CW/Session/decoded artwork; Root serialized actual SDL/GLES executor"]
fn native_synthetic_featured_film_back_and_logout_end_to_end() {
    run(Journey::Featured);
}
