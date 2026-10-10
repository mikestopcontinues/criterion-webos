// SPDX-License-Identifier: GPL-3.0-or-later
//! Actual SDL/GLES Sort journey; HTTP, issuer and decoded artwork are synthetic.
//! The existing factory enters Detail through Application events; SDL starts there.
use super::super::native_sort_tests::sort_fixture_with_positions;
use super::*;
use crate::presentation::ImageSource;
use criterion_ui::{
    DetailSortDirection as Direction, DetailSortField as Field, DetailSortSelection,
};

const READS: [Kind; 7] = [
    Kind::ContinueWatching,
    Kind::WatchList,
    Kind::NativeDetail("Listed01"),
    Kind::MyListIds,
    Kind::NativeDetail("Related1"),
    Kind::MyListIds,
    Kind::MyListIds,
];
// The exact keys are shared by the SDL journey and the separate CPU witness,
// without replacing either real Application input path with a model.
const DRAFT_TITLE: [(u32, i32); 7] = [DOWN, DOWN, DOWN, SELECT, DOWN, SELECT, RIGHT];
const DRAFT_DESCENDING: [(u32, i32); 3] = [UP, SELECT, SELECT];
const APPLY_DEFAULT: [(u32, i32); 5] = [UP, SELECT, RIGHT, SELECT, DOWN];
const REAPPLY_TITLE: [(u32, i32); 8] = [UP, SELECT, DOWN, SELECT, RIGHT, SELECT, DOWN, RIGHT];

#[derive(Clone, Copy)]
pub(super) enum Capture {
    PendingTitle,
    TitleAscending,
    DefaultRestored,
    WarmBack,
    SignedOutBack,
}
pub(super) const CAPTURES: [Capture; 5] = [
    Capture::PendingTitle,
    Capture::TitleAscending,
    Capture::DefaultRestored,
    Capture::WarmBack,
    Capture::SignedOutBack,
];

fn selection(field: Field, direction: Direction) -> DetailSortSelection {
    DetailSortSelection { field, direction }
}
fn read_prefix(calls: &[Kind]) -> bool {
    calls.len() <= READS.len() && calls == &READS[..calls.len()]
}
pub(super) fn reads_bounded(fixture: &Fixture) -> bool {
    read_prefix(&fixture.script.calls.lock().unwrap())
}
fn local_reads(fixture: &Fixture) -> bool {
    *fixture.script.calls.lock().unwrap() == READS[..4]
        && fixture.script.active.load(Ordering::SeqCst) == 0
        && fixture.script.violation.lock().unwrap().is_none()
}
fn sort_state(
    fixture: &Fixture,
    visible: bool,
    pending: DetailSortSelection,
    applied: DetailSortSelection,
) -> bool {
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            view.detail
                .as_ref()
                .and_then(|detail| detail.sort)
                .is_some_and(|sort| {
                    sort.visible == visible && sort.pending == pending && sort.applied == applied
                })
        })
}
fn projection(fixture: &Fixture, ascending: bool, fraction: Option<f32>) -> bool {
    fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            let Some(detail) = &view.detail else {
                return false;
            };
            let Some(feature) = &detail.featured else {
                return false;
            };
            let [first, tail] = view.rails else {
                return false;
            };
            let expected = if ascending {
                [
                    ("Related2", "Alpha", "2 min", fraction),
                    ("Related1", "beta", "1 min", None),
                ]
            } else {
                [
                    ("Related1", "beta", "1 min", None),
                    ("Related2", "Alpha", "2 min", fraction),
                ]
            };
            view.status == LoadState::Ready
                && detail.kind == DetailKind::Collection
                && matches!(detail.card.key, Target::Native(id) if id.as_str() == "Listed01")
                && detail.card.title == "Synthetic sortable collection"
                && detail.primary_playback_target.is_none()
                && detail.selected_playlist == Some(0)
                && feature.title == Some("Supplied Feature")
                && feature.cards.len() == 1
                && feature.cards[0].title == "Feature unchanged"
                && matches!(feature.cards[0].key, Target::Native(id) if id.as_str() == "Feature1")
                && first.title == "First supplied tab"
                && first.cards.len() == 2
                && first
                    .cards
                    .iter()
                    .zip(expected)
                    .all(|(card, (id, title, runtime, saved))| {
                        matches!(card.key, Target::Native(key) if key.as_str() == id)
                            && card.title == title
                            && card.duration_label == Some(runtime)
                            && card.saved_fraction == saved
                            && card.action == criterion_ui::CardAction::Open
                    })
                && tail.title == "Tail unchanged"
                // The actual ViewData publishes cards only for the active tab.
                && tail.cards.is_empty()
                && (fraction.is_some()
                    || view
                        .rails
                        .iter()
                        .flat_map(|rail| rail.cards.iter())
                        .all(|card| card.saved_fraction.is_none()))
        })
}
fn root_ready(fixture: &Fixture) -> bool {
    fixture.native_ready("Listed01")
        && fixture.app.membership_view() == criterion_ui::ListMembership::Known { present: true }
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
                        && detail.card.title == "Native Related1"
                        && detail
                            .primary_playback_target
                            .is_some_and(|id| id.as_str() == "Related1")
                })
            })
}

