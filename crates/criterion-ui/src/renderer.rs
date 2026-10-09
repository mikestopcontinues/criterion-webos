use glow::HasContext;
use std::{marker::PhantomData, rc::Rc, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    UnsupportedContext,
    Initialization,
    Disposed,
    InvalidSurface,
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedContext => "OpenGL ES 2 with uint indices or newer is required",
            Self::Initialization => "unable to initialize the renderer",
            Self::Disposed => "renderer is disposed",
            Self::InvalidSurface => "invalid render surface",
        })
    }
}
impl std::error::Error for RenderError {}
/// Admit an actual `GL_VERSION` value, rather than the SDL requested version.
pub fn check_gles_capabilities(value: &str, uint_indices: bool) -> Result<(), RenderError> {
    let Some(version) = value
        .strip_prefix("OpenGL ES ")
        .and_then(|v| v.split_whitespace().next())
    else {
        return Err(RenderError::UnsupportedContext);
    };
    let Some((major, minor)) = version.split_once('.') else {
        return Err(RenderError::UnsupportedContext);
    };
    if major
        .parse::<u8>()
        .is_ok_and(|v| v >= 3 || v == 2 && uint_indices)
        && minor.parse::<u8>().is_ok()
    {
        Ok(())
    } else {
        Err(RenderError::UnsupportedContext)
    }
}

/// Main-thread GLES painter. Its GL context must remain current until disposal.
pub struct GlowRenderer {
    painter: Option<egui_glow::Painter>,
    _main_thread: PhantomData<Rc<()>>,
}
impl GlowRenderer {
    /// # Safety
    /// `gl` must refer to the current context on this thread. That context must
    /// remain alive/current whenever this renderer paints or is destroyed.
    /// Dispose/drop this renderer before the platform window/context.
    pub unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, RenderError> {
        // SAFETY: the caller admits a current live context.
        let version = unsafe { gl.get_parameter_string(glow::VERSION) };
        check_gles_capabilities(
            &version,
            gl.supported_extensions()
                .contains("GL_OES_element_index_uint"),
        )?;
        let painter = egui_glow::Painter::new(gl, "", Some(egui_glow::ShaderVersion::Es100), false)
            .map_err(|_| RenderError::Initialization)?;
        Ok(Self {
            painter: Some(painter),
            _main_thread: PhantomData,
        })
    }
    pub fn paint(
        &mut self,
        size: [u32; 2],
        context: &egui::Context,
        output: &mut egui::FullOutput,
    ) -> Result<(), RenderError> {
        if size.contains(&0) || size.iter().any(|side| *side > 8192) {
            return Err(RenderError::InvalidSurface);
        }
        let painter = self.painter.as_mut().ok_or(RenderError::Disposed)?;
        let fit = fit_canvas(size)?;
        let mut primitives =
            context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let transform =
            egui::emath::TSTransform::new(egui::vec2(fit.offset[0], fit.offset[1]), fit.scale);
        for primitive in &mut primitives {
            primitive.clip_rect = transform * primitive.clip_rect;
            match &mut primitive.primitive {
                egui::epaint::Primitive::Mesh(mesh) => {
                    for vertex in &mut mesh.vertices {
                        vertex.pos = transform * vertex.pos;
                    }
                }
                egui::epaint::Primitive::Callback(callback) => {
                    callback.rect = transform * callback.rect;
                }
            }
        }
        painter.clear(size, [0.0, 0.0, 0.0, 1.0]);
        painter.paint_and_update_textures(size, 1.0, &primitives, &mut output.textures_delta);
        Ok(())
    }
    pub fn destroy(&mut self) {
        if let Some(mut painter) = self.painter.take() {
            painter.destroy();
        }
    }
}
impl Drop for GlowRenderer {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// The same centered logical canvas used by platform pointer admission.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasFit {
    pub scale: f32,
    pub offset: [f32; 2],
}
pub fn fit_canvas(size: [u32; 2]) -> Result<CanvasFit, RenderError> {
    if size.contains(&0) || size.iter().any(|side| *side > 8192) {
        return Err(RenderError::InvalidSurface);
    }
    let scale = (f64::from(size[0]) / f64::from(crate::LOGICAL_SIZE[0]))
        .min(f64::from(size[1]) / f64::from(crate::LOGICAL_SIZE[1]));
    let width = (f64::from(crate::LOGICAL_SIZE[0]) * scale).round() as u32;
    let height = (f64::from(crate::LOGICAL_SIZE[1]) * scale).round() as u32;
    Ok(CanvasFit {
        scale: scale as f32,
        offset: [
            ((size[0] - width) / 2) as f32,
            ((size[1] - height) / 2) as f32,
        ],
    })
}
