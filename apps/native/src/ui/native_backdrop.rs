//! The layer the app's window draws its backdrops in (`backdrop`), under GPUI's drawing of the window: a view of Wings'
//! own between the window's vibrancy (`main.rs`) and GPUI's view, hosting a Core Animation layer for each slot that shows
//! a backdrop. A slot's layer clips to the card's rounded corners and holds a Metal layer, which a step draws into
//! straight from `gradient.metal`: a gradient's frame at half the slot's device pixels, smoothed up, or a pattern of
//! pixels one pixel a cell, scaled up without smoothing.
//!
//! GPUI's window is see-through where it paints nothing (`WindowBackgroundAppearance::Transparent`: its Metal layer isn't
//! opaque and each frame starts clear), and its glass leaves a hole where the slot is (`backdrop::window_glass`), so the
//! layer shows there under the page.

use std::{collections::HashMap, ffi::c_void, mem, ptr::NonNull};

use gpui::{Bounds, Pixels, SharedString, Window};
use objc2::{MainThreadMarker, msg_send, rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindowOrderingMode};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLLoadAction,
    MTLPixelFormat, MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor, MTLRenderPipelineState,
    MTLResourceOptions, MTLStoreAction,
};
use objc2_quartz_core::{CALayer, CAMetalDrawable, CAMetalLayer, CATransaction, kCAFilterLinear, kCAFilterNearest};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{
    backdrop::{Drawing, Placed, Present},
    shader,
};
use crate::pixels::SHADES;

/// A pattern's grid and its shades' colours, laid out as `Cells` in `gradient.metal` (128 bytes).
#[repr(C)]
// Read by the GPU, not by Rust.
#[allow(dead_code)]
struct CellColors {
    cols: u32,
    rows: u32,
    _pad: [u32; 2],
    colors: [[f32; 4]; SHADES],
}

/// The GPU, its queue and the two pipelines that draw into the layers.
struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    gradient: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    cells: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
}

impl Gpu {
    fn new() -> Result<Self, String> {
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let library = shader::library(&device)?;
        // Untagged BGRA, as GPUI's own layer, so a colour here is the colour GPUI paints for it.
        let gradient = shader::pipeline(&device, &library, "gradient_rgb", MTLPixelFormat::BGRA8Unorm)?;
        let cells = shader::pipeline(&device, &library, "cells_rgb", MTLPixelFormat::BGRA8Unorm)?;
        let queue = device.newCommandQueue().ok_or("no Metal command queue")?;
        Ok(Self { device, queue, gradient, cells })
    }

    /// Draws all of `layer`'s next drawable with `pipeline`, its fragment reading `uniforms` at buffer 0 and `shades` at 1,
    /// and presents it once the GPU is done, without waiting. A step whose layer has no drawable free is skipped.
    fn present<T>(
        &self,
        layer: &CAMetalLayer,
        pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
        uniforms: &T,
        shades: Option<&[u8]>,
    ) -> Result<(), String> {
        let Some(drawable) = layer.nextDrawable() else { return Ok(()) };
        let work = self.queue.commandBuffer().ok_or("no command buffer")?;
        let descriptor = MTLRenderPassDescriptor::new();
        // SAFETY: Index zero is within the pass's colour attachments.
        let attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
        attachment.setTexture(Some(&*drawable.texture()));
        // Every pixel is drawn, so what was there before needn't be loaded.
        attachment.setLoadAction(MTLLoadAction::DontCare);
        attachment.setStoreAction(MTLStoreAction::Store);
        let encoder = work.renderCommandEncoderWithDescriptor(&descriptor).ok_or("no render encoder")?;
        encoder.setRenderPipelineState(pipeline);
        let buffer = match shades {
            // SAFETY: The bytes are live for the call, which copies them into a buffer of their length.
            Some(shades) => Some(
                unsafe {
                    self.device.newBufferWithBytes_length_options(
                        NonNull::from(shades).cast::<c_void>(),
                        shades.len(),
                        MTLResourceOptions::StorageModeShared,
                    )
                }
                .ok_or("no buffer for the cells")?,
            ),
            None => None,
        };
        // SAFETY: The bytes are a live `T`, laid out as the fragment's buffer 0, which Metal copies; buffer 1 is the
        // cells, one byte each, which the command buffer keeps until it is done; three vertices make the one triangle
        // `gradient_vertex` draws.
        unsafe {
            encoder.setFragmentBytes_length_atIndex(NonNull::from(uniforms).cast::<c_void>(), mem::size_of::<T>(), 0);
            if let Some(buffer) = &buffer {
                encoder.setFragmentBuffer_offset_atIndex(Some(&**buffer), 0, 1);
            }
            encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        }
        encoder.endEncoding();
        work.presentDrawable(ProtocolObject::from_ref(&*drawable));
        work.commit();
        Ok(())
    }
}

/// A slot's layers: the one that clips to the card's corners, and the Metal layer in it.
struct Layers {
    clip: Retained<CALayer>,
    metal: Retained<CAMetalLayer>,
    /// Whether the Metal layer holds a pattern's cells, sized by its grid, rather than a gradient over all of the clip.
    cells: bool,
}