pub(super) fn fixture() -> Fixture {
    let fixture = sort_fixture_with_positions(true);
    assert!(projection(&fixture, false, Some(0.25)));
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
        "actual factory preserves initial font upload for GLES"
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
    fixture
}

fn artwork_key(fixture: &Fixture, expected: &str) -> Option<String> {
    let key = fixture
        .app
        .controller
        .view
        .with_view(fixture.app.authentication.view(), |view| {
            view.rails.first()?.cards.iter().find_map(|card| {
                (matches!(card.key, Target::Native(id) if id.as_str() == expected))
                    .then(|| card.artwork_key.map(str::to_owned))
                    .flatten()
            })
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
        } if id.as_str() == expected)
        })
        .then_some(key)
}
fn admit_current_artwork(fixture: &mut Fixture) -> Result<(), &'static str> {
    let blue = artwork_key(fixture, "Related1").ok_or("current beta artwork binding")?;
    let red = artwork_key(fixture, "Related2").ok_or("current Alpha artwork binding")?;
    require(blue != red, "distinct exact native card artwork keys")?;
    // Each capture resets Failed offline attempts and injects decoded fixture
    // pixels. This is renderer admission, not loader or warm-cache success.
    fixture.app.artwork.clear();
    for (key, color) in [
        (blue, egui::Color32::from_rgb(30, 50, 200)),
        (red, egui::Color32::from_rgb(210, 30, 50)),
    ] {
        fixture
            .app
            .ui
            .admit_image(&key, egui::ColorImage::new([2, 2], vec![color; 4]))
            .map_err(|_| "synthetic decoded card image admission")?;
    }
    Ok(())
}

fn modal_shapes(output: &egui::FullOutput, focus: Focus) -> bool {
    let rect = |x, y, width, height| {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
    };
    let focused = match focus {
        Focus::DetailSortApply => rect(510.0, 750.0, 900.0, 82.0),
        Focus::DetailSortOption(Field::Title) => rect(510.0, 370.0, 900.0, 80.0),
        _ => return false,
    };
    [
        ("Sort by", rect(510.0, 183.0, 750.0, 58.0)),
        ("Default", rect(510.0, 270.0, 900.0, 80.0)),
        ("Title", rect(510.0, 370.0, 900.0, 80.0)),
        ("Release Date", rect(510.0, 470.0, 900.0, 80.0)),
        ("Runtime", rect(510.0, 570.0, 900.0, 80.0)),
        ("APPLY", rect(510.0, 750.0, 900.0, 82.0)),
    ].into_iter().all(|(text, area)| text_inside(output, text, area))
        && title_direction_shapes(output, Direction::Ascending)
        && output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(r)
            if r.rect == rect(450.0, 130.0, 1050.0, 760.0)
                && r.fill == egui::Color32::from_rgb(28,28,28) && shape.clip_rect.contains_rect(r.rect)))
        && output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(r)
            if r.rect == rect(0.0, 0.0, 1920.0, 1080.0)
                && r.fill == egui::Color32::from_black_alpha(220)))
        && output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(r)
                if r.rect == focused
                && r.fill == egui::Color32::from_rgb(181,138,22)))
        && output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Circle(c)
            if c.center == egui::pos2(1382.0,410.0) && c.radius == 6.0
                && c.fill == egui::Color32::from_rgb(181,138,22)))
}

