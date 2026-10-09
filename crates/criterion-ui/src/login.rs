use crate::view::{GOLD, MUTED, WHITE, button, label, paragraph};
use crate::{Action, AppUi, Command, Focus, Page};
use egui::{Color32, Pos2, Rect, Vec2};
use std::sync::Arc;

/// Borrowed presentation of the runtime-owned session. Private values have no Debug implementation.
#[derive(Clone, Copy, Default)]
pub enum LoginView<'a> {
    #[default]
    SignedOut,
    Requesting,
    Awaiting {
        /// Unique runtime generation for each activation challenge; never reuse it.
        generation: u64,
        user_code: &'a str,
        verification_uri_complete: &'a str,
        remaining_seconds: u32,
    },
    SignedIn,
    SigningOut,
    Expired,
    Denied,
    Error,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    SignedOut,
    Requesting,
    Awaiting,
    SignedIn,
    SigningOut,
    Expired,
    Denied,
    Error,
}
impl LoginView<'_> {
    pub(crate) fn phase(self) -> Phase {
        match self {
            Self::SignedOut => Phase::SignedOut,
            Self::Requesting => Phase::Requesting,
            Self::Awaiting { .. } => Phase::Awaiting,
            Self::SignedIn => Phase::SignedIn,
            Self::SigningOut => Phase::SigningOut,
            Self::Expired => Phase::Expired,
            Self::Denied => Phase::Denied,
            Self::Error => Phase::Error,
        }
    }
}
#[derive(Default)]
pub(crate) struct LoginState {
    phase: Option<Phase>,
    generation: Option<u64>,
    qr_generation: Option<u64>,
    qr: Option<Arc<egui::Mesh>>,
}
impl AppUi {
    pub(crate) fn sync_login(&mut self, login: LoginView<'_>) {
        if self.page() != Page::Login {
            self.login = LoginState::default();
            return;
        }
        let phase = login.phase();
        let generation = if let LoginView::Awaiting { generation, .. } = login {
            Some(generation)
        } else {
            None
        };
        if self.login.phase != Some(phase) || self.login.generation != generation {
            self.login.qr_generation = None;
            self.login.qr = None;
            self.login.generation = generation;
            self.login.phase = Some(phase);
            self.pointer_press = None;
            self.pointer_layout_focus = None;
            self.focus = if matches!(
                phase,
                Phase::Awaiting | Phase::Requesting | Phase::SigningOut
            ) {
                Focus::LoginCancel
            } else {
                Focus::LoginPrimary
            };
        }
    }
    pub(crate) fn handle_login(
        &mut self,
        action: Action,
        login: LoginView<'_>,
    ) -> Option<Vec<Command>> {
        if self.page() != Page::Login || matches!(self.focus(), Focus::Rail(_)) {
            return None;
        }
        let pending = matches!(login, LoginView::Requesting | LoginView::Awaiting { .. });
        if action == Action::Back
            || (action == Action::Select && self.focus() == Focus::LoginCancel)
        {
            let mut commands = Vec::new();
            if pending {
                commands.push(Command::CancelAuthentication);
            }
            if let Some(page) = self.restore_previous() {
                commands.push(Command::Restore(page));
            }
            self.login = LoginState::default();
            return Some(commands);
        }
        if action == Action::Select && self.focus() == Focus::LoginPrimary {
            return Some(match login {
                LoginView::SignedOut => vec![Command::Authenticate],
                LoginView::SignedIn => vec![Command::Logout],
                LoginView::Expired | LoginView::Denied | LoginView::Error => {
                    vec![Command::RetryAuthentication]
                }
                LoginView::Requesting | LoginView::Awaiting { .. } | LoginView::SigningOut => {
                    vec![]
                }
            });
        }
        if action == Action::Left {
            self.return_focus = self.focus();
            self.focus = Focus::Rail(crate::RailItem::Login);
        }
        Some(vec![])
    }
}