impl Layers {
    fn new(device: &ProtocolObject<dyn MTLDevice>, root: &CALayer) -> Self {
        let clip = CALayer::new();
        clip.setMasksToBounds(true);
        clip.setHidden(true);
        let metal = CAMetalLayer::new();
        metal.setDevice(Some(device));
        metal.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        // Every pixel is drawn opaque.
        metal.setOpaque(true);
        unanimated(|| {
            clip.addSublayer(&metal);
            root.addSublayer(&clip);
        });
        Self { clip, metal, cells: false }
    }
}

/// Runs `change` on layers as one change of the screen, without Core Animation's implicit animations, which would ease
/// each move over a quarter of a second.
fn unanimated(change: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    change();
    CATransaction::commit();
}

fn rect(bounds: Bounds<Pixels>) -> CGRect {
    let points = |value: Pixels| f64::from(f32::from(value));
    CGRect::new(
        CGPoint::new(points(bounds.origin.x), points(bounds.origin.y)),
        CGSize::new(points(bounds.size.width), points(bounds.size.height)),
    )
}

/// The window's view under GPUI's, and the layers of the slots that show a backdrop in it.
pub(crate) struct Host {
    view: Retained<NSView>,
    root: Retained<CALayer>,
    gpu: Gpu,
    slots: HashMap<SharedString, Layers>,
}

impl Host {
    /// A view of its own under GPUI's in `window`, with Metal set up to draw in it, or why not.
    pub(crate) fn new(window: &Window) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let handle = HasWindowHandle::window_handle(window).map_err(|error| format!("no window handle: {error}"))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("not an AppKit window".to_owned());
        };
        // SAFETY: GPUI's window view, alive while its window is.
        let gpui = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        // SAFETY: The window's content view, which holds GPUI's view for as long as the window is open.
        let content = unsafe { gpui.superview() }.ok_or("GPUI's view isn't in a window")?;
        let gpu = Gpu::new()?;
        let root = CALayer::new();
        // GPUI's points run down from the window's top left corner.
        root.setGeometryFlipped(true);
        let view = NSView::initWithFrame(mtm.alloc(), gpui.frame());
        // A layer-hosting view: given its layer before it wants one, AppKit leaves what is in the layer to Wings.
        // SAFETY: `setLayer:` takes a CALayer, which the view keeps.
        let _: () = unsafe { msg_send![&*view, setLayer: &*root] };
        view.setWantsLayer(true);
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        // Right under GPUI's view, so over the vibrancy under it; GPUI's view, over all of it, takes the pointer.
        content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(gpui));
        Ok(Self { view, root, gpu, slots: HashMap::new() })
    }
}

impl Present for Host {
    fn show(&mut self, slot: &SharedString, placed: &Placed, drawing: Option<&Drawing>) -> Result<(), String> {
        let layers = self.slots.entry(slot.clone()).or_insert_with(|| Layers::new(&self.gpu.device, &self.root));
        let frame = rect(placed.frame);
        unanimated(|| {
            layers.clip.setFrame(frame);
            layers.clip.setCornerRadius(f64::from(placed.radius));
            layers.clip.setContentsScale(f64::from(placed.scale));
            layers.clip.setHidden(!placed.visible);
            match drawing {
                Some(Drawing::Cells { cell, cols, rows, .. }) => {
                    // SAFETY: A static string Core Animation defines.
                    layers.metal.setMagnificationFilter(unsafe { kCAFilterNearest });
                    // From the slot's top left corner, a whole cell wide and tall each: the clip cuts the last ones.
                    let size = CGSize::new(f64::from(*cols as f32 * cell), f64::from(*rows as f32 * cell));
                    layers.metal.setFrame(CGRect::new(CGPoint::ZERO, size));
                    layers.metal.setDrawableSize(CGSize::new(f64::from(*cols), f64::from(*rows)));
                    layers.cells = true;
                }
                Some(Drawing::Gradient(uniforms)) => {
                    // SAFETY: A static string Core Animation defines.
                    layers.metal.setMagnificationFilter(unsafe { kCAFilterLinear });
                    layers.metal.setDrawableSize(CGSize::new(f64::from(uniforms.size[0]), f64::from(uniforms.size[1])));
                    layers.cells = false;
                }
                None => {}
            }
            if !layers.cells {
                layers.metal.setFrame(CGRect::new(CGPoint::ZERO, frame.size));
            }
        });
        match drawing {
            Some(Drawing::Cells { cols, rows, shades, colors, .. }) => {
                let uniforms = CellColors { cols: *cols, rows: *rows, _pad: [0; 2], colors: *colors };
                self.gpu.present(&layers.metal, &self.gpu.cells, &uniforms, Some(*shades))
            }
            Some(Drawing::Gradient(uniforms)) => self.gpu.present(&layers.metal, &self.gpu.gradient, *uniforms, None),
            None => Ok(()),
        }
    }

    fn remove(&mut self, slot: &SharedString) {
        if let Some(layers) = self.slots.remove(slot) {
            unanimated(|| layers.clip.removeFromSuperlayer());
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cells_colours_are_laid_out_as_the_shader_reads_them() {
        assert_eq!(mem::size_of::<CellColors>(), 128);
        let cells = CellColors { cols: 1, rows: 2, _pad: [0; 2], colors: [[0.0; 4]; SHADES] };
        let offset = (&cells.colors as *const _ as usize) - (&cells as *const CellColors as usize);
        assert_eq!(offset, 16, "float4 colors[7] after two uints and a uint2");
    }
}
