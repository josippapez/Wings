//! The window's spinners turning in Core Animation (`spinner_host`), over GPUI's drawing of the window: a layer in GPUI's
//! view's own layer, above what GPUI draws in it, holding each spinner's ring as `status_dot::paint_ring` paints it. Its
//! quarter turns once a second in the render server, so a step draws no frame of the window. A layer added to a view's
//! layer doesn't take the pointer: AppKit asks the view, not its layers, where a click goes.

use std::{collections::HashMap, f64::consts::TAU, ptr};

use gpui::{Bounds, Window};
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSView;
use objc2_core_foundation::{CFRetained, CGFloat};
use objc2_core_graphics::{CGColor, CGColorSpace, CGMutablePath, CGPath};
use objc2_foundation::{NSNumber, NSString};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming, CAShapeLayer};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{
    native_backdrop::{rect, slots_layer, unanimated},
    spinner_host::{Key, Placed, Present},
};

/// The key the turning animation goes by on a spinner's arc.
const SPIN: &str = "spin";

/// A spinner's layers: the one that cuts it as GPUI clips its slot, the faint ring in it, and the quarter that turns.
struct Layers {
    clip: Retained<CALayer>,
    ring: Retained<CAShapeLayer>,
    arc: Retained<CAShapeLayer>,
    /// The width its paths were made for, the colour they are in, and whether the arc turns.
    width: Option<f64>,
    color: Option<u32>,
    turning: bool,
}

impl Layers {
    fn new(spinners: &CALayer) -> Self {
        let clip = CALayer::new();
        clip.setMasksToBounds(true);
        let ring = stroke();
        let arc = stroke();
        unanimated(|| {
            clip.addSublayer(&ring);
            clip.addSublayer(&arc);
            spinners.addSublayer(&clip);
        });
        Self { clip, ring, arc, width: None, color: None, turning: false }
    }

    /// Puts the spinner as `placed`, starting its arc `phase` of a turn round if it starts turning now.
    fn place(&mut self, placed: &Placed, phase: f64) {
        let inside = Bounds::new(placed.bounds.origin - placed.clip.origin, placed.bounds.size);
        let width = f64::from(f32::from(placed.bounds.size.width));
        unanimated(|| {
            self.clip.setFrame(rect(placed.clip));
            let layers: [&CALayer; 3] = [&self.clip, &self.ring, &self.arc];
            for layer in layers {
                layer.setContentsScale(f64::from(placed.scale));
            }
            // Untransformed in the model, which the animation only turns as it shows, so the frame is the slot's.
            self.ring.setFrame(rect(inside));
            self.arc.setFrame(rect(inside));
            if self.width != Some(width) {
                // As `paint_ring`: a 1.5-point line round the middle of the box, 0.75 in from its edge, and its quarter
                // centred on the top, in the flipped layer's points down from the top, as GPUI's.
                let (middle, radius) = (width / 2.0, width / 2.0 - 0.75);
                self.ring.setPath(Some(&*arc(middle, radius, 0.0, TAU)));
                self.arc.setPath(Some(&*arc(middle, radius, -TAU / 4.0 - TAU / 8.0, -TAU / 4.0 + TAU / 8.0)));
                self.width = Some(width);
            }
            if self.color != Some(placed.color) {
                self.ring.setStrokeColor(color(placed.color, f64::from(0x4d_u8) / 255.0).as_deref());
                self.arc.setStrokeColor(color(placed.color, 1.0).as_deref());
                self.color = Some(placed.color);
            }
            let key = NSString::from_str(SPIN);
            match (placed.still, self.turning) {
                (false, false) => self.arc.addAnimation_forKey(&spin(phase), Some(&*key)),
                (true, true) => self.arc.removeAnimationForKey(&key),
                _ => {}
            }
            self.turning = !placed.still;
        });
    }
}

/// A shape layer that strokes its path 1.5 points wide and fills nothing.
fn stroke() -> Retained<CAShapeLayer> {
    let layer = CAShapeLayer::new();
    layer.setLineWidth(1.5);
    layer.setFillColor(None);
    layer
}

/// The arc round (`middle`, `middle`) `radius` points from it, from the angle `from` to `to`, growing.
fn arc(middle: f64, radius: f64, from: f64, to: f64) -> CFRetained<CGPath> {
    let path = CGMutablePath::new();
    // SAFETY: No transform: the pointer is null, which the function takes as none.
    unsafe { CGMutablePath::add_arc(Some(&*path), ptr::null(), middle, middle, radius, from, to, false) };
    CFRetained::<CGPath>::from(&*path)
}

/// `rgb` at `alpha`, as GPUI's view shows it: its Metal layer has no colour space, so its pixels go to the display as
/// they are, as a device RGB colour's do.
fn color(rgb: u32, alpha: f64) -> Option<CFRetained<CGColor>> {
    let space = CGColorSpace::new_device_rgb()?;
    let channel = |shift: u32| CGFloat::from(((rgb >> shift) & 0xff) as u8) / 255.0;
    let components: [CGFloat; 4] = [channel(16), channel(8), channel(0), alpha];
    // SAFETY: The space's three components and alpha, live for the call.
    unsafe { CGColor::new(Some(&*space), components.as_ptr()) }
}