impl Capture {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::PendingTitle => "synthetic-sort-pending-title.png",
            Self::TitleAscending => "synthetic-sort-title-ascending.png",
            Self::DefaultRestored => "synthetic-sort-default-restored.png",
            Self::WarmBack => "synthetic-sort-warm-back.png",
            Self::SignedOutBack => "synthetic-sort-signed-out-back.png",
        }
    }
    fn ascending(self) -> bool {
        matches!(
            self,
            Self::TitleAscending | Self::WarmBack | Self::SignedOutBack
        )
    }
    fn fraction(self) -> Option<f32> {
        (!matches!(self, Self::WarmBack | Self::SignedOutBack)).then_some(0.25)
    }
    pub(super) fn focus(self) -> Focus {
        match self {
            Self::PendingTitle => Focus::DetailSortApply,
            Self::TitleAscending | Self::DefaultRestored => Focus::Card { row: 0, column: 0 },
            Self::WarmBack | Self::SignedOutBack => Focus::Card { row: 0, column: 1 },
        }
    }
    pub(super) fn focus_region(self) -> (Range<usize>, Range<usize>) {
        match self {
            Self::PendingTitle => (510..1410, 750..832),
            Self::TitleAscending | Self::DefaultRestored => (142..536, 427..656),
            Self::WarmBack | Self::SignedOutBack => (556..950, 427..656),
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
                && fixture.native_ready("Listed01")
                && fixture.app.ui.focus() == self.focus()
                && fixture.app.authentication.signed_in() != signed_out
                && fixture.app.membership_view()
                    == if signed_out {
                        criterion_ui::ListMembership::SignedOut
                    } else {
                        criterion_ui::ListMembership::Known { present: true }
                    }
                && projection(fixture, self.ascending(), self.fraction()),
            "exact current Sort root/cards/runtime/fractions/focus/membership",
        )?;
        let pending = matches!(self, Self::PendingTitle);
        require(
            sort_state(
                fixture,
                pending,
                selection(
                    if pending || self.ascending() {
                        Field::Title
                    } else {
                        Field::Default
                    },
                    Direction::Ascending,
                ),
                selection(
                    if self.ascending() {
                        Field::Title
                    } else {
                        Field::Default
                    },
                    Direction::Ascending,
                ),
            ),
            "exact pending/applied local Sort selection",
        )?;
        if pending {
            require(
                local_reads(fixture) && modal_shapes(output, self.focus()),
                "current complete pending Sort modal geometry",
            )?;
            return Ok(());
        }
        require(fixture.app.ui.scroll_y() == 1020.0
            && !output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.job.text == "Sort by")),
            "current ordinary-card scroll or retired modal")?;
        let mut textures = Vec::with_capacity(2);
        for (column, (id, title, runtime)) in if self.ascending() {
            [
                ("Related2", "Alpha", "2 min"),
                ("Related1", "beta", "1 min"),
            ]
        } else {
            [
                ("Related1", "beta", "1 min"),
                ("Related2", "Alpha", "2 min"),
            ]
        }
        .into_iter()
        .enumerate()
        {
            let left = 150.0 + column as f32 * 414.0;
            let image =
                egui::Rect::from_min_size(egui::pos2(left, 435.0), egui::vec2(378.0, 213.0));
            let key = artwork_key(fixture, id).ok_or("exact current ordinary artwork source")?;
            require(
                fixture.app.ui.image_bytes(&key) == Some(16),
                "current decoded ordinary texture absent",
            )?;
            let texture = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh)
                        if mesh.texture_id != egui::TextureId::default()
                            && mesh.calc_bounds() == image
                            && shape.clip_rect.contains_rect(image) =>
                    {
                        Some(mesh.texture_id)
                    }
                    _ => None,
                })
                .ok_or("current card mesh bounds or clip")?;
            textures.push(texture);
            for (text, area) in [
                (
                    title,
                    egui::Rect::from_min_max(
                        egui::pos2(left, 656.0),
                        egui::pos2(left + 346.0, 700.0),
                    ),
                ),
                (
                    runtime,
                    egui::Rect::from_min_max(
                        egui::pos2(left + 280.0, 694.0),
                        egui::pos2(left + 378.0, 732.0),
                    ),
                ),
            ] {
                require(
                    text_inside(output, text, area),
                    "complete current card title/runtime visual bounds",
                )?;
            }
            if self.focus() == (Focus::Card { row: 0, column }) {
                require(
                    output.shapes.iter().any(|shape| {
                        matches!(&shape.shape,egui::Shape::Rect(r)
                    if r.rect == image.expand(8.0) && r.stroke.width == 8.0
                        && r.stroke.color == egui::Color32::from_rgb(181,138,22)
                        && shape.clip_rect.contains_rect(r.rect))
                    }),
                    "canonical current Sort card focus",
                )?;
            }
            let progress =
                egui::Rect::from_min_size(egui::pos2(left, 645.0), egui::vec2(378.0, 3.0));
            if id == "Related2" && self.fraction().is_some() {
                require(output.shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Rect(r)
                    if r.rect.min == progress.min && r.rect.height() == 3.0 && r.rect.width() == 94.5
                        && r.fill == egui::Color32::from_rgb(239,239,239))),"literal quarter-progress geometry")?;
            } else {
                require(
                    !output.shapes.iter().any(|shape| {
                        matches!(&shape.shape,egui::Shape::Rect(r)
                    if r.rect.height() == 3.0 && progress.intersects(r.rect))
                    }),
                    "current Sort card retained private progress",
                )?;
            }
        }
        require(textures[0] != textures[1], "distinct current card meshes")?;
        if signed_out {
            require(
                fixture.app.positions.is_none(),
                "logout retained private position snapshot",
            )?;
        }
        Ok(())
    }
    pub(super) fn pixels(self, pixels: &[u8]) -> bool {
        if matches!(self, Self::PendingTitle) {
            let gold = |x, y| {
                let rgb = pixel(pixels, x, y);
                rgb[0] > 120 && rgb[1] > 80 && rgb[2] < 70
            };
            return gold(550, 790)
                && gold(1382, 410)
                && gold(1340, 410)
                && pixel(pixels, 550, 410).iter().all(|v| *v < 20)
                && pixel(pixels, 480, 160)
                    .iter()
                    .all(|v| (24..=32).contains(v))
                && pixel(pixels, 180, 80).iter().all(|v| *v < 40);
        }
        let red = |rgb: &[u8]| rgb[0] > rgb[2].saturating_add(80);
        let blue = |rgb: &[u8]| rgb[2] > rgb[0].saturating_add(80);
        let first = pixel(pixels, 180, 480);
        let second = pixel(pixels, 594, 480);
        if !(if self.ascending() {
            red(first) && blue(second)
        } else {
            blue(first) && red(second)
        }) {
            return false;
        }
        let alpha_left = if self.ascending() { 150 } else { 564 };
        let fill = pixel(pixels, alpha_left + 12, 646);
        let track = pixel(pixels, alpha_left + 150, 646);
        if self.fraction().is_some() {
            fill.iter().all(|v| *v > 220) && (90..=190).contains(&track[1]) && track[0] > 150
        } else {
            red(fill) && red(track)
        }
    }
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
fn unchanged_reads(rendered: &mut Rendered<'_>, keys: &[(u32, i32)]) -> Result<(), &'static str> {
    require(
        local_reads(rendered.fixture),
        "Sort initial exact four-read boundary",
    )?;
    for key in keys {
        rendered.key(*key)?;
        require(
            local_reads(rendered.fixture),
            "local Sort input issued a read",
        )?;
    }
    Ok(())
}
fn capture(rendered: &mut Rendered<'_>, stage: Capture) -> Result<(), &'static str> {
    if !matches!(stage, Capture::PendingTitle) {
        admit_current_artwork(rendered.fixture)?;
    }
    rendered.capture(super::Capture::Sort(stage))
}
pub(super) fn journey(rendered: &mut Rendered<'_>) -> Result<(), &'static str> {
    rendered.wait(root_ready)?;
    unchanged_reads(rendered, &DRAFT_TITLE)?;
    capture(rendered, Capture::PendingTitle)?;
    unchanged_reads(rendered, &[SELECT, DOWN])?;
    capture(rendered, Capture::TitleAscending)?;
    unchanged_reads(rendered, &DRAFT_DESCENDING)?;
    require(
        sort_state(
            rendered.fixture,
            true,
            selection(Field::Title, Direction::Descending),
            selection(Field::Title, Direction::Ascending),
        ) && projection(rendered.fixture, true, Some(0.25)),
        "pending descending must not change applied order",
    )?;
    unchanged_reads(rendered, &[BACK])?;
    require(
        sort_state(
            rendered.fixture,
            false,
            selection(Field::Title, Direction::Descending),
            selection(Field::Title, Direction::Ascending),
        ) && rendered.fixture.app.ui.focus() == Focus::DetailTab(0)
            && rendered.fixture.app.ui.scroll_y() == 388.0
            && projection(rendered.fixture, true, Some(0.25)),
        "dismiss must preserve order and restore exact tab focus",
    )?;
    unchanged_reads(rendered, &[SELECT])?;
    require(
        sort_state(
            rendered.fixture,
            true,
            selection(Field::Title, Direction::Ascending),
            selection(Field::Title, Direction::Ascending),
        ) && projection(rendered.fixture, true, Some(0.25)),
        "dismiss/reopen must copy applied ascending order",
    )?;
    rendered
        .fixture
        .app
        .consume(&rendered.fixture.runtime, rendered.fixture.clock.now());
    require(
        rendered.fixture.app.ui.focus() == Focus::DetailSortOption(Field::Title)
            && rendered
                .fixture
                .app
                .output
                .as_ref()
                .is_some_and(|output| modal_shapes(output, Focus::DetailSortOption(Field::Title))),
        "reopened fresh modal must paint complete applied ascending choice and focus",
    )?;
    rendered.paint(None)?;
    unchanged_reads(rendered, &APPLY_DEFAULT)?;
    capture(rendered, Capture::DefaultRestored)?;
    unchanged_reads(rendered, &REAPPLY_TITLE)?;
    require(
        rendered.fixture.app.ui.focus() == (Focus::Card { row: 0, column: 1 })
            && projection(rendered.fixture, true, Some(0.25)),
        "sorted column1 must be exact clicked Related1 beta",
    )?;
    *rendered.fixture.script.detail_body.lock().unwrap() = None;
    rendered.key(SELECT)?;
    rendered.wait(child_ready)?;
    require(
        rendered.fixture.script.calls.lock().unwrap().len() == 6,
        "single exact sorted child Detail/IDs",
    )?;
    rendered.key(BACK)?;
    rendered
        .wait(|fixture| root_ready(fixture) && fixture.script.calls.lock().unwrap().len() == 7)?;
    capture(rendered, Capture::WarmBack)?;
    rendered.account()?;
    rendered.key(SELECT)?;
    rendered.wait(|fixture| {
        matches!(fixture.app.authentication.view(), LoginView::SignedOut)
            && fixture.issuer.revokes.load(Ordering::SeqCst) == 1
    })?;
    rendered.key(BACK)?;
    capture(rendered, Capture::SignedOutBack)?;
    require(
        exact_final_census(rendered.fixture),
        "exact seven-read Sort/logout census",
    )
}

fn title_direction_shapes(output: &egui::FullOutput, direction: Direction) -> bool {
    let (stem, head) = match direction {
        Direction::Ascending => (
            [egui::pos2(1340.0, 422.0), egui::pos2(1340.0, 398.0)],
            [
                egui::pos2(1332.0, 406.0),
                egui::pos2(1340.0, 398.0),
                egui::pos2(1348.0, 406.0),
            ],
        ),
        Direction::Descending => (
            [egui::pos2(1340.0, 398.0), egui::pos2(1340.0, 422.0)],
            [
                egui::pos2(1332.0, 414.0),
                egui::pos2(1340.0, 422.0),
                egui::pos2(1348.0, 414.0),
            ],
        ),
    };
    let gold = egui::Color32::from_rgb(181, 138, 22);
    output.shapes.iter().any(|shape| {
        matches!(&shape.shape,
        egui::Shape::LineSegment { points, stroke }
        if *points == stem && stroke.color == gold && (stroke.width - 2.6666667).abs() < 0.00001
            && stem.iter().all(|point| shape.clip_rect.contains(*point)))
    }) && output.shapes.iter().any(|shape| {
        matches!(&shape.shape,
            egui::Shape::Path(path)
            if path.points == head && path.stroke.color == egui::epaint::ColorMode::Solid(gold)
                && (path.stroke.width - 2.6666667).abs() < 0.00001
                && head.iter().all(|point| shape.clip_rect.contains(*point)))
    })
}

#[test]
fn sort_direction_is_visible_vector_geometry_for_both_actual_choices() {
    let mut fixture = sort_fixture_with_positions(true);
    fixture.wait(root_ready);
    for key in DRAFT_TITLE {
        fixture.key(key.0, key.1);
        fixture.pump();
    }
    assert!(local_reads(&fixture));
    assert!(
        title_direction_shapes(fixture.app.output.as_ref().unwrap(), Direction::Ascending),
        "current ascending direction must paint a complete unclipped vector arrow"
    );
    let mut missing = fixture.app.output.as_ref().unwrap().clone();
    missing.textures_delta.clear();
    missing.shapes.retain(|shape| {
        !matches!(&shape.shape, egui::Shape::LineSegment { points, .. }
        if *points == [egui::pos2(1340.0, 422.0), egui::pos2(1340.0, 398.0)])
    });
    assert!(
        !title_direction_shapes(&missing, Direction::Ascending),
        "missing stem refuses"
    );
    let mut clipped = fixture.app.output.as_ref().unwrap().clone();
    clipped.textures_delta.clear();
    for shape in &mut clipped.shapes {
        if matches!(&shape.shape, egui::Shape::Path(path)
            if path.points == [egui::pos2(1332.0, 406.0), egui::pos2(1340.0, 398.0), egui::pos2(1348.0, 406.0)])
        {
            shape.clip_rect = egui::Rect::ZERO;
        }
    }
    assert!(
        !title_direction_shapes(&clipped, Direction::Ascending),
        "clipped head refuses"
    );
    fixture.key(SELECT.0, SELECT.1);
    fixture.pump();
    fixture.key(DOWN.0, DOWN.1);
    fixture.pump();
    for key in DRAFT_DESCENDING {
        fixture.key(key.0, key.1);
        fixture.pump();
    }
    assert!(local_reads(&fixture));
    let output = fixture.app.output.as_ref().unwrap();
    assert!(
        title_direction_shapes(output, Direction::Descending),
        "current descending direction must paint a complete unclipped vector arrow"
    );
    assert!(!title_direction_shapes(output, Direction::Ascending));
}

#[test]
fn sort_capture_shapes_preserve_font_upload_and_match_actual_application_journey() {
    let mut fixture = fixture();
    fixture.wait(root_ready);
    let keys = |fixture: &mut Fixture, keys: &[(u32, i32)]| {
        assert!(local_reads(fixture));
        for key in keys {
            fixture.key(key.0, key.1);
            fixture.pump();
            assert!(local_reads(fixture));
        }
    };
    let fresh = |fixture: &mut Fixture, stage: Capture| {
        if !matches!(stage, Capture::PendingTitle) {
            admit_current_artwork(fixture).unwrap();
        }
        // CPU has no painter. Retire its unused work after the separate factory
        // font assertion; the opt-in SDL runner paints every texture delta.
        if let Some(mut output) = fixture.app.take_output() {
            output.textures_delta.clear();
        }
        fixture.pump();
        let mut output = fixture.app.take_output().unwrap();
        output.textures_delta.clear();
        stage.admit(fixture, &output).unwrap();
        output
    };
    keys(&mut fixture, &DRAFT_TITLE);
    let mut modal = fresh(&mut fixture, Capture::PendingTitle);
    let title = modal
        .shapes
        .iter_mut()
        .find(|shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.job.text=="Title"))
        .unwrap();
    title.clip_rect = egui::Rect::ZERO;
    assert!(
        !modal_shapes(&modal, Capture::PendingTitle.focus()),
        "clipped modal text must refuse"
    );
    keys(&mut fixture, &[SELECT, DOWN]);
    fresh(&mut fixture, Capture::TitleAscending);
    keys(&mut fixture, &DRAFT_DESCENDING);
    assert!(sort_state(
        &fixture,
        true,
        selection(Field::Title, Direction::Descending),
        selection(Field::Title, Direction::Ascending)
    ));
    assert!(projection(&fixture, true, Some(0.25)));
    keys(&mut fixture, &[BACK]);
    assert!(sort_state(
        &fixture,
        false,
        selection(Field::Title, Direction::Descending),
        selection(Field::Title, Direction::Ascending)
    ));
    assert_eq!(fixture.app.ui.focus(), Focus::DetailTab(0));
    assert_eq!(fixture.app.ui.scroll_y(), 388.0);
    assert!(projection(&fixture, true, Some(0.25)));
    assert!(!fixture.app.output.as_ref().unwrap().shapes.iter().any(
        |shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.job.text == "Sort by")
    ));
    keys(&mut fixture, &[SELECT]);
    assert!(sort_state(
        &fixture,
        true,
        selection(Field::Title, Direction::Ascending),
        selection(Field::Title, Direction::Ascending)
    ));
    assert!(projection(&fixture, true, Some(0.25)));
    assert_eq!(
        fixture.app.ui.focus(),
        Focus::DetailSortOption(Field::Title)
    );
    assert!(modal_shapes(
        fixture.app.output.as_ref().unwrap(),
        Focus::DetailSortOption(Field::Title)
    ));
    keys(&mut fixture, &APPLY_DEFAULT);
    fresh(&mut fixture, Capture::DefaultRestored);
    keys(&mut fixture, &REAPPLY_TITLE);
    assert_eq!(fixture.app.ui.focus(), Focus::Card { row: 0, column: 1 });
    *fixture.script.detail_body.lock().unwrap() = None;
    fixture.key(SELECT.0, SELECT.1);
    fixture.wait(child_ready);
    assert_eq!(fixture.script.calls.lock().unwrap().len(), 6);
    fixture.key(BACK.0, BACK.1);
    fixture.wait(|fixture| root_ready(fixture) && fixture.script.calls.lock().unwrap().len() == 7);
    fresh(&mut fixture, Capture::WarmBack);
    // Direct CPU logout is distinct from actual SDL Account navigation above.
    fixture
        .app
        .command(Command::Logout, fixture.runtime.handle());
    fixture.wait(|fixture| {
        matches!(fixture.app.authentication.view(), LoginView::SignedOut)
            && fixture.issuer.revokes.load(Ordering::SeqCst) == 1
    });
    fresh(&mut fixture, Capture::SignedOutBack);
    assert!(exact_final_census(&fixture));
}

#[test]
fn sort_pixel_admission_refuses_placeholders_wrong_order_and_private_progress() {
    let mut pixels = vec![39; 1920 * 1080 * 4];
    for stage in CAPTURES {
        assert!(!stage.pixels(&pixels));
    }
    let set = |pixels: &mut [u8], x: usize, y: usize, rgb: [u8; 3]| {
        let at = ((1079 - y) * 1920 + x) * 4;
        pixels[at..at + 3].copy_from_slice(&rgb);
    };
    for (x, y, rgb) in [
        (180, 480, [210, 30, 50]),
        (594, 480, [30, 50, 200]),
        (162, 646, [239, 239, 239]),
        (300, 646, [222, 110, 122]),
    ] {
        set(&mut pixels, x, y, rgb);
    }
    assert!(Capture::TitleAscending.pixels(&pixels));
    assert!(!Capture::DefaultRestored.pixels(&pixels));
    assert!(!Capture::WarmBack.pixels(&pixels));
    set(&mut pixels, 162, 646, [210, 30, 50]);
    set(&mut pixels, 300, 646, [210, 30, 50]);
    assert!(Capture::WarmBack.pixels(&pixels));
    assert!(Capture::SignedOutBack.pixels(&pixels));
    set(&mut pixels, 180, 480, [30, 50, 200]);
    set(&mut pixels, 594, 480, [210, 30, 50]);
    set(&mut pixels, 576, 646, [239, 239, 239]);
    set(&mut pixels, 714, 646, [222, 110, 122]);
    assert!(Capture::DefaultRestored.pixels(&pixels));
    assert!(!Capture::TitleAscending.pixels(&pixels));
    for (x, y, rgb) in [
        (550, 790, [181, 138, 22]),
        (1382, 410, [181, 138, 22]),
        (1340, 410, [181, 138, 22]),
        (550, 410, [11, 11, 11]),
        (480, 160, [28, 28, 28]),
        (180, 80, [10, 10, 10]),
    ] {
        set(&mut pixels, x, y, rgb);
    }
    assert!(Capture::PendingTitle.pixels(&pixels));
    set(&mut pixels, 1340, 410, [11, 11, 11]);
    assert!(
        !Capture::PendingTitle.pixels(&pixels),
        "missing direction refuses pixel admission"
    );
    set(&mut pixels, 1340, 410, [181, 138, 22]);
    set(&mut pixels, 1382, 410, [11, 11, 11]);
    assert!(!Capture::PendingTitle.pixels(&pixels));
}

#[test]
fn sort_read_prefix_refuses_extra_detail_and_wrong_order() {
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
#[ignore = "synthetic offline Sort/CW/Session/decoded artwork; Root serialized actual SDL/GLES executor"]
fn native_synthetic_detail_sort_apply_default_back_and_logout_end_to_end() {
    run(Journey::Sort);
}
