//! Draws the gradient backdrops (`gradients.rs`) with a Metal shader of Wings' own (`gradient.metal`), into frames GPUI
//! draws as surfaces (`Window::paint_surface`). GPUI's surface shader reads only 4:2:0 bi-planar full range YCbCr
//! (`kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`, gpui-pre-apple's `metal_renderer.rs` `draw_surfaces`), so the
//! shader draws each frame straight into its two planes: the luma, then the chroma at half the size.
//!
//! Each slot has three IOSurface-backed frames drawn into in turn on a command queue of its own. The slot paints the
//! newest frame the GPU has finished, so drawing one never waits on the GPU, and the frame drawn into next is never
//! the one handed to the window.

use std::{collections::HashMap, ffi::c_void, mem, ptr, ptr::NonNull};

use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer, CVPixelBufferCreate,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetWidthOfPlane, kCVMetalTextureUsage,
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVReturnSuccess,
};
use objc2_foundation::NSString;
use objc2_metal::{
    MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice,
    MTLDevice, MTLLibrary, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLStoreAction, MTLTextureUsage,
};

use super::backdrop::{Shade, SlotKey};
use crate::gradients::{Ring, Uniforms};

/// A frame of a gradient, as the window draws it.
pub(crate) type Frame = CFRetained<CVPixelBuffer>;

const SOURCE: &str = include_str!("gradient.metal");

/// A frame and its two planes as textures the shader draws into, with the drawing that last went into it.
struct Target {
    frame: Frame,
    luma: CFRetained<CVMetalTexture>,
    chroma: CFRetained<CVMetalTexture>,
    size: (u32, u32),
    work: Option<Retained<ProtocolObject<dyn MTLCommandBuffer>>>,
}

pub(crate) struct Metal {
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    luma: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    chroma: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    cache: CFRetained<CVMetalTextureCache>,
    slots: HashMap<SlotKey, Ring<Target, 3>>,
}

impl Metal {
    /// The system's GPU, the shader compiled and the two pipelines made, or why not.
    pub(crate) fn new() -> Result<Self, String> {
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        let library = device
            .newLibraryWithSource_options_error(&NSString::from_str(SOURCE), None)
            .map_err(|error| format!("compiling the gradient shader: {error:?}"))?;
        let pipeline = |fragment: &str, format: MTLPixelFormat| -> Result<_, String> {
            let function = |name: &str| {
                library.newFunctionWithName(&NSString::from_str(name)).ok_or(format!("no {name} in the gradient shader"))
            };
            let descriptor = MTLRenderPipelineDescriptor::new();
            descriptor.setVertexFunction(Some(&*function("gradient_vertex")?));
            descriptor.setFragmentFunction(Some(&*function(fragment)?));
            // SAFETY: Index zero is within the pipeline's colour attachments.
            unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(format);
            device.newRenderPipelineStateWithDescriptor_error(&descriptor).map_err(|error| format!("{fragment}: {error:?}"))
        };
        let luma = pipeline("gradient_luma", MTLPixelFormat::R8Unorm)?;
        let chroma = pipeline("gradient_chroma", MTLPixelFormat::RG8Unorm)?;
        let queue = device.newCommandQueue().ok_or("no Metal command queue")?;
        // The planes' textures are drawn into, and GPUI reads them.
        let usage = CFNumber::new_i64((MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead).0 as i64);
        // SAFETY: A static key Core Video defines.
        let keys: [&CFString; 1] = [unsafe { kCVMetalTextureUsage }];
        let values: [&CFType; 1] = [AsRef::<CFType>::as_ref(&*usage)];
        let attributes = CFDictionary::from_slices(&keys, &values);
        let mut cache = ptr::null_mut();
        // SAFETY: The output pointer is writable, and the attributes are a dictionary of CFString keys.
        let status = unsafe {
            CVMetalTextureCache::create(None, None, &device, Some(attributes.as_opaque()), NonNull::from(&mut cache))
        };
        if status != kCVReturnSuccess {
            return Err(format!("making a Core Video texture cache: status {status}"));
        }
        let cache = NonNull::new(cache).ok_or("no Core Video texture cache")?;
        // SAFETY: A successful create hands over this cache.
        let cache = unsafe { CFRetained::from_raw(cache) };
        Ok(Self { queue, luma, chroma, cache, slots: HashMap::new() })
    }
}

