//! Screen-capture glue for the ext-image-copy-capture-v1 protocol.
//!
//! The Wayland protocol plumbing lives in `smithay::wayland::image_copy_capture`.
//! Our role is to:
//!
//! 1. Accept `frame()` callbacks from the handler and queue them against the
//!    Output they belong to (see `state.rs` -> `ImageCopyCaptureHandler::frame`).
//! 2. After we finish rendering an output, re-render the same element list
//!    into an offscreen GLES texture, read the pixels back, and memcpy them
//!    into the client's shm buffer.
//! 3. Signal `Frame::success` with the presentation timestamp so clients
//!    (OBS, grim, the xdg-desktop-portal screen-cast backend) see a real
//!    frame instead of the previous hard-coded failure.
//!
//! Scope of this implementation:
//! - One-shot capture per `capture()` request (no streaming `request_frame`
//!   continuous-update loop).
//! - shm buffers only. TODO(screencopy-dmabuf): dmabuf path for zero-copy
//!   screencast into the portal pipewire node.
//! - Whole-output capture (no region crop, no explicit cursor compositing —
//!   we re-render the same element list the backend just used).
//! - Generic over the renderer: works with both the winit `GlesRenderer`
//!   path and the udev `MultiRenderer<GbmGlesBackend, GbmGlesBackend>` path.
//!   Both back ends use a `GlesTexture` as the offscreen target type.

use std::{fmt::Debug, ptr, time::Duration};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Color32F, ExportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::RenderElement,
        },
    },
    output::{Output, WeakOutput},
    reexports::wayland_server::protocol::wl_shm,
    utils::{Buffer as BufferCoords, Rectangle, Size, Transform},
    wayland::{
        image_copy_capture::{CaptureFailureReason, Frame, SessionRef},
        shm::{self, with_buffer_contents_mut},
    },
};
use tracing::{debug, info, warn};

/// A capture request that arrived via the protocol handler and is waiting
/// for the next render of the owning output.
pub struct PendingScreencopy {
    /// Weak reference to the output this capture is bound to.
    pub output: WeakOutput,
    /// Session the frame belongs to (kept for telemetry; unused today).
    pub session: SessionRef,
    /// The frame we must eventually `success()` or `fail()` on.
    pub frame: Frame,
}

impl std::fmt::Debug for PendingScreencopy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingScreencopy")
            .field("output_alive", &self.output.upgrade().is_some())
            .finish()
    }
}

/// Drain the pending screencopy queue for the given output and fulfill each
/// request by rendering the scene into an offscreen texture and memcpy'ing
/// the pixels into the client's shm buffer.
///
/// `elements` is the same element list the backend just rendered to the real
/// framebuffer; we re-render it into a fresh texture so the client gets an
/// exact copy of what was just presented to the user.
///
/// `presented_at` is the compositor-clock time the frame was presented; it
/// becomes the `presentation_time` event on the captured frame.
pub fn drain_pending_for_output<R, T, E>(
    pending: &mut Vec<PendingScreencopy>,
    output: &Output,
    renderer: &mut R,
    elements: &[E],
    presented_at: Duration,
) where
    R: Renderer + Bind<T> + Offscreen<T> + ExportMem,
    R::TextureId: Texture,
    R::Error: Debug,
    T: Texture + 'static,
    E: RenderElement<R>,
{
    if pending.is_empty() {
        return;
    }

    // Partition: keep requests whose output is not this one; consume the rest.
    let mut remaining = Vec::with_capacity(pending.len());
    let mut to_service: Vec<PendingScreencopy> = Vec::new();
    for entry in pending.drain(..) {
        match entry.output.upgrade() {
            Some(entry_output) if &entry_output == output => to_service.push(entry),
            Some(_) => remaining.push(entry),
            None => {
                // Output went away; fail the capture with a well-defined reason.
                entry.frame.fail(CaptureFailureReason::Stopped);
            }
        }
    }
    *pending = remaining;

    if to_service.is_empty() {
        return;
    }

    // Figure out the buffer geometry the client was told about in
    // `capture_constraints()` — output mode size, identity transform, scale 1.
    let Some(mode) = output.current_mode() else {
        warn!("screencopy: output has no current mode; failing pending frames");
        for entry in to_service {
            entry.frame.fail(CaptureFailureReason::Stopped);
        }
        return;
    };
    let buffer_size = mode
        .size
        .to_logical(1)
        .to_buffer(1, Transform::Normal);

    for entry in to_service {
        let output_name = entry
            .output
            .upgrade()
            .map(|o| o.name())
            .unwrap_or_else(|| "<gone>".into());
        match try_capture(renderer, elements, &entry.frame, buffer_size) {
            Ok(()) => {
                info!(
                    output = %output_name,
                    w = buffer_size.w,
                    h = buffer_size.h,
                    "screencopy: frame captured",
                );
                entry.frame.success(Transform::Normal, None, presented_at);
            }
            Err(reason) => {
                debug!(?reason, output = %output_name, "screencopy: frame failed");
                entry.frame.fail(reason);
            }
        }
    }
}