/// A clockwise turn a second, for ever, `phase` of a turn in from its start.
fn spin(phase: f64) -> Retained<CABasicAnimation> {
    let spin = CABasicAnimation::animationWithKeyPath(Some(&*NSString::from_str("transform.rotation.z")));
    let (from, to) = (NSNumber::numberWithDouble(0.0), NSNumber::numberWithDouble(TAU));
    let (from, to): (&AnyObject, &AnyObject) = (&from, &to);
    // SAFETY: A rotation's angles are numbers.
    unsafe {
        spin.setFromValue(Some(from));
        spin.setToValue(Some(to));
    }
    spin.setDuration(1.0);
    spin.setRepeatCount(f32::INFINITY);
    spin.setTimeOffset(phase);
    spin
}

/// The window's spinners in a layer over GPUI's drawing.
pub(crate) struct Host {
    /// What the spinners' layers are in (`slots_layer`), in GPUI's view's layer.
    spinners: Retained<CALayer>,
    layers: HashMap<Key, Layers>,
}

impl Host {
    /// A layer of its own over GPUI's drawing in `window`, or why not.
    pub(crate) fn new(window: &Window) -> Result<Self, String> {
        let handle = HasWindowHandle::window_handle(window).map_err(|error| format!("no window handle: {error}"))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err("not an AppKit window".to_owned());
        };
        // SAFETY: GPUI's window view, alive while its window is.
        let gpui = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        // SAFETY: `layer` takes nothing and returns the view's layer, if it has one: GPUI makes its view layer-backed,
        // with its Metal layer, as it sets the window up.
        let root: Option<Retained<CALayer>> = unsafe { msg_send![gpui, layer] };
        let root = root.ok_or("GPUI's view has no layer")?;
        Ok(Self { spinners: slots_layer(&root), layers: HashMap::new() })
    }
}

impl Present for Host {
    fn place(&mut self, key: &Key, placed: &Placed, phase: f64) {
        let spinners = &self.spinners;
        self.layers.entry(key.clone()).or_insert_with(|| Layers::new(spinners)).place(placed, phase);
    }

    fn remove(&mut self, key: &Key) {
        if let Some(layers) = self.layers.remove(key) {
            unanimated(|| layers.clip.removeFromSuperlayer());
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        unanimated(|| self.spinners.removeFromSuperlayer());
    }
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size};
    use objc2_core_foundation::{CGAffineTransform, CGPoint, CGRect, CGSize};

    use super::*;

    fn near(at: CGPoint, x: f64, y: f64) -> bool {
        (at.x - x).abs() < 1e-6 && (at.y - y).abs() < 1e-6
    }

    /// GPUI's view's layer is unflipped, as AppKit leaves a view that isn't: `y` up from the window's bottom. A ring at a
    /// slot counted down from the top lands there, its quarter on top, and a positive turn, as the animation's
    /// `transform.rotation.z` gives it, takes the quarter round to the right: clockwise on screen, as `paint_ring` turns.
    #[test]
    fn a_ring_lands_on_its_slot_with_its_quarter_on_top_and_turns_clockwise() {
        unanimated(|| {
            let root = CALayer::new();
            root.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(800.0, 600.0)));
            let spinners = slots_layer(&root);
            let mut layers = Layers::new(&spinners);
            let placed = Placed {
                bounds: Bounds::new(point(px(100.0), px(50.0)), size(px(10.0), px(10.0))),
                clip: Bounds::new(point(px(0.0), px(40.0)), size(px(800.0), px(560.0))),
                color: 0xe9b65f,
                scale: 2.0,
                still: true,
            };
            layers.place(&placed, 0.0);
            // The middle of the quarter: the ring's top, 0.75 under the slot's top edge, 50 points down the window.
            let top = CGPoint::new(5.0, 0.75);
            let landed = layers.arc.convertPoint_toLayer(top, Some(&*root));
            assert!(near(landed, 105.0, 600.0 - 50.75), "the quarter on the ring's top: {landed:?}");
            let clip = layers.clip.convertRect_toLayer(layers.clip.bounds(), Some(&*root));
            assert_eq!((clip.origin.y, clip.size.height), (0.0, 560.0), "cut where GPUI clips it");
            // A quarter of a turn.
            layers.arc.setAffineTransform(CGAffineTransform { a: 0.0, b: 1.0, c: -1.0, d: 0.0, tx: 0.0, ty: 0.0 });
            let turned = layers.arc.convertPoint_toLayer(top, Some(&*root));
            assert!(near(turned, 109.25, 600.0 - 55.0), "the quarter on the ring's right: {turned:?}");
        });
    }

    #[test]
    fn a_ring_turns_until_reduce_motion_stops_it_and_a_move_keeps_it_turning() {
        unanimated(|| {
            let root = CALayer::new();
            root.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(800.0, 600.0)));
            let spinners = slots_layer(&root);
            let mut layers = Layers::new(&spinners);
            let mut placed = Placed {
                bounds: Bounds::new(point(px(10.0), px(10.0)), size(px(10.0), px(10.0))),
                clip: Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0))),
                color: 0xe9b65f,
                scale: 2.0,
                still: false,
            };
            let key = NSString::from_str(SPIN);
            let animation = |layers: &Layers| unsafe { layers.arc.animationForKey(&key) };
            layers.place(&placed, 0.25);
            assert_eq!(animation(&layers).expect("it turns").timeOffset(), 0.25, "a quarter of a turn in");
            placed.bounds.origin.y = px(30.0);
            layers.place(&placed, 0.5);
            let after = animation(&layers).expect("it still turns");
            assert_eq!(after.timeOffset(), 0.25, "the same turn, not started again at the clock's half turn");
            placed.still = true;
            layers.place(&placed, 0.5);
            assert!(animation(&layers).is_none(), "standing still under Reduce motion");
        });
    }
}