/// A new frame `size` pixels big, IOSurface-backed so Metal can draw into it, and its planes as textures.
fn target(cache: &CVMetalTextureCache, size: (u32, u32)) -> Result<Target, String> {
    let empty = CFDictionary::<CFString, CFType>::empty();
    // SAFETY: Static keys Core Video defines.
    let keys: [&CFString; 2] = unsafe { [kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey] };
    let values: [&CFType; 2] = [AsRef::<CFType>::as_ref(&*empty), AsRef::<CFType>::as_ref(CFBoolean::new(true))];
    let attributes = CFDictionary::from_slices(&keys, &values);
    let mut frame = ptr::null_mut();
    // SAFETY: The output pointer is writable, and the attributes are a dictionary of CFString keys.
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            size.0 as usize,
            size.1 as usize,
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
            Some(attributes.as_opaque()),
            NonNull::from(&mut frame),
        )
    };
    if status != kCVReturnSuccess {
        return Err(format!("making a {}x{} frame: status {status}", size.0, size.1));
    }
    // SAFETY: A successful create hands over this buffer.
    let frame = unsafe { CFRetained::from_raw(NonNull::new(frame).ok_or("no frame")?) };
    let plane = |index: usize, format: MTLPixelFormat| -> Result<CFRetained<CVMetalTexture>, String> {
        let (width, height) = (CVPixelBufferGetWidthOfPlane(&frame, index), CVPixelBufferGetHeightOfPlane(&frame, index));
        let mut texture = ptr::null_mut();
        // SAFETY: The frame has two planes of these formats and sizes, and the output pointer is writable.
        let status = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                cache,
                &frame,
                None,
                format,
                width,
                height,
                index,
                NonNull::from(&mut texture),
            )
        };
        if status != kCVReturnSuccess {
            return Err(format!("a texture of plane {index}: status {status}"));
        }
        // SAFETY: A successful create hands over this texture.
        Ok(unsafe { CFRetained::from_raw(NonNull::new(texture).ok_or("no plane texture")?) })
    };
    let luma = plane(0, MTLPixelFormat::R8Unorm)?;
    let chroma = plane(1, MTLPixelFormat::RG8Unorm)?;
    Ok(Target { frame, luma, chroma, size, work: None })
}

/// Encodes a draw of the whole of `texture` with `pipeline`.
fn pass(
    work: &ProtocolObject<dyn MTLCommandBuffer>,
    pipeline: &ProtocolObject<dyn MTLRenderPipelineState>,
    texture: &CVMetalTexture,
    uniforms: &Uniforms,
) -> Result<(), String> {
    let texture = CVMetalTextureGetTexture(texture).ok_or("a plane without a Metal texture")?;
    let descriptor = MTLRenderPassDescriptor::new();
    // SAFETY: Index zero is within the pass's colour attachments.
    let attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    attachment.setTexture(Some(&*texture));
    // Every pixel is drawn, so what was there before needn't be loaded.
    attachment.setLoadAction(MTLLoadAction::DontCare);
    attachment.setStoreAction(MTLStoreAction::Store);
    let encoder = work.renderCommandEncoderWithDescriptor(&descriptor).ok_or("no render encoder")?;
    encoder.setRenderPipelineState(pipeline);
    // SAFETY: The bytes are a live `Uniforms`, laid out as the shader's `Gradient` at buffer 0, which Metal copies; three
    // vertices make the one triangle `gradient_vertex` draws.
    unsafe {
        encoder.setFragmentBytes_length_atIndex(
            NonNull::from(uniforms).cast::<c_void>(),
            mem::size_of::<Uniforms>(),
            0,
        );
        encoder.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
    }
    encoder.endEncoding();
    Ok(())
}

impl Shade for Metal {
    fn render(&mut self, slot: &SlotKey, uniforms: &Uniforms) -> Result<(), String> {
        let size = (uniforms.size[0] as u32, uniforms.size[1] as u32);
        let ring = self.slots.entry(slot.clone()).or_default();
        // A slot that changed size starts its frames again; the window holds on to any it still draws.
        if ring.newest_first().next().is_some_and(|target| target.size != size) {
            *ring = Ring::default();
        }
        let target = ring.advance(|| target(&self.cache, size))?;
        let work = self.queue.commandBuffer().ok_or("no command buffer")?;
        pass(&work, &self.luma, &target.luma, uniforms)?;
        let half = Uniforms { target: [(size.0 / 2) as f32, (size.1 / 2) as f32], ..*uniforms };
        pass(&work, &self.chroma, &target.chroma, &half)?;
        work.commit();
        target.work = Some(work);
        Ok(())
    }

    fn latest(&mut self, slot: &SlotKey) -> Option<Frame> {
        let ring = self.slots.get(slot)?;
        let finished = |target: &&Target| {
            target.work.as_ref().is_some_and(|work| work.status() == MTLCommandBufferStatus::Completed)
        };
        if let Some(target) = ring.newest_first().find(finished) {
            return Some(target.frame.clone());
        }
        // The slot's first frame: wait for it once, rather than show the plain ground for a step.
        let newest = ring.newest_first().next()?;
        newest.work.as_ref()?.waitUntilCompleted();
        finished(&newest).then(|| newest.frame.clone())
    }

    fn forget(&mut self, keep: &dyn Fn(&SlotKey) -> bool) {
        self.slots.retain(|slot, _| keep(slot));
    }
}