// Placement is currently unmeasured against the official TV activation screen.
pub(crate) fn login_button() -> Rect {
    Rect::from_min_size(Pos2::new(210.0, 840.0), Vec2::new(400.0, 82.0))
}
fn qr_mesh(uri: &str) -> Option<Arc<egui::Mesh>> {
    if uri.is_empty() || uri.len() > 2048 {
        return None;
    }
    let code = qrcode::QrCode::new(uri.as_bytes()).ok()?;
    let width = code.width();
    if width > 177 {
        return None;
    }
    // A four-module quiet zone surrounds the standard QR symbol.
    let step = 520.0 / (width + 8) as f32;
    let origin = Pos2::new(1225.0 + step * 4.0, 260.0 + step * 4.0);
    let mut mesh = egui::Mesh::default();
    for y in 0..width {
        let mut x = 0;
        while x < width {
            if code[(x, y)] != qrcode::Color::Dark {
                x += 1;
                continue;
            }
            let start = x;
            while x < width && code[(x, y)] == qrcode::Color::Dark {
                x += 1;
            }
            mesh.add_colored_rect(
                Rect::from_min_max(
                    origin + Vec2::new(start as f32 * step, y as f32 * step),
                    origin + Vec2::new(x as f32 * step, (y + 1) as f32 * step),
                ),
                Color32::BLACK,
            );
        }
    }
    Some(Arc::new(mesh))
}
impl AppUi {
    pub(crate) fn paint_login(&mut self, p: &egui::Painter, login: LoginView<'_>) {
        label(
            p,
            [210.0, 150.0],
            if matches!(login, LoginView::SignedIn | LoginView::SigningOut) {
                "Account"
            } else {
                "Log In"
            },
            60.0,
            WHITE,
            1300.0,
        );
        let text = match login {
            LoginView::SignedOut => "Log in with your Criterion Channel account.",
            LoginView::Requesting => "Requesting your activation code…",
            LoginView::Awaiting { .. } => "Scan the QR code with your phone, or visit",
            LoginView::SignedIn => "You are logged in to The Criterion Channel.",
            LoginView::SigningOut => "Logging out…",
            LoginView::Expired => "Your activation code expired. Try again for a new code.",
            LoginView::Denied => "Activation was declined. Try again to log in.",
            LoginView::Error => "Unable to connect. Check your connection and try again.",
        };
        paragraph(p, [210.0, 285.0], text, 38.0, WHITE, 950.0, (3, 256));
        if let LoginView::Awaiting {
            generation,
            user_code,
            verification_uri_complete,
            remaining_seconds,
        } = login
        {
            label(
                p,
                [210.0, 425.0],
                "https://login.criterion.com/activate",
                38.0,
                GOLD,
                950.0,
            );
            label(
                p,
                [210.0, 510.0],
                "and enter this code:",
                34.0,
                MUTED,
                950.0,
            );
            if !user_code.is_empty()
                && user_code.len() <= 128
                && !user_code.chars().any(char::is_control)
            {
                let size = if user_code.len() <= 16 {
                    64.0
                } else if user_code.len() <= 32 {
                    48.0
                } else {
                    28.0
                };
                let mut job = egui::text::LayoutJob::simple(
                    user_code.to_owned(),
                    egui::FontId::monospace(size),
                    WHITE,
                    950.0,
                );
                job.wrap.max_rows = 4;
                job.wrap.break_anywhere = true;
                p.galley(Pos2::new(210.0, 565.0), p.layout_job(job), WHITE);
            } else {
                label(
                    p,
                    [210.0, 565.0],
                    "Activation code unavailable",
                    34.0,
                    WHITE,
                    950.0,
                );
            }
            let remaining = remaining_seconds.min(3600);
            label(
                p,
                [210.0, 735.0],
                &format!("Expires in {}:{:02}", remaining / 60, remaining % 60),
                30.0,
                MUTED,
                950.0,
            );
            if self.login.qr_generation != Some(generation) {
                self.login.qr_generation = Some(generation);
                self.login.qr = qr_mesh(verification_uri_complete);
            }
            if let Some(mesh) = &self.login.qr {
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(1225.0, 260.0), Vec2::splat(520.0)),
                    0,
                    Color32::WHITE,
                );
                p.add(egui::Shape::Mesh(mesh.clone()));
            } else {
                paragraph(
                    p,
                    [1225.0, 430.0],
                    "Use the website and activation code to log in.",
                    34.0,
                    MUTED,
                    520.0,
                    (4, 128),
                );
            }
        }
        let pending = matches!(login, LoginView::Requesting | LoginView::Awaiting { .. });
        let closing = matches!(login, LoginView::SigningOut);
        let focus = if pending || closing {
            Focus::LoginCancel
        } else {
            Focus::LoginPrimary
        };
        button(
            p,
            login_button(),
            match login {
                LoginView::SignedOut => "LOG IN",
                LoginView::SignedIn => "LOG OUT",
                LoginView::SigningOut => "BACK",
                LoginView::Requesting | LoginView::Awaiting { .. } => "CANCEL",
                LoginView::Expired | LoginView::Denied | LoginView::Error => "TRY AGAIN",
            },
            self.focus() == focus,
        );
    }
}