/// Render the element list into an offscreen texture matching the client's
/// buffer size, read the pixels back, and copy them into the shm buffer
/// currently attached to `frame`.
fn try_capture<R, T, E>(
    renderer: &mut R,
    elements: &[E],
    frame: &Frame,
    buffer_size: Size<i32, BufferCoords>,
) -> Result<(), CaptureFailureReason>
where
    R: Renderer + Bind<T> + Offscreen<T> + ExportMem,
    R::TextureId: Texture,
    R::Error: Debug,
    T: Texture + 'static,
    E: RenderElement<R>,
{
    let wl_buffer = frame.buffer();

    // --- validate the buffer is shm and matches our advertised constraints ---
    let buffer_info = shm::with_buffer_contents(&wl_buffer, |_, _, data| data).map_err(|_| {
        // Not an shm buffer (probably dmabuf). TODO(screencopy-dmabuf).
        CaptureFailureReason::BufferConstraints
    })?;

    if buffer_info.width != buffer_size.w
        || buffer_info.height != buffer_size.h
        || buffer_info.stride < buffer_size.w * 4
    {
        return Err(CaptureFailureReason::BufferConstraints);
    }

    // Both advertised shm formats store pixels little-endian BGRA in memory,
    // which matches Fourcc::Argb8888 on the GLES side. The texture we create
    // and the pixels we read back are in the same byte layout, so no swizzle
    // is needed for either format.
    let fourcc = match buffer_info.format {
        wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888 => Fourcc::Argb8888,
        _ => return Err(CaptureFailureReason::BufferConstraints),
    };

    // --- allocate an offscreen texture and damage tracker ---
    let mut texture: T =
        Offscreen::<T>::create_buffer(renderer, fourcc, buffer_size).map_err(|err| {
            warn!(?err, "screencopy: failed to allocate offscreen texture");
            CaptureFailureReason::Unknown
        })?;

    // Fresh, full-damage tracker. For streaming captures we'd cache one per
    // session; see TODO at the top of the file.
    let mut damage_tracker =
        OutputDamageTracker::new((buffer_size.w, buffer_size.h), 1.0, Transform::Normal);

    // --- render the elements into the texture ---
    {
        let mut framebuffer = Bind::<T>::bind(renderer, &mut texture).map_err(|err| {
            warn!(?err, "screencopy: failed to bind texture as framebuffer");
            CaptureFailureReason::Unknown
        })?;
        damage_tracker
            .render_output(
                renderer,
                &mut framebuffer,
                0,
                elements,
                Color32F::TRANSPARENT,
            )
            .map_err(|err| {
                warn!(?err, "screencopy: render_output failed");
                CaptureFailureReason::Unknown
            })?;
    }

    // --- read the framebuffer back through a PBO and memcpy into shm ---
    let bytes_len = (buffer_size.w as usize) * (buffer_size.h as usize) * 4;
    // Re-bind the texture as framebuffer so `copy_framebuffer` reads from it.
    let mapping = {
        let framebuffer = Bind::<T>::bind(renderer, &mut texture).map_err(|err| {
            warn!(?err, "screencopy: failed to rebind texture for readback");
            CaptureFailureReason::Unknown
        })?;
        ExportMem::copy_framebuffer(renderer, &framebuffer, Rectangle::from_size(buffer_size), fourcc)
            .map_err(|err| {
                warn!(?err, "screencopy: copy_framebuffer failed");
                CaptureFailureReason::Unknown
            })?
        // framebuffer dropped at end of block
    };

    let pixels = ExportMem::map_texture(renderer, &mapping).map_err(|err| {
        warn!(?err, "screencopy: map_texture failed");
        CaptureFailureReason::Unknown
    })?;
    if pixels.len() < bytes_len {
        warn!(
            pixels_len = pixels.len(),
            expected = bytes_len,
            "screencopy: mapped pixel buffer smaller than expected"
        );
        return Err(CaptureFailureReason::Unknown);
    }

    with_buffer_contents_mut(&wl_buffer, |dst_ptr, dst_len, data| {
        // Defensive recheck: the pool could have been resized between the
        // earlier peek and now.
        if data.width != buffer_size.w || data.height != buffer_size.h {
            return Err(CaptureFailureReason::BufferConstraints);
        }
        if dst_len < bytes_len {
            return Err(CaptureFailureReason::BufferConstraints);
        }
        let stride = data.stride as usize;
        let row_bytes = (buffer_size.w as usize) * 4;

        // Intentionally reading only `bytes_len` bytes from `pixels`; length
        // already verified above.
        if stride == row_bytes {
            // Contiguous fast path.
            // SAFETY: src is at least `bytes_len` bytes long (verified above);
            // dst is at least `dst_len >= bytes_len` bytes of writable shm
            // memory. The ranges do not overlap: PBO mapping vs. client shm.
            unsafe {
                ptr::copy_nonoverlapping(pixels.as_ptr(), dst_ptr.cast::<u8>(), bytes_len);
            }
        } else {
            // Client picked a padded stride; copy row-by-row so we don't
            // scribble into padding bytes.
            for y in 0..buffer_size.h as usize {
                let src_off = y * row_bytes;
                let dst_off = y * stride;
                if dst_off + row_bytes > dst_len {
                    return Err(CaptureFailureReason::BufferConstraints);
                }
                // SAFETY: src_off + row_bytes <= bytes_len, dst range just
                // bounds-checked above.
                unsafe {
                    ptr::copy_nonoverlapping(
                        pixels.as_ptr().add(src_off),
                        dst_ptr.add(dst_off),
                        row_bytes,
                    );
                }
            }
        }
        Ok(())
    })
    .map_err(|_| CaptureFailureReason::BufferConstraints)??;

    Ok(())
}
